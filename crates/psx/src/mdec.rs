//! MDEC (0x1F801820–0x1F801827): descompressor de imagens no estilo JPEG,
//! usado nos vídeos.
//!
//! A CPU (ou o DMA 0) escreve o comando e os parâmetros; o comando 1 recebe
//! blocos comprimidos por run-length, que passam pela quantização inversa,
//! pela IDCT e, nas cores, pela conversão YUV→RGB. O resultado sai numa
//! fila lida pela CPU ou pelo DMA 1.
//!
//! A decodificação é instantânea: cada bloco é processado quando chega a
//! última palavra dele. A porta de dados entrega os macroblocos coloridos
//! em quatro blocos 8×8 (Y1..Y4); o DMA 1 os reordena em linhas de 16
//! pixels.

use std::collections::VecDeque;

/// Ordem em zigue-zague: `ZAGZIG[k]` é a posição (x + 8·y) do k-ésimo
/// coeficiente.
const ZAGZIG: [usize; 64] = {
    const ZIGZAG: [usize; 64] = [
        0, 1, 5, 6, 14, 15, 27, 28, 2, 4, 7, 13, 16, 26, 29, 42, 3, 8, 12, 17, 25, 30, 41, 43, 9,
        11, 18, 24, 31, 40, 44, 53, 10, 19, 23, 32, 39, 45, 52, 54, 20, 22, 33, 38, 46, 51, 55, 60,
        21, 34, 37, 47, 50, 56, 59, 61, 35, 36, 48, 49, 57, 58, 62, 63,
    ];
    let mut table = [0; 64];
    let mut i = 0;
    while i < 64 {
        table[ZIGZAG[i]] = i;
        i += 1;
    }
    table
};

/// Tabela da IDCT que os jogos sempre enviam (MDEC(3)).
#[rustfmt::skip]
const STANDARD_SCALE: [u16; 64] = [
    0x5A82, 0x5A82, 0x5A82, 0x5A82, 0x5A82, 0x5A82, 0x5A82, 0x5A82,
    0x7D8A, 0x6A6D, 0x471C, 0x18F8, 0xE707, 0xB8E3, 0x9592, 0x8275,
    0x7641, 0x30FB, 0xCF04, 0x89BE, 0x89BE, 0xCF04, 0x30FB, 0x7641,
    0x6A6D, 0xE707, 0x8275, 0xB8E3, 0x471C, 0x7D8A, 0x18F8, 0x9592,
    0x5A82, 0xA57D, 0xA57D, 0x5A82, 0x5A82, 0xA57D, 0xA57D, 0x5A82,
    0x471C, 0x8275, 0x18F8, 0x6A6D, 0x9592, 0xE707, 0x7D8A, 0xB8E3,
    0x30FB, 0x89BE, 0x7641, 0xCF04, 0xCF04, 0x7641, 0x89BE, 0x30FB,
    0x18F8, 0xB8E3, 0x6A6D, 0x8275, 0x7D8A, 0x9592, 0x471C, 0xE707,
];

/// Fim de bloco (e enchimento, no começo de um bloco).
const END_OF_BLOCK: u16 = 0xFE00;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Depth {
    Four,
    Eight,
    TwentyFour,
    Fifteen,
}

impl Depth {
    fn colored(self) -> bool {
        matches!(self, Self::TwentyFour | Self::Fifteen)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Idle,
    Decode,
    QuantTables { color: bool },
    ScaleTable,
}

/// Bloco sendo montado pelo run-length.
#[derive(Clone, Copy)]
struct Block {
    coefficients: [i16; 64],
    /// Próximo coeficiente; `None` antes do primeiro (DC).
    k: Option<usize>,
    q_scale: i32,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            coefficients: [0; 64],
            k: None,
            q_scale: 0,
        }
    }
}

fn signed10(value: u16) -> i32 {
    i32::from((value << 6) as i16 >> 6)
}

#[derive(Clone)]
pub(crate) struct Mdec {
    command: Command,
    /// Bits 25–28 do último comando (profundidade, sinal e bit 15).
    command_bits: u32,
    /// Palavras de parâmetro que ainda faltam.
    remaining: u32,
    /// Bits 0–15 do status quando não vêm de `remaining`: 0 depois do
    /// reset e o eco do comando sem função.
    status_low: Option<u32>,
    dma_in: bool,
    dma_out: bool,
    params: Vec<u32>,
    luma_quant: [u8; 64],
    chroma_quant: [u8; 64],
    scale: [i16; 64],
    block: Block,
    /// Blocos do macrobloco colorido: Cr, Cb e a saída RGB (16×16).
    current: usize,
    cr: [i16; 64],
    cb: [i16; 64],
    rgb: [[u8; 3]; 256],
    output: VecDeque<u32>,
    /// Macrobloco colorido já reordenado para o DMA 1.
    dma_output: VecDeque<u32>,
    /// Palavras lidas do macrobloco em saída (para o bloco atual no status).
    read_in_macroblock: usize,
}

impl Default for Mdec {
    fn default() -> Self {
        Self {
            command: Command::Idle,
            command_bits: 0,
            remaining: 0,
            status_low: Some(0),
            dma_in: false,
            dma_out: false,
            params: Vec::with_capacity(32),
            luma_quant: [0; 64],
            chroma_quant: [0; 64],
            scale: STANDARD_SCALE.map(|v| v as i16),
            block: Block::default(),
            current: 0,
            cr: [0; 64],
            cb: [0; 64],
            rgb: [[0; 3]; 256],
            output: VecDeque::new(),
            dma_output: VecDeque::new(),
            read_in_macroblock: 0,
        }
    }
}

impl Mdec {
    fn depth(&self) -> Depth {
        match (self.command_bits >> 27) & 3 {
            0 => Depth::Four,
            1 => Depth::Eight,
            2 => Depth::TwentyFour,
            _ => Depth::Fifteen,
        }
    }

    fn signed(&self) -> bool {
        self.command_bits & (1 << 26) != 0
    }

    fn set_bit15(&self) -> bool {
        self.command_bits & (1 << 25) != 0
    }

    fn words_per_macroblock(&self) -> usize {
        match self.depth() {
            Depth::Four => 8,
            Depth::Eight => 16,
            Depth::TwentyFour => 192,
            Depth::Fifteen => 128,
        }
    }

    // --- Registradores ---

    /// Lê a próxima palavra decodificada (lixo, aqui 0, se não houver).
    pub(crate) fn read_data(&mut self) -> u32 {
        let Some(word) = self.output.pop_front() else {
            return 0;
        };
        self.read_in_macroblock = (self.read_in_macroblock + 1) % self.words_per_macroblock();
        self.finish_if_done();
        word
    }

    pub(crate) fn status(&self) -> u32 {
        let block = if !self.output.is_empty() || !self.dma_output.is_empty() {
            if self.depth().colored() {
                (self.read_in_macroblock * 4 / self.words_per_macroblock()) as u32
            } else {
                4
            }
        } else if self.command == Command::Decode && self.depth().colored() {
            // Cr e Cb chegam antes de Y1..Y4.
            [4, 5, 0, 1, 2, 3][self.current] as u32
        } else {
            4
        };
        let busy = self.command != Command::Idle;
        u32::from(self.output_len() == 0) << 31
            | u32::from(busy) << 29
            | u32::from(self.dma_in && self.remaining > 0) << 28
            | u32::from(self.dma_out && self.output_len() > 0) << 27
            | (self.command_bits >> 25 & 0xF) << 23
            | block << 16
            | self
                .status_low
                .unwrap_or(self.remaining.wrapping_sub(1) & 0xFFFF)
    }

    /// Palavras prontas para o DMA 1.
    pub(crate) fn output_len(&self) -> usize {
        self.output.len() + self.dma_output.len()
    }

    /// Palavra lida pelo DMA 1: nos coloridos, o macrobloco sai em linhas de
    /// 16 pixels em vez de blocos 8×8.
    pub(crate) fn dma_read(&mut self) -> u32 {
        let words = self.words_per_macroblock();
        if self.depth().colored() && self.dma_output.is_empty() && self.output.len() >= words {
            // Palavras por linha de um bloco: 8 pixels de 2 ou 3 bytes.
            let row = words / 32;
            let mut macroblock = Vec::with_capacity(words);
            for _ in 0..words {
                macroblock.push(self.read_data());
            }
            for line in 0..16 {
                for half in 0..2 {
                    let block = line / 8 * 2 + half;
                    let at = block * words / 4 + line % 8 * row;
                    self.dma_output.extend(&macroblock[at..at + row]);
                }
            }
        }
        match self.dma_output.pop_front() {
            Some(word) => word,
            None => self.read_data(),
        }
    }

    /// Registrador de controle: reset (bit 31) e pedidos de DMA (30, 29).
    pub(crate) fn control(&mut self, value: u32) {
        if value & (1 << 31) != 0 {
            // As tabelas continuam valendo.
            *self = Self {
                luma_quant: self.luma_quant,
                chroma_quant: self.chroma_quant,
                scale: self.scale,
                ..Self::default()
            };
        }
        self.dma_in = value & (1 << 30) != 0;
        self.dma_out = value & (1 << 29) != 0;
    }

    /// Comando ou parâmetro (pela CPU ou pelo DMA 0).
    pub(crate) fn write(&mut self, word: u32) {
        if self.remaining == 0 {
            self.start(word);
            return;
        }
        self.remaining -= 1;
        match self.command {
            Command::Decode => {
                self.feed(word as u16);
                self.feed((word >> 16) as u16);
                if self.remaining == 0 {
                    self.finish_if_done();
                }
            }
            Command::QuantTables { .. } | Command::ScaleTable => {
                self.params.push(word);
                if self.remaining == 0 {
                    self.load_tables();
                }
            }
            Command::Idle => {}
        }
    }

    fn start(&mut self, word: u32) {
        self.command_bits = word & 0x1E00_0000;
        self.params.clear();
        let (command, remaining) = match word >> 29 {
            1 => (Command::Decode, word & 0xFFFF),
            2 => {
                let color = word & 1 != 0;
                (Command::QuantTables { color }, if color { 32 } else { 16 })
            }
            3 => (Command::ScaleTable, 32),
            _ => (Command::Idle, 0),
        };
        self.command = command;
        self.remaining = remaining;
        // Sem função: os bits 0–15 aparecem no status, sem o "menos 1".
        self.status_low = (command == Command::Idle).then_some(word & 0xFFFF);
        self.block = Block::default();
        self.current = 0;
        self.read_in_macroblock = 0;
        if self.remaining == 0 {
            self.finish_if_done();
        }
    }

    /// O comando termina quando todos os parâmetros chegaram e a saída foi
    /// lida.
    fn finish_if_done(&mut self) {
        if self.remaining == 0 && self.output_len() == 0 {
            self.command = Command::Idle;
        }
    }

    fn load_tables(&mut self) {
        let bytes: Vec<u8> = self.params.iter().flat_map(|w| w.to_le_bytes()).collect();
        match self.command {
            Command::QuantTables { color } => {
                self.luma_quant.copy_from_slice(&bytes[..64]);
                if color {
                    self.chroma_quant.copy_from_slice(&bytes[64..128]);
                }
            }
            Command::ScaleTable => {
                for (value, pair) in self.scale.iter_mut().zip(bytes.as_chunks::<2>().0) {
                    *value = i16::from_le_bytes(*pair);
                }
            }
            Command::Decode | Command::Idle => {}
        }
        self.command = Command::Idle;
    }

    // --- Decodificação ---

    fn quant_table(&self) -> &[u8; 64] {
        if self.depth().colored() && self.current < 2 {
            &self.chroma_quant
        } else {
            &self.luma_quant
        }
    }

    /// Recebe uma meia-palavra do run-length.
    fn feed(&mut self, value: u16) {
        let qt = *self.quant_table();
        let block = &mut self.block;
        let k = match block.k {
            None if value == END_OF_BLOCK => return,
            None => {
                block.q_scale = i32::from(value >> 10);
                0
            }
            Some(k) => {
                let k = k + usize::from(value >> 10) + 1;
                if k > 63 {
                    self.finish_block();
                    return;
                }
                k
            }
        };
        let level = signed10(value);
        let coefficient = if block.q_scale == 0 {
            level * 2
        } else if k == 0 {
            level * i32::from(qt[0])
        } else {
            (level * i32::from(qt[k]) * block.q_scale + 4) / 8
        };
        let coefficient = coefficient.clamp(-0x400, 0x3FF) as i16;
        let position = if block.q_scale == 0 { k } else { ZAGZIG[k] };
        block.coefficients[position] = coefficient;
        block.k = Some(k);
        // Bloco completo sem o código de fim.
        if k == 63 {
            self.finish_block();
        }
    }

    fn finish_block(&mut self) {
        let mut block = std::mem::take(&mut self.block).coefficients;
        self.idct(&mut block);
        if !self.depth().colored() {
            self.output_mono(&block);
            return;
        }
        match self.current {
            0 => self.cr = block,
            1 => self.cb = block,
            n => self.yuv_to_rgb(&block, (n - 2) % 2 * 8, (n - 2) / 2 * 8),
        }
        self.current += 1;
        if self.current == 6 {
            self.current = 0;
            self.output_colored();
        }
    }

    /// IDCT em duas passadas (colunas e linhas) com os 13 bits altos da
    /// tabela de escala, como o hardware. A fórmula exata do hardware não é
    /// conhecida; arredondar uma vez só, no fim, foi o que chegou mais perto
    /// das imagens de referência do ps1-tests (diferença de no máximo 1 em
    /// 5 bits). O resultado vai para 9 bits com sinal, saturado em 8.
    fn idct(&self, block: &mut [i16; 64]) {
        let table = self.scale.map(|t| i64::from(t >> 3));
        let mut values = block.map(i64::from);
        for _ in 0..2 {
            let mut out = [0; 64];
            for x in 0..8 {
                for y in 0..8 {
                    out[x + y * 8] = (0..8).map(|z| values[y + z * 8] * table[x + z * 8]).sum();
                }
            }
            values = out;
        }
        for (pixel, value) in block.iter_mut().zip(values) {
            let value = (value + (1 << 25)) >> 26;
            *pixel = ((value << 55) >> 55).clamp(-128, 127) as i16;
        }
    }

    fn output_mono(&mut self, block: &[i16; 64]) {
        let flip = if self.signed() { 0 } else { 0x80 };
        match self.depth() {
            Depth::Four => {
                // Arredonda para 4 bits com saturação (medido no hardware).
                let nibbles =
                    block.map(|y| ((((y + 8) >> 4).clamp(-8, 7) as u8) & 0xF) ^ (flip >> 4));
                for chunk in nibbles.as_chunks::<8>().0 {
                    let word = chunk
                        .iter()
                        .enumerate()
                        .fold(0, |w, (i, &p)| w | u32::from(p) << (4 * i));
                    self.output.push_back(word);
                }
            }
            _ => {
                let pixels = block.map(|y| (y as u8) ^ flip);
                for chunk in pixels.as_chunks::<4>().0 {
                    self.output.push_back(u32::from_le_bytes(*chunk));
                }
            }
        }
    }

    fn yuv_to_rgb(&mut self, y_block: &[i16; 64], xx: usize, yy: usize) {
        let flip = if self.signed() { 0 } else { 0x80 };
        for y in 0..8 {
            for x in 0..8 {
                let chroma = (x + xx) / 2 + (y + yy) / 2 * 8;
                let (r, b) = (f32::from(self.cr[chroma]), f32::from(self.cb[chroma]));
                let g = (-0.3437 * b - 0.7143 * r).round() as i16;
                let (r, b) = ((1.402 * r).round() as i16, (1.772 * b).round() as i16);
                let luma = i32::from(y_block[x + y * 8]);
                let channel = |c: i16| ((luma + i32::from(c)).clamp(-128, 127) as u8) ^ flip;
                self.rgb[(x + xx) + (y + yy) * 16] = [channel(r), channel(g), channel(b)];
            }
        }
    }

    /// Põe o macrobloco na saída em blocos 8×8: Y1, Y2, Y3 e Y4.
    fn output_colored(&mut self) {
        let blocks = (0..4).flat_map(|b| {
            (0..8).flat_map(move |y| (0..8).map(move |x| (b % 2 * 8 + x) + (b / 2 * 8 + y) * 16))
        });
        if self.depth() == Depth::TwentyFour {
            let bytes: Vec<u8> = blocks.flat_map(|i| self.rgb[i]).collect();
            for chunk in bytes.as_chunks::<4>().0 {
                self.output.push_back(u32::from_le_bytes(*chunk));
            }
            return;
        }
        let bit15 = u16::from(self.set_bit15()) << 15;
        let pixels: Vec<u16> = blocks
            .map(|i| {
                let [r, g, b] = self.rgb[i];
                // Arredonda para 5 bits (mais perto do hardware que truncar).
                let five = |c: u8| u16::from(c.saturating_add(4) >> 3);
                five(r) | five(g) << 5 | five(b) << 10 | bit15
            })
            .collect();
        for pair in pixels.as_chunks::<2>().0 {
            self.output
                .push_back(u32::from(pair[0]) | u32::from(pair[1]) << 16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bloco do "heart" do ps1-tests (64 coeficientes, sem fim de bloco).
    #[rustfmt::skip]
    const HEART: [u8; 128] = [
        0x1e, 0x04, 0x74, 0x00, 0xbd, 0x00, 0x93, 0x03, 0x24, 0x00, 0x2b, 0x03,
        0x15, 0x00, 0xf9, 0x03, 0xe9, 0x03, 0xda, 0x03, 0xee, 0x03, 0xfe, 0x03,
        0x2b, 0x00, 0x3f, 0x00, 0xc0, 0x03, 0xfe, 0x03, 0xbe, 0x03, 0xfe, 0x03,
        0xca, 0x03, 0xfd, 0x03, 0xf3, 0x03, 0xf3, 0x03, 0xfd, 0x03, 0x07, 0x00,
        0xd7, 0x03, 0x00, 0x00, 0xe0, 0x03, 0xf5, 0x03, 0x05, 0x00, 0x02, 0x00,
        0xec, 0x03, 0x07, 0x00, 0x0c, 0x00, 0x08, 0x00, 0xfe, 0x03, 0xf7, 0x03,
        0xff, 0x03, 0xf9, 0x03, 0x01, 0x00, 0xdd, 0x03, 0xd8, 0x03, 0x09, 0x00,
        0x16, 0x00, 0x01, 0x00, 0x13, 0x00, 0xca, 0x03, 0x04, 0x00, 0xfa, 0x03,
        0xfc, 0x03, 0xfe, 0x03, 0x00, 0x00, 0x09, 0x00, 0x0f, 0x00, 0x06, 0x00,
        0xfe, 0x03, 0xfc, 0x03, 0xf2, 0x03, 0xfa, 0x03, 0xf6, 0x03, 0x08, 0x00,
        0xfa, 0x03, 0x15, 0x00, 0x01, 0x00, 0xf5, 0x03,
    ];

    /// Tabela de quantização do ps1-tests (a mesma para luma e croma).
    #[rustfmt::skip]
    const QUANT: [u8; 64] = [
        0x02, 0x10, 0x10, 0x13, 0x10, 0x13, 0x16, 0x16, 0x16, 0x16, 0x16, 0x16, 0x1a, 0x18, 0x1a,
        0x1b, 0x1b, 0x1b, 0x1a, 0x1a, 0x1a, 0x1a, 0x1b, 0x1b, 0x1b, 0x1d, 0x1d, 0x1d, 0x22, 0x22,
        0x22, 0x1d, 0x1d, 0x1d, 0x1b, 0x1b, 0x1d, 0x1d, 0x20, 0x20, 0x22, 0x22, 0x25, 0x26, 0x25,
        0x23, 0x23, 0x22, 0x23, 0x26, 0x26, 0x28, 0x28, 0x28, 0x30, 0x30, 0x2e, 0x2e, 0x38, 0x38,
        0x3a, 0x45, 0x45, 0x53,
    ];

    /// Tabela da IDCT do ps1-tests (difere em 1 da padrão em alguns
    /// valores negativos).
    #[rustfmt::skip]
    const IDCT: [i16; 64] = [
        23170, 23170, 23170, 23170, 23170, 23170, 23170, 23170, 32138, 27245, 18204, 6392, -6393,
        -18205, -27246, -32139, 30273, 12539, -12540, -30274, -30274, -12540, 12539, 30273, 27245,
        -6393, -32139, -18205, 18204, 32138, 6392, -27246, 23170, -23171, -23171, 23170, 23170,
        -23171, -23171, 23170, 18204, -32139, 6392, 27245, -27246, -6393, 32138, -18205, 12539,
        -30274, 30273, -12540, -12540, 30273, -30274, 12539, 6392, -18205, 27245, -32139, 32138,
        -27246, 18204, -6393,
    ];

    fn words(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
    }

    /// MDEC com as tabelas do ps1-tests, como o programa delas configura.
    fn configured() -> Mdec {
        let mut mdec = Mdec::default();
        mdec.control(1 << 31);
        mdec.write(2 << 29 | 1);
        for word in words(&[QUANT, QUANT].concat()) {
            mdec.write(word);
        }
        mdec.write(3 << 29);
        let idct: Vec<u8> = IDCT.iter().flat_map(|v| v.to_le_bytes()).collect();
        for word in words(&idct) {
            mdec.write(word);
        }
        assert_eq!(mdec.status(), 0x8004_FFFF, "livre, sem parâmetros");
        mdec
    }

    fn decode(mdec: &mut Mdec, command: u32, data: &[u8]) -> Vec<u8> {
        mdec.write(command | (data.len() / 4) as u32);
        for word in words(data) {
            mdec.write(word);
        }
        let mut out = Vec::new();
        while mdec.status() & (1 << 31) == 0 {
            out.extend(mdec.read_data().to_le_bytes());
        }
        out
    }

    /// A fórmula exata da IDCT do hardware não é conhecida (psx-spx): a
    /// saída fica a no máximo 2 níveis da medida no console.
    fn assert_close(out: &[u8], expected: &[u8], tolerance: i32) {
        assert_eq!(out.len(), expected.len());
        for (i, (&a, &b)) in out.iter().zip(expected).enumerate() {
            assert!(
                (i32::from(a) - i32::from(b)).abs() <= tolerance,
                "pixel {i}: {a:#04x}, hardware {b:#04x}"
            );
        }
    }

    /// Separa os pixels de 4 bits (o baixo primeiro).
    fn nibbles(bytes: &[u8]) -> Vec<u8> {
        bytes.iter().flat_map(|b| [b & 0xF, b >> 4]).collect()
    }

    #[test]
    fn heart_block_matches_hardware_in_8bit() {
        let mut mdec = configured();
        let out = decode(&mut mdec, 1 << 29 | 1 << 27, &HEART);
        #[rustfmt::skip]
        let expected = [
            0x00, 0xff, 0xff, 0x00, 0xff, 0xff, 0x04, 0x00,
            0xc9, 0xec, 0xef, 0xed, 0xef, 0xfc, 0xf2, 0x00,
            0xd5, 0xdb, 0xfa, 0xe8, 0xfe, 0xe8, 0xff, 0x00,
            0xb7, 0xf3, 0xec, 0xef, 0xeb, 0xff, 0xe3, 0x00,
            0x00, 0xfb, 0xff, 0xf5, 0xf2, 0xff, 0x03, 0x00,
            0x00, 0x05, 0xff, 0xfc, 0xff, 0x08, 0x1a, 0x00,
            0x0f, 0x28, 0x1e, 0xff, 0x05, 0x2a, 0x23, 0x00,
            0x10, 0x38, 0x40, 0x29, 0x32, 0x32, 0x16, 0x0f,
        ];
        assert_close(&out, &expected, 2);
        assert_eq!(mdec.status() & (1 << 29), 0, "comando terminou");
    }

    #[test]
    fn heart_block_matches_hardware_in_4bit() {
        let mut mdec = configured();
        let out = decode(&mut mdec, 1 << 29, &HEART);
        #[rustfmt::skip]
        let expected = [
            0xf0, 0x0f, 0xff, 0x00, 0xfd, 0xff, 0xff, 0x0f, 0xed, 0xff, 0xff, 0x0f,
            0xfb, 0xff, 0xff, 0x0e, 0xf0, 0xff, 0xff, 0x00, 0x00, 0xff, 0x1f, 0x02,
            0x31, 0xf2, 0x30, 0x02, 0x41, 0x34, 0x33, 0x11,
        ];
        assert_close(&nibbles(&out), &nibbles(&expected), 1);
    }

    /// Macrobloco só com DCs: Cr e Cb zerados e cada Y com um nível
    /// (`level` vira ~level/4 depois da quantização e da IDCT, com o
    /// QUANT[0] = 2). Começa com enchimento, que é ignorado.
    fn flat_macroblock(levels: [u16; 4]) -> Vec<u8> {
        let mut halfwords = vec![END_OF_BLOCK, 0x0400, END_OF_BLOCK, 0x0400, END_OF_BLOCK];
        for level in levels {
            halfwords.extend([0x0400 | level, END_OF_BLOCK]);
        }
        halfwords.push(END_OF_BLOCK);
        halfwords.iter().flat_map(|h| h.to_le_bytes()).collect()
    }

    #[test]
    fn colored_macroblock_comes_out_in_8x8_blocks_and_dma_reorders_it() {
        let data = flat_macroblock([40, 80, 120, 160]);
        let mut mdec = configured();
        let out = decode(&mut mdec, 1 << 29 | 2 << 27, &data);
        // 24 bits: 4 blocos de 8×8 pixels de 3 bytes, cinza (Y + 128).
        assert_eq!(out.len(), 768);
        for (block, gray) in [138, 148, 158, 168].into_iter().enumerate() {
            let bytes = &out[block * 192..(block + 1) * 192];
            assert!(bytes.iter().all(|&b| b == gray), "bloco {block}");
        }

        mdec.write(1 << 29 | 2 << 27 | (data.len() / 4) as u32);
        for word in words(&data) {
            mdec.write(word);
        }
        let mut dma: Vec<u8> = Vec::new();
        while mdec.output_len() > 0 {
            dma.extend(mdec.dma_read().to_le_bytes());
        }
        // Linhas de 16 pixels: 8 do bloco da esquerda e 8 da direita.
        let row = |line: usize| &dma[line * 48..(line + 1) * 48];
        assert!(row(0)[..24].iter().all(|&b| b == 138));
        assert!(row(0)[24..].iter().all(|&b| b == 148));
        assert!(row(15)[..24].iter().all(|&b| b == 158));
        assert!(row(15)[24..].iter().all(|&b| b == 168));
    }

    #[test]
    fn fifteen_bit_output_rounds_and_sets_bit_15() {
        let data = flat_macroblock([40, 40, 40, 40]);
        let mut mdec = configured();
        let out = decode(&mut mdec, 1 << 29 | 3 << 27 | 1 << 25, &data);
        assert_eq!(out.len(), 512);
        // 138 → 17,25 em 5 bits: arredonda para 17.
        let gray = 17 | 17 << 5 | 17 << 10 | 0x8000;
        let pixels: Vec<u16> = out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes(*p))
            .collect();
        assert!(pixels.iter().all(|&p| p == gray), "{:#06x}", pixels[0]);
    }

    #[test]
    fn status_tracks_commands_and_dma_requests() {
        let mut mdec = Mdec::default();
        assert_eq!(mdec.status(), 0x8004_0000, "depois de ligar");
        mdec.control(3 << 29);
        mdec.write(1 << 29 | 2 << 27 | 10);
        let status = mdec.status();
        assert_eq!(status & 0xFFFF, 9, "parâmetros que faltam menos 1");
        assert_ne!(status & (1 << 29), 0, "ocupado");
        assert_ne!(status & (1 << 28), 0, "pede dados de entrada");
        assert_eq!(status & (1 << 27), 0, "nada a entregar");
        assert_eq!((status >> 25) & 3, 2, "profundidade 24 bits");
        assert_eq!((status >> 16) & 7, 4, "esperando o Cr");

        mdec.control(1 << 31);
        assert_eq!(mdec.status(), 0x8004_0000, "reset aborta o comando");
        // Sem função: ecoa os bits 0–15, sem o "menos 1", e fica livre.
        mdec.write(0x1234);
        assert_eq!(mdec.status() & 0x2000_FFFF, 0x1234);
    }

    #[test]
    fn quant_tables_are_loaded_from_bytes() {
        let mut mdec = Mdec::default();
        mdec.write(2 << 29);
        for i in 0..16u32 {
            mdec.write(u32::from_le_bytes([0, 1, 2, 3].map(|b| (i * 4 + b) as u8)));
        }
        assert_eq!(mdec.luma_quant[63], 63);
        assert_eq!(mdec.chroma_quant, [0; 64], "só luma");
        assert_eq!(mdec.status() & 0x2000_FFFF, 0xFFFF, "terminou");
    }
}
