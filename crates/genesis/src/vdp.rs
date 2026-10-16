//! VDP do Mega Drive (modo 5), NTSC, 224 linhas, H32 (256) ou H40 (320).
//!
//! Renderiza linha a linha: [`Vdp::render_line`] é chamado pelo console no
//! meio de cada linha (depois do H-blank), para que mudanças feitas pela
//! interrupção de linha valham a partir da linha seguinte, como no hardware.
//! O DMA de 68000 → VDP é pedido aqui e executado pelo barramento, que tem
//! acesso à memória do 68000.

use chips::state::{Reader, StateError, Writer};

pub const MAX_WIDTH: usize = 320;
pub const HEIGHT: usize = 224;
pub const LINES_PER_FRAME: u16 = 262;

const VRAM_SIZE: usize = 0x10000;
const CRAM_WORDS: usize = 64;
const VSRAM_WORDS: usize = 40;

const STATUS_FIFO_EMPTY: u16 = 0x0200;
const STATUS_VINT: u16 = 0x0080;
const STATUS_OVERFLOW: u16 = 0x0040;
const STATUS_COLLISION: u16 = 0x0020;
const STATUS_VBLANK: u16 = 0x0008;
const STATUS_HBLANK: u16 = 0x0004;

/// Níveis de saída do DAC de cor (medidos), para os 8 valores de 3 bits.
const LEVELS: [u8; 8] = [0, 52, 87, 116, 144, 172, 206, 255];
/// Shadow/highlight: metade e metade + 128.
const SHADOW: [u8; 8] = [0, 26, 43, 58, 72, 86, 103, 127];
const HIGHLIGHT: [u8; 8] = [128, 154, 171, 186, 200, 214, 231, 255];

/// Transferência de DMA pedida ao barramento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DmaRequest {
    /// Endereço de origem no 68000 (par).
    pub source: u32,
    pub words: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    /// DMA de preenchimento: espera a próxima escrita na porta de dados.
    Fill,
}

#[derive(Debug, Clone)]
pub struct Vdp {
    vram: Box<[u8; VRAM_SIZE]>,
    cram: [u16; CRAM_WORDS],
    vsram: [u16; VSRAM_WORDS],
    regs: [u8; 24],
    /// Segunda palavra de comando esperada.
    command_pending: bool,
    code: u8,
    address: u16,
    status: u16,
    pending_dma: Pending,
    vint_pending: bool,
    hint_pending: bool,
    hint_counter: i16,
    line: u16,
    /// Pedido de DMA 68000 → VDP para o barramento executar.
    dma_request: Option<DmaRequest>,
    /// Último valor lido pela porta de dados (leituras de byte).
    framebuffer: Box<[u32; MAX_WIDTH * HEIGHT]>,
    /// Largura do último quadro (256 ou 320).
    frame_width: usize,
}

impl Default for Vdp {
    fn default() -> Self {
        Self::new()
    }
}

/// Pixel de um plano: índice de cor (0–63, 0 = transparente na paleta) e prioridade.
#[derive(Clone, Copy, Default)]
struct Pixel {
    color: u8,
    priority: bool,
}

impl Vdp {
    pub fn new() -> Self {
        Self {
            vram: Box::new([0; VRAM_SIZE]),
            cram: [0; CRAM_WORDS],
            vsram: [0; VSRAM_WORDS],
            regs: [0; 24],
            command_pending: false,
            code: 0,
            address: 0,
            status: STATUS_FIFO_EMPTY,
            pending_dma: Pending::None,
            vint_pending: false,
            hint_pending: false,
            hint_counter: 0,
            line: 0,
            dma_request: None,
            framebuffer: Box::new([0; MAX_WIDTH * HEIGHT]),
            frame_width: 320,
        }
    }

    /// Registradores, para depuração.
    pub fn registers(&self) -> [u8; 24] {
        self.regs
    }

    pub fn framebuffer(&self) -> &[u32] {
        self.framebuffer.as_slice()
    }

    /// Largura e altura úteis do último quadro (as linhas do framebuffer
    /// têm sempre [`MAX_WIDTH`] pixels).
    pub fn frame_size(&self) -> (usize, usize) {
        (self.frame_width, HEIGHT)
    }

    pub(crate) fn h40(&self) -> bool {
        self.regs[12] & 0x81 != 0
    }

    fn width(&self) -> usize {
        if self.h40() { 320 } else { 256 }
    }

    fn display_enabled(&self) -> bool {
        self.regs[1] & 0x40 != 0
    }

    fn dma_enabled(&self) -> bool {
        self.regs[1] & 0x10 != 0
    }

    fn auto_increment(&self) -> u16 {
        u16::from(self.regs[15])
    }

    // ---- Portas -----------------------------------------------------------

    pub(crate) fn write_control(&mut self, value: u16) {
        if self.command_pending {
            self.command_pending = false;
            self.address = (self.address & 0x3FFF) | ((value & 3) << 14);
            self.code = (self.code & 3) | (((value >> 2) & 0x3C) as u8);
            if self.code & 0x20 != 0 && self.dma_enabled() {
                self.start_dma();
            }
            return;
        }
        if value & 0xC000 == 0x8000 {
            let reg = usize::from((value >> 8) & 0x1F);
            if reg < self.regs.len() {
                self.regs[reg] = value as u8;
            }
            // Escrever registrador também zera o código (comportamento real).
            self.code = 0;
            return;
        }
        self.command_pending = true;
        self.code = (self.code & 0x3C) | ((value >> 14) as u8);
        self.address = (self.address & 0xC000) | (value & 0x3FFF);
    }

    fn dma_length(&self) -> u32 {
        let len = u32::from(self.regs[19]) | u32::from(self.regs[20]) << 8;
        if len == 0 { 0x10000 } else { len }
    }

    fn start_dma(&mut self) {
        match self.regs[23] >> 6 {
            0 | 1 => {
                let source = (u32::from(self.regs[21])
                    | u32::from(self.regs[22]) << 8
                    | u32::from(self.regs[23] & 0x7F) << 16)
                    << 1;
                self.dma_request = Some(DmaRequest {
                    source,
                    words: self.dma_length(),
                });
            }
            2 => self.pending_dma = Pending::Fill,
            _ => self.dma_copy(),
        }
    }

    /// O destino atual do comando é a VRAM (os outros contam por palavra no DMA).
    pub(crate) fn target_is_vram(&self) -> bool {
        self.code & 0x0F == 1
    }

    /// Pedido de DMA 68000 → VDP ainda não executado.
    pub(crate) fn take_dma_request(&mut self) -> Option<DmaRequest> {
        self.dma_request.take()
    }

    /// Uma palavra vinda do DMA 68000 → VDP.
    pub(crate) fn dma_write(&mut self, value: u16) {
        self.write_target(value);
        self.address = self.address.wrapping_add(self.auto_increment());
    }

    /// Fim do DMA: os registradores de origem e tamanho ficam como o hardware deixa.
    pub(crate) fn finish_dma(&mut self, source_end: u32) {
        let src = source_end >> 1;
        self.regs[21] = src as u8;
        self.regs[22] = (src >> 8) as u8;
        self.regs[23] = (self.regs[23] & 0x80) | ((src >> 16) as u8 & 0x7F);
        self.regs[19] = 0;
        self.regs[20] = 0;
        self.code &= !0x20;
    }

    fn dma_copy(&mut self) {
        let mut source = u16::from(self.regs[21]) | u16::from(self.regs[22]) << 8;
        for _ in 0..self.dma_length() {
            let byte = self.vram[usize::from(source)];
            self.vram[usize::from(self.address ^ 1)] = byte;
            source = source.wrapping_add(1);
            self.address = self.address.wrapping_add(self.auto_increment());
        }
        self.regs[21] = source as u8;
        self.regs[22] = (source >> 8) as u8;
        self.regs[19] = 0;
        self.regs[20] = 0;
        self.code &= !0x20;
    }

    fn write_target(&mut self, value: u16) {
        match self.code & 0x0F {
            1 => {
                let a = usize::from(self.address);
                // Endereço ímpar grava a palavra com os bytes trocados.
                self.vram[a] = (value >> 8) as u8;
                self.vram[a ^ 1] = value as u8;
            }
            3 => self.cram[usize::from(self.address >> 1) & 0x3F] = value & 0x0EEE,
            5 => {
                let i = usize::from(self.address >> 1) & 0x3F;
                if i < VSRAM_WORDS {
                    self.vsram[i] = value & 0x03FF;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn write_data(&mut self, value: u16) {
        self.command_pending = false;
        self.write_target(value);
        self.address = self.address.wrapping_add(self.auto_increment());
        if self.pending_dma == Pending::Fill {
            self.pending_dma = Pending::None;
            let fill = (value >> 8) as u8;
            for _ in 0..self.dma_length() {
                if self.code & 0x0F == 1 {
                    self.vram[usize::from(self.address ^ 1)] = fill;
                } else {
                    self.write_target(value);
                }
                self.address = self.address.wrapping_add(self.auto_increment());
            }
            self.regs[19] = 0;
            self.regs[20] = 0;
            self.code &= !0x20;
        }
    }

    pub(crate) fn read_data(&mut self) -> u16 {
        self.command_pending = false;
        let value = match self.code & 0x0F {
            0 => {
                let a = usize::from(self.address & !1);
                u16::from(self.vram[a]) << 8 | u16::from(self.vram[a + 1])
            }
            4 => {
                let i = usize::from(self.address >> 1) & 0x3F;
                self.vsram.get(i).copied().unwrap_or(0)
            }
            8 => self.cram[usize::from(self.address >> 1) & 0x3F],
            _ => 0,
        };
        self.address = self.address.wrapping_add(self.auto_increment());
        value
    }

    pub(crate) fn read_status(&mut self) -> u16 {
        self.command_pending = false;
        let mut status = self.status | STATUS_FIFO_EMPTY;
        if self.vint_pending {
            status |= STATUS_VINT;
        }
        if usize::from(self.line) >= HEIGHT || !self.display_enabled() {
            status |= STATUS_VBLANK;
        }
        self.status &= !(STATUS_OVERFLOW | STATUS_COLLISION);
        status
    }

    /// Contador HV: V no byte alto (NTSC 224: 0x00–0xEA, depois 0xE5–0xFF),
    /// H no baixo, aproximado pela posição na linha (`fraction` em 0..1).
    pub(crate) fn hv_counter(&self, fraction: f32) -> u16 {
        let v = if self.line <= 0xEA {
            self.line
        } else {
            self.line - 6
        } as u8;
        let h = if self.h40() {
            let pos = (fraction * 210.0) as u16;
            if pos <= 0xB6 { pos } else { pos + 0x2D }
        } else {
            let pos = (fraction * 171.0) as u16;
            if pos <= 0x93 { pos } else { pos + 0x55 }
        } as u8;
        u16::from(v) << 8 | u16::from(h)
    }

    // ---- Interrupções -----------------------------------------------------

    /// Nível pedido ao 68000: 6 (V-int) tem prioridade sobre 4 (H-int).
    pub(crate) fn interrupt_level(&self) -> u8 {
        if self.vint_pending && self.regs[1] & 0x20 != 0 {
            6
        } else if self.hint_pending && self.regs[0] & 0x10 != 0 {
            4
        } else {
            0
        }
    }

    pub(crate) fn acknowledge(&mut self, level: u8) {
        if level == 6 {
            self.vint_pending = false;
        } else if level == 4 {
            self.hint_pending = false;
        }
    }

    /// V-blank ou tela desligada: o DMA usa a taxa rápida.
    pub(crate) fn in_blank(&self) -> bool {
        usize::from(self.line) >= HEIGHT || !self.display_enabled()
    }

    /// Início da linha `line`: atualiza H-blank e o contador de H-int.
    pub(crate) fn begin_line(&mut self, line: u16) {
        self.line = line;
        self.status |= STATUS_HBLANK;
        if usize::from(line) <= HEIGHT {
            self.hint_counter -= 1;
            if self.hint_counter < 0 {
                self.hint_counter = i16::from(self.regs[10]);
                self.hint_pending = true;
            }
        } else {
            self.hint_counter = i16::from(self.regs[10]);
        }
        if line == 0 {
            self.frame_width = self.width();
        }
    }

    /// Meio da linha: fim do H-blank, desenho e (na linha 224) o V-int.
    pub(crate) fn render_line(&mut self) {
        self.status &= !STATUS_HBLANK;
        let line = usize::from(self.line);
        if line < HEIGHT {
            self.draw_line(line);
        } else if line == HEIGHT {
            self.vint_pending = true;
        }
    }

    // ---- Renderização -----------------------------------------------------

    fn color(&self, index: u8, shade: Shade) -> u32 {
        let c = self.cram[usize::from(index & 0x3F)];
        let table = match shade {
            Shade::Normal => &LEVELS,
            Shade::Shadow => &SHADOW,
            Shade::Highlight => &HIGHLIGHT,
        };
        let r = table[usize::from((c >> 1) & 7)];
        let g = table[usize::from((c >> 5) & 7)];
        let b = table[usize::from((c >> 9) & 7)];
        u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
    }

    fn draw_line(&mut self, line: usize) {
        let width = self.width();
        let background = self.regs[7] & 0x3F;
        let mut out = [0u32; MAX_WIDTH];
        if !self.display_enabled() {
            let c = self.color(background, Shade::Normal);
            out[..width].fill(c);
        } else {
            let mut plane_b = [Pixel::default(); MAX_WIDTH];
            let mut plane_a = [Pixel::default(); MAX_WIDTH];
            let mut sprites = [Pixel::default(); MAX_WIDTH];
            self.draw_plane(line, false, &mut plane_b[..width]);
            self.draw_plane(line, true, &mut plane_a[..width]);
            self.draw_window(line, &mut plane_a[..width]);
            self.draw_sprites(line, &mut sprites[..width]);
            let shadow_highlight = self.regs[12] & 0x08 != 0;
            for x in 0..width {
                out[x] = self.compose(
                    background,
                    plane_b[x],
                    plane_a[x],
                    sprites[x],
                    shadow_highlight,
                );
            }
            if self.regs[0] & 0x20 != 0 {
                let c = self.color(background, Shade::Normal);
                out[..8].fill(c);
            }
        }
        self.framebuffer[line * MAX_WIDTH..line * MAX_WIDTH + width].copy_from_slice(&out[..width]);
    }

    /// Ordem de prioridade: fundo, B, A, sprite (baixa), B, A, sprite (alta).
    fn compose(&self, background: u8, b: Pixel, a: Pixel, s: Pixel, sh: bool) -> u32 {
        let opaque = |p: Pixel| p.color & 0x0F != 0;
        let mut layers = [(s, 2u8), (a, 1), (b, 0)];
        // Prioridade alta primeiro, mantendo sprite > A > B dentro do grupo.
        layers.sort_by_key(|(p, order)| (!p.priority, 2 - order));
        let top = layers
            .iter()
            .find(|(p, _)| opaque(*p))
            .map(|(p, order)| (*p, *order));

        if !sh {
            return match top {
                Some((p, _)) => self.color(p.color, Shade::Normal),
                None => self.color(background, Shade::Normal),
            };
        }
        // Shadow/highlight: sem plano de prioridade alta, tudo começa na sombra.
        let mut shade = if a.priority || b.priority {
            Shade::Normal
        } else {
            Shade::Shadow
        };
        // Sprites da paleta 3 com cor 14/15 viram operadores (invisíveis).
        if opaque(s) && s.color & 0x30 == 0x30 && (s.color & 0x0F) >= 14 {
            let under = layers
                .iter()
                .filter(|(_, order)| *order != 2)
                .find(|(p, _)| opaque(*p))
                .map(|(p, _)| p.color)
                .unwrap_or(background);
            shade = if s.color & 0x0F == 14 {
                match shade {
                    Shade::Shadow => Shade::Normal,
                    _ => Shade::Highlight,
                }
            } else {
                Shade::Shadow
            };
            return self.color(under, shade);
        }
        match top {
            Some((p, 2)) if p.priority => self.color(p.color, Shade::Normal),
            Some((p, _)) => self.color(p.color, shade),
            None => self.color(background, shade),
        }
    }

    fn plane_size(&self) -> (usize, usize) {
        let cells = |bits: u8| match bits & 3 {
            0 => 32,
            1 => 64,
            _ => 128,
        };
        let w = cells(self.regs[16]);
        let h = cells(self.regs[16] >> 4);
        // Combinações inválidas no hardware ficam limitadas a 64×64 células.
        if w * h > 64 * 64 {
            (w.min(64), h.min(64))
        } else {
            (w, h)
        }
    }

    fn hscroll(&self, line: usize, plane_a: bool) -> usize {
        let base = usize::from(self.regs[13] & 0x3F) << 10;
        let row = match self.regs[11] & 3 {
            0 => 0,
            1 => line & 7,
            2 => line & !7,
            _ => line,
        };
        let addr = (base + row * 4 + if plane_a { 0 } else { 2 }) & 0xFFFF;
        let value = u16::from(self.vram[addr]) << 8 | u16::from(self.vram[addr | 1]);
        usize::from(value & 0x3FF)
    }

    /// Rolagem vertical; `column` é a coluna de 16 pixels (modo por 2 células).
    fn vscroll(&self, column: usize, plane_a: bool) -> usize {
        let index = if self.regs[11] & 0x04 != 0 {
            column * 2
        } else {
            0
        } + usize::from(!plane_a);
        usize::from(self.vsram.get(index).copied().unwrap_or(0) & 0x3FF)
    }

    fn name_entry(&self, addr: usize) -> u16 {
        let a = addr & 0xFFFE;
        u16::from(self.vram[a]) << 8 | u16::from(self.vram[a + 1])
    }

    /// Cor (0–15) de um pixel de tile 4bpp.
    fn tile_pixel(&self, tile: usize, row: usize, col: usize) -> u8 {
        let addr = (tile * 32 + row * 4 + col / 2) & 0xFFFF;
        let byte = self.vram[addr];
        if col & 1 == 0 { byte >> 4 } else { byte & 0x0F }
    }

    fn entry_pixel(&self, entry: u16, row: usize, col: usize) -> Pixel {
        let tile = usize::from(entry & 0x07FF);
        let row = if entry & 0x1000 != 0 { 7 - row } else { row };
        let col = if entry & 0x0800 != 0 { 7 - col } else { col };
        let color = self.tile_pixel(tile, row, col);
        Pixel {
            color: if color == 0 {
                0
            } else {
                ((entry >> 9) & 0x30) as u8 | color
            },
            priority: entry & 0x8000 != 0,
        }
    }

    fn draw_plane(&self, line: usize, plane_a: bool, out: &mut [Pixel]) {
        let base = if plane_a {
            usize::from(self.regs[2] & 0x38) << 10
        } else {
            usize::from(self.regs[4] & 0x07) << 13
        };
        let (w_cells, h_cells) = self.plane_size();
        let (w_px, h_px) = (w_cells * 8, h_cells * 8);
        let hs = self.hscroll(line, plane_a);
        for (x, pixel) in out.iter_mut().enumerate() {
            let vs = self.vscroll(x / 16, plane_a);
            let px = (x + w_px * 2 - hs % w_px) % w_px;
            let py = (line + vs) % h_px;
            let entry = self.name_entry(base + ((py / 8) * w_cells + px / 8) * 2);
            *pixel = self.entry_pixel(entry, py % 8, px % 8);
        }
    }

    /// A janela substitui o plano A na região definida por R17/R18.
    fn draw_window(&self, line: usize, plane_a: &mut [Pixel]) {
        let width = plane_a.len();
        let v_pos = usize::from(self.regs[18] & 0x1F) * 8;
        let in_rows = if self.regs[18] & 0x80 != 0 {
            line >= v_pos
        } else {
            line < v_pos
        };
        let h_pos = usize::from(self.regs[17] & 0x1F) * 16;
        let (from, to) = if in_rows {
            (0, width)
        } else if self.regs[17] & 0x80 != 0 {
            (h_pos.min(width), width)
        } else {
            (0, h_pos.min(width))
        };
        if from >= to {
            return;
        }
        let (base, cells) = if self.h40() {
            (usize::from(self.regs[3] & 0x3C) << 10, 64)
        } else {
            (usize::from(self.regs[3] & 0x3E) << 10, 32)
        };
        for (x, pixel) in plane_a.iter_mut().enumerate().take(to).skip(from) {
            let entry = self.name_entry(base + ((line / 8) * cells + x / 8) * 2);
            *pixel = self.entry_pixel(entry, line % 8, x % 8);
        }
    }

    fn draw_sprites(&mut self, line: usize, out: &mut [Pixel]) {
        let width = out.len();
        let (table, max_sprites, max_per_line) = if self.h40() {
            (usize::from(self.regs[5] & 0x7E) << 9, 80, 20)
        } else {
            (usize::from(self.regs[5] & 0x7F) << 9, 64, 16)
        };
        let max_pixels = width;
        let y = line + 128;
        let mut index = 0usize;
        let mut on_line = 0;
        let mut pixels = 0;
        let mut drawn = [false; MAX_WIDTH];
        let mut masked = false;
        let mut seen_nonzero_x = false;
        for _ in 0..max_sprites {
            let entry = (table + index * 8) & 0xFFFF;
            let word = |o: usize| self.name_entry(entry + o);
            let sy = usize::from(word(0) & 0x3FF);
            let size = word(2);
            let link = usize::from(size & 0x7F);
            let h_cells = usize::from((size >> 8) & 3) + 1;
            let w_cells = usize::from((size >> 10) & 3) + 1;
            if y >= sy && y < sy + h_cells * 8 {
                on_line += 1;
                if on_line > max_per_line {
                    self.status |= STATUS_OVERFLOW;
                    break;
                }
                let attr = word(4);
                let sx = usize::from(word(6) & 0x1FF);
                if sx == 0 {
                    if seen_nonzero_x {
                        masked = true;
                    }
                } else {
                    seen_nonzero_x = true;
                }
                let row = y - sy;
                let row = if attr & 0x1000 != 0 {
                    h_cells * 8 - 1 - row
                } else {
                    row
                };
                for cx in 0..w_cells * 8 {
                    if pixels >= max_pixels {
                        break;
                    }
                    pixels += 1;
                    if masked {
                        continue;
                    }
                    let Some(screen_x) = (sx + cx).checked_sub(128) else {
                        continue;
                    };
                    if screen_x >= width {
                        continue;
                    }
                    let col = if attr & 0x0800 != 0 {
                        w_cells * 8 - 1 - cx
                    } else {
                        cx
                    };
                    let tile = usize::from(attr & 0x07FF) + (col / 8) * h_cells + row / 8;
                    let color = self.tile_pixel(tile, row % 8, col % 8);
                    if color == 0 {
                        continue;
                    }
                    if drawn[screen_x] {
                        self.status |= STATUS_COLLISION;
                        continue;
                    }
                    drawn[screen_x] = true;
                    out[screen_x] = Pixel {
                        color: ((attr >> 9) & 0x30) as u8 | color,
                        priority: attr & 0x8000 != 0,
                    };
                }
            }
            if link == 0 || link >= max_sprites {
                break;
            }
            index = link;
        }
    }

    // ---- Estado salvo -----------------------------------------------------

    pub(crate) fn save_state(&self, w: &mut Writer) {
        w.slice(self.vram.as_slice());
        for v in self.cram.iter().chain(self.vsram.iter()) {
            w.u16(*v);
        }
        w.slice(&self.regs);
        w.bool(self.command_pending);
        w.u8(self.code);
        w.u16(self.address);
        w.u16(self.status);
        w.bool(self.pending_dma == Pending::Fill);
        w.bool(self.vint_pending);
        w.bool(self.hint_pending);
        w.u16(self.hint_counter as u16);
        w.u16(self.line);
    }

    pub(crate) fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        r.fill(self.vram.as_mut_slice())?;
        for v in self.cram.iter_mut().chain(self.vsram.iter_mut()) {
            *v = r.u16()?;
        }
        r.fill(&mut self.regs)?;
        self.command_pending = r.bool()?;
        self.code = r.u8()? & 0x3F;
        self.address = r.u16()?;
        self.status = r.u16()?;
        self.pending_dma = if r.bool()? {
            Pending::Fill
        } else {
            Pending::None
        };
        self.vint_pending = r.bool()?;
        self.hint_pending = r.bool()?;
        self.hint_counter = r.u16()? as i16;
        self.line = r.u16()?;
        if self.line >= LINES_PER_FRAME {
            return Err(StateError::Invalid("linha do VDP"));
        }
        self.dma_request = None;
        self.frame_width = self.width();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shade {
    Normal,
    Shadow,
    Highlight,
}

#[cfg(test)]
mod tests;
