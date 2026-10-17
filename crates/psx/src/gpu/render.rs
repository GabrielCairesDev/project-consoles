//! Comandos de desenho do GP0 e o rasterizador por software, na resolução
//! nativa da VRAM (1024×512, 16 bits por pixel).
//!
//! Regras do hardware seguidas:
//! - **Triângulos:** só os pixels com o centro dentro (regra topo-esquerda:
//!   as bordas da direita e de baixo ficam de fora). Quadriláteros são os
//!   triângulos (0, 1, 2) e (1, 2, 3). Polígonos com mais de 1023 pixels
//!   de largura ou 511 de altura são descartados.
//! - **Atributos:** cor e coordenadas de textura andam por gradientes em
//!   ponto fixo, como o chip.
//! - **Texturas:** 4, 8 e 15 bits, CLUT, janela de textura, texel 0 é
//!   transparente; a cor modula o texel (128 = neutro), salvo nos
//!   comandos "raw".
//! - **Cache de CLUT:** texturas de 4 e 8 bits leem a paleta de um cache
//!   (16 ou 256 cores) carregado ao desenhar quando a CLUT muda ou faltam
//!   cores (4 bits depois de 8 aproveita as 256); escrever na VRAM não o atualiza, só o GP0 01h o
//!   invalida.
//! - **Semitransparência:** 4 modos; em polígonos texturizados só onde o
//!   texel tem o bit 15.
//! - **Dithering:** matriz 4×4 em polígonos com gouraud ou textura
//!   modulada e em todas as linhas; retângulos nunca.
//! - **Máscara:** bit 15 forçado ao desenhar e/ou pixels com bit 15
//!   protegidos.
//!
//! O desenho é instantâneo (sem o tempo de rasterização do chip).
//!
//! **Resolução interna maior** (melhoria de imagem, opcional): a VRAM 1×
//! continua sendo a fonte de verdade — tudo é desenhado nela exatamente
//! como acima, e é dela que vêm as leituras da CPU, as cópias, as texturas
//! e as CLUTs. Com escala `s` > 1, cada polígono, retângulo e linha também
//! é desenhado numa cópia `s`× maior ([`Upscaled`]), que serve só para a
//! imagem mostrada. Nela os polígonos têm bordas e texturas com `s`× mais
//! detalhe (vértices × `s`, gradientes por pixel ampliado); retângulos e
//! linhas ocupam os mesmos blocos `s`×`s` do 1× (sprites ficam nítidos);
//! transferências, preenchimentos e cópias repetem na cópia o que a VRAM 1×
//! recebeu, bloco a bloco.

pub(crate) const VRAM_WIDTH: usize = 1024;
pub(crate) const VRAM_HEIGHT: usize = 512;

const DITHER: [[i32; 4]; 4] = [
    [-4, 0, -3, 1],
    [2, -2, 3, -1],
    [-3, 1, -4, 0],
    [3, -1, 2, -2],
];

/// Bits de fração dos gradientes (como no chip).
const FRAC: u32 = 12;

/// Maior lista de linhas aceita antes do terminador.
const MAX_POLYLINE_WORDS: usize = 4096;

/// Maior escala da resolução interna.
pub(crate) const MAX_SCALE: usize = 4;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Vertex {
    x: i32,
    y: i32,
    color: [i32; 3],
    u: i32,
    v: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Depth {
    Bits4,
    Bits8,
    Bits15,
}

/// Textura de um primitivo: página, profundidade e CLUT.
#[derive(Clone, Copy, Debug)]
struct Texture {
    base_x: i32,
    base_y: i32,
    depth: Depth,
    clut_x: i32,
    clut_y: i32,
    /// Usa o texel como está, sem modular pela cor.
    raw: bool,
}

#[derive(Clone, Copy)]
struct Primitive {
    texture: Option<Texture>,
    semi_transparent: bool,
    /// Modo de semitransparência (0–3).
    blend: u32,
    dither: bool,
}

#[derive(Clone)]
enum Transfer {
    /// Esperando comandos.
    Idle,
    /// CPU → VRAM: pixels chegando pelo GP0.
    ToVram {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        index: u32,
    },
}

#[derive(Clone)]
pub(crate) struct Renderer {
    pub(crate) vram: Box<[u16]>,
    /// Bits 0–10 do E1 (página de textura, semitransparência, dithering…).
    draw_mode: u32,
    flip_x: bool,
    flip_y: bool,
    texture_window: u32,
    area_left: i32,
    area_top: i32,
    area_right: i32,
    area_bottom: i32,
    offset_x: i32,
    offset_y: i32,
    mask_set: bool,
    mask_check: bool,
    /// Palavras do comando em montagem.
    words: Vec<u32>,
    expected: usize,
    polyline: bool,
    transfer: Transfer,
    /// VRAM → CPU em andamento: (x, y, w, h, índice).
    read_back: Option<(u32, u32, u32, u32, u32)>,
    pub(crate) read_latch: u32,
    /// GP0 1Fh pediu interrupção.
    pub(crate) irq_requested: bool,
    clut_cache: Box<[u16; 256]>,
    /// CLUT (15 bits) no cache e quantas cores dela foram carregadas.
    clut_tag: Option<(u32, usize)>,
    /// Cópia ampliada da VRAM (só com escala > 1); derivada da VRAM 1× e
    /// do desenho, não faz parte do estado do console.
    upscaled: Option<Upscaled>,
}

/// VRAM em resolução interna `scale`× maior, só para a imagem.
#[derive(Clone)]
pub(crate) struct Upscaled {
    scale: usize,
    pixels: Box<[u16]>,
}

/// `v` dentro de `0..n`, sabendo que `v < 2n` (evita a divisão do `%`).
fn wrap(v: usize, n: usize) -> usize {
    if v >= n { v - n } else { v }
}

impl Upscaled {
    /// Cópia da VRAM 1× com cada pixel repetido em `scale`×`scale`.
    fn from_native(vram: &[u16], scale: usize) -> Self {
        let width = VRAM_WIDTH * scale;
        let mut pixels = vec![0; width * VRAM_HEIGHT * scale].into_boxed_slice();
        for (y, row) in pixels.chunks_exact_mut(width).enumerate() {
            let native = &vram[(y / scale) * VRAM_WIDTH..][..VRAM_WIDTH];
            for (out, &p) in row.chunks_exact_mut(scale).zip(native) {
                out.fill(p);
            }
        }
        Self { scale, pixels }
    }

    pub(crate) fn scale(&self) -> usize {
        self.scale
    }

    pub(crate) fn width(&self) -> usize {
        VRAM_WIDTH * self.scale
    }

    pub(crate) fn height(&self) -> usize {
        VRAM_HEIGHT * self.scale
    }

    /// Linha `y` (`0..height()`) da VRAM ampliada.
    pub(crate) fn row(&self, y: usize) -> &[u16] {
        let width = self.width();
        &self.pixels[y * width..][..width]
    }

    /// Pixel ampliado; `None` fora da VRAM (o desenho já vem recortado
    /// pela área, então não acontece).
    fn pixel_mut(&mut self, x: i32, y: i32) -> Option<&mut u16> {
        let (x, y) = (usize::try_from(x).ok()?, usize::try_from(y).ok()?);
        if x >= self.width() {
            return None;
        }
        let i = y * self.width() + x;
        self.pixels.get_mut(i)
    }

    /// Bloco do pixel 1× (`x`, `y`), já dentro da VRAM.
    fn block_start(&self, x: usize, y: usize) -> usize {
        y * self.scale * self.width() + x * self.scale
    }

    /// Repete no bloco o valor escrito no pixel 1× (`x`, `y`).
    fn write_block(&mut self, x: usize, y: usize, value: u16) {
        let (start, width, scale) = (self.block_start(x, y), self.width(), self.scale);
        for row in 0..scale {
            self.pixels[start + row * width..][..scale].fill(value);
        }
    }

    /// Copia o bloco do pixel 1× de origem para o de destino, como a VRAM
    /// 1× fez com o pixel, preservando o detalhe ampliado.
    fn copy_block(&mut self, src: (usize, usize), dst: (usize, usize), mask: u16) {
        let (from, to) = (
            self.block_start(src.0, src.1),
            self.block_start(dst.0, dst.1),
        );
        let (width, scale) = (self.width(), self.scale);
        for row in 0..scale {
            for i in 0..scale {
                let value = self.pixels[from + row * width + i];
                self.pixels[to + row * width + i] = value | mask;
            }
        }
    }

    /// GP0 02h no retângulo 1× (`x`, `y`, `w`, `h`), com as voltas da VRAM.
    fn fill(&mut self, (x, y, w, h): (usize, usize, usize, usize), value: u16) {
        let (width, height, scale) = (self.width(), self.height(), self.scale);
        for dy in 0..h * scale {
            let row = wrap(y * scale + dy, height) * width;
            for dx in 0..w * scale {
                self.pixels[row + wrap(x * scale + dx, width)] = value;
            }
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            vram: vec![0; VRAM_WIDTH * VRAM_HEIGHT].into_boxed_slice(),
            draw_mode: 0,
            flip_x: false,
            flip_y: false,
            texture_window: 0,
            area_left: 0,
            area_top: 0,
            area_right: 0,
            area_bottom: 0,
            offset_x: 0,
            offset_y: 0,
            mask_set: false,
            mask_check: false,
            words: Vec::with_capacity(16),
            expected: 0,
            polyline: false,
            transfer: Transfer::Idle,
            read_back: None,
            read_latch: 0,
            irq_requested: false,
            clut_cache: Box::new([0; 256]),
            clut_tag: None,
            upscaled: None,
        }
    }
}

fn sign_extend_11(v: u32) -> i32 {
    ((v << 21) as i32) >> 21
}

fn rgb_of(word: u32) -> [i32; 3] {
    [
        (word & 0xFF) as i32,
        ((word >> 8) & 0xFF) as i32,
        ((word >> 16) & 0xFF) as i32,
    ]
}

/// Converte uma cor de 24 bits em 15 (sem máscara).
fn rgb15(word: u32) -> u16 {
    let c = rgb_of(word);
    ((c[0] >> 3) | ((c[1] >> 3) << 5) | ((c[2] >> 3) << 10)) as u16
}

/// Comprimento (em palavras) de um comando pelo primeiro byte; `None` para
/// lista de linhas (vai até o terminador).
fn command_length(command: u32) -> Option<usize> {
    let gouraud = command & 0x10 != 0;
    Some(match command {
        0x02 => 3,
        0x20..=0x3F => {
            let vertices = if command & 0x08 != 0 { 4 } else { 3 };
            let textured = usize::from(command & 0x04 != 0);
            1 + vertices * (1 + textured) + if gouraud { vertices - 1 } else { 0 }
        }
        0x40..=0x5F if command & 0x08 != 0 => return None,
        0x40..=0x5F => 3 + usize::from(gouraud),
        0x60..=0x7F => {
            let textured = usize::from(command & 0x04 != 0);
            let variable = usize::from((command >> 3) & 3 == 0);
            2 + textured + variable
        }
        0x80..=0x9F => 4,
        0xA0..=0xDF => 3,
        _ => 1,
    })
}

impl Renderer {
    pub(crate) fn reset(&mut self) {
        // O reset da GPU não apaga a VRAM (nem a cópia ampliada).
        let vram = std::mem::take(&mut self.vram);
        let upscaled = self.upscaled.take();
        *self = Self {
            vram,
            upscaled,
            ..Self::default()
        };
    }

    /// Escala da resolução interna (1 = nativa, até [`MAX_SCALE`]). Refaz a
    /// cópia ampliada a partir da VRAM 1× — também serve para ressincronizá-la
    /// depois de a VRAM mudar por fora do desenho (estado carregado).
    pub(crate) fn set_scale(&mut self, scale: usize) {
        let scale = scale.clamp(1, MAX_SCALE);
        self.upscaled = (scale > 1).then(|| Upscaled::from_native(&self.vram, scale));
    }

    pub(crate) fn scale(&self) -> usize {
        self.upscaled.as_ref().map_or(1, Upscaled::scale)
    }

    pub(crate) fn upscaled(&self) -> Option<&Upscaled> {
        self.upscaled.as_ref()
    }

    /// Escala da cópia ampliada, se houver, para desenhar nela também.
    fn upscaled_scale(&self) -> Option<i32> {
        self.upscaled.as_ref().map(|up| up.scale as i32)
    }

    pub(crate) fn reset_command_buffer(&mut self) {
        self.words.clear();
        self.expected = 0;
        self.polyline = false;
        self.transfer = Transfer::Idle;
    }

    /// Bits do GPUSTAT que vêm do estado de desenho (0–12 e 15).
    pub(crate) fn status_bits(&self) -> u32 {
        (self.draw_mode & 0x7FF)
            | (u32::from(self.mask_set) << 11)
            | (u32::from(self.mask_check) << 12)
            | (((self.draw_mode >> 11) & 1) << 15)
    }

    /// Há pixels esperando a leitura pela CPU (GPUSTAT bit 27).
    pub(crate) fn has_read_data(&self) -> bool {
        self.read_back.is_some()
    }

    /// Resposta do GP1 10h.
    pub(crate) fn info(&mut self, what: u32) {
        let value = match what & 7 {
            2 => self.texture_window,
            3 => (self.area_left as u32) | ((self.area_top as u32) << 10),
            4 => (self.area_right as u32) | ((self.area_bottom as u32) << 10),
            5 => ((self.offset_x as u32) & 0x7FF) | (((self.offset_y as u32) & 0x7FF) << 11),
            7 => 2,
            _ => return,
        };
        self.read_latch = value;
    }

    pub(crate) fn pixel(&self, x: usize, y: usize) -> u16 {
        self.vram[(y % VRAM_HEIGHT) * VRAM_WIDTH + (x % VRAM_WIDTH)]
    }

    fn set_pixel(&mut self, x: usize, y: usize, value: u16) {
        self.vram[(y % VRAM_HEIGHT) * VRAM_WIDTH + (x % VRAM_WIDTH)] = value;
    }

    /// Escreve respeitando a máscara (para transferências e cópias);
    /// devolve se escreveu.
    fn write_masked(&mut self, x: usize, y: usize, value: u16) -> bool {
        if self.mask_check && self.pixel(x, y) & 0x8000 != 0 {
            return false;
        }
        let value = value | if self.mask_set { 0x8000 } else { 0 };
        self.set_pixel(x, y, value);
        true
    }

    /// GPUREAD: pixels da transferência VRAM → CPU, dois por palavra.
    pub(crate) fn read(&mut self) -> u32 {
        if let Some((x, y, w, h, index)) = self.read_back {
            let mut word = 0;
            for half in 0..2 {
                let i = index + half;
                let px = (x + i % w) as usize;
                let py = (y + i / w) as usize;
                word |= u32::from(self.pixel(px, py)) << (16 * half);
            }
            let next = index + 2;
            self.read_back = (next < w * h).then_some((x, y, w, h, next));
            self.read_latch = word;
        }
        self.read_latch
    }

    pub(crate) fn gp0(&mut self, word: u32) {
        if let Transfer::ToVram { x, y, w, h, index } = self.transfer {
            let mut i = index;
            for half in 0..2 {
                if i < w * h {
                    let px = (x + i % w) as usize;
                    let py = (y + i / w) as usize;
                    let value = (word >> (16 * half)) as u16;
                    if self.write_masked(px, py, value)
                        && let Some(up) = &mut self.upscaled
                    {
                        let value = value | if self.mask_set { 0x8000 } else { 0 };
                        up.write_block(px % VRAM_WIDTH, py % VRAM_HEIGHT, value);
                    }
                    i += 1;
                }
            }
            self.transfer = if i < w * h {
                Transfer::ToVram {
                    x,
                    y,
                    w,
                    h,
                    index: i,
                }
            } else {
                Transfer::Idle
            };
            return;
        }

        if self.words.is_empty() {
            match command_length(word >> 24) {
                Some(n) => {
                    self.expected = n;
                    self.polyline = false;
                }
                None => {
                    self.expected = usize::MAX;
                    self.polyline = true;
                }
            }
        }
        self.words.push(word);
        let complete = if self.polyline {
            let terminator = self.words.len() > 3 && word & 0xF000_F000 == 0x5000_5000;
            terminator || self.words.len() >= MAX_POLYLINE_WORDS
        } else {
            self.words.len() >= self.expected
        };
        if complete {
            let words = std::mem::take(&mut self.words);
            self.execute(&words);
            self.words = words;
            self.words.clear();
        }
    }

    fn execute(&mut self, words: &[u32]) {
        let command = words[0] >> 24;
        match command {
            0x01 => self.clut_tag = None,
            0x02 => self.fill(words),
            0x1F => self.irq_requested = true,
            0x20..=0x3F => self.polygon(words),
            0x40..=0x5F => self.lines(words),
            0x60..=0x7F => self.rectangle(words),
            0x80..=0x9F => self.copy(words),
            0xA0..=0xBF => {
                let (x, y, w, h) = transfer_rect(words);
                self.transfer = Transfer::ToVram {
                    x,
                    y,
                    w,
                    h,
                    index: 0,
                };
            }
            0xC0..=0xDF => {
                let (x, y, w, h) = transfer_rect(words);
                self.read_back = Some((x, y, w, h, 0));
            }
            0xE1 => {
                self.draw_mode = words[0] & 0xFFF;
                self.flip_x = words[0] & (1 << 12) != 0;
                self.flip_y = words[0] & (1 << 13) != 0;
            }
            0xE2 => self.texture_window = words[0] & 0xF_FFFF,
            0xE3 => {
                self.area_left = (words[0] & 0x3FF) as i32;
                self.area_top = ((words[0] >> 10) & 0x1FF) as i32;
            }
            0xE4 => {
                self.area_right = (words[0] & 0x3FF) as i32;
                self.area_bottom = ((words[0] >> 10) & 0x1FF) as i32;
            }
            0xE5 => {
                self.offset_x = sign_extend_11(words[0]);
                self.offset_y = sign_extend_11(words[0] >> 11);
            }
            0xE6 => {
                self.mask_set = words[0] & 1 != 0;
                self.mask_check = words[0] & 2 != 0;
            }
            _ => {}
        }
    }

    fn vertex(&self, word: u32) -> (i32, i32) {
        (
            sign_extend_11(word) + self.offset_x,
            sign_extend_11(word >> 16) + self.offset_y,
        )
    }

    fn texture_from(&self, clut: u32, page: u32, raw: bool) -> Texture {
        Texture {
            base_x: ((page & 0xF) * 64) as i32,
            base_y: (((page >> 4) & 1) * 256) as i32,
            depth: match (page >> 7) & 3 {
                0 => Depth::Bits4,
                1 => Depth::Bits8,
                _ => Depth::Bits15,
            },
            clut_x: ((clut & 0x3F) * 16) as i32,
            clut_y: ((clut >> 6) & 0x1FF) as i32,
            raw,
        }
    }

    /// Recarrega o cache de CLUT se a textura usa outra paleta ou precisa
    /// de mais cores do que as carregadas.
    fn update_clut_cache(&mut self, t: &Texture, clut: u32) {
        let count = match t.depth {
            Depth::Bits4 => 16,
            Depth::Bits8 => 256,
            Depth::Bits15 => return,
        };
        // O bit 15 da CLUT é ignorado pelo chip.
        let tag = clut & 0x7FFF;
        if matches!(self.clut_tag, Some((cached, loaded)) if cached == tag && loaded >= count) {
            return;
        }
        for i in 0..count {
            // A paleta dá a volta na mesma linha da VRAM.
            self.clut_cache[i] =
                self.pixel((t.clut_x as usize + i) % VRAM_WIDTH, t.clut_y as usize);
        }
        self.clut_tag = Some((tag, count));
    }

    fn dither_enabled(&self) -> bool {
        self.draw_mode & (1 << 9) != 0
    }

    fn polygon(&mut self, words: &[u32]) {
        let command = words[0] >> 24;
        let gouraud = command & 0x10 != 0;
        let quad = command & 0x08 != 0;
        let textured = command & 0x04 != 0;
        let raw = command & 0x01 != 0;
        let count = if quad { 4 } else { 3 };

        let mut vertices = [Vertex::default(); 4];
        let mut at = 0;
        let mut clut = 0;
        let mut page = 0;
        for (n, vertex) in vertices.iter_mut().take(count).enumerate() {
            let color = if n == 0 || gouraud {
                let c = words[at];
                at += 1;
                c
            } else {
                words[0]
            };
            let (x, y) = self.vertex(words[at]);
            at += 1;
            *vertex = Vertex {
                x,
                y,
                color: rgb_of(color),
                u: 0,
                v: 0,
            };
            if textured {
                let uv = words[at];
                at += 1;
                vertex.u = (uv & 0xFF) as i32;
                vertex.v = ((uv >> 8) & 0xFF) as i32;
                match n {
                    0 => clut = uv >> 16,
                    1 => page = uv >> 16,
                    _ => {}
                }
            }
        }

        let texture = if textured {
            // A página do polígono vale para o GPUSTAT também.
            self.draw_mode = (self.draw_mode & !0x9FF) | (page & 0x9FF);
            let texture = self.texture_from(clut, page, raw);
            self.update_clut_cache(&texture, clut);
            Some(texture)
        } else {
            None
        };
        if raw && textured {
            // Sem modulação: cor neutra.
            for v in &mut vertices {
                v.color = [128; 3];
            }
        }
        let primitive = Primitive {
            texture,
            semi_transparent: command & 0x02 != 0,
            blend: (self.draw_mode >> 5) & 3,
            dither: self.dither_enabled() && (gouraud || (textured && !raw)),
        };
        self.triangle(&primitive, vertices[0], vertices[1], vertices[2], gouraud);
        if quad {
            self.triangle(&primitive, vertices[1], vertices[2], vertices[3], gouraud);
        }
    }

    /// Desenha o triângulo na cópia ampliada (antes, para ler as texturas
    /// como estavam antes do primitivo) e na VRAM 1×.
    fn triangle(&mut self, p: &Primitive, a: Vertex, b: Vertex, c: Vertex, gouraud: bool) {
        if let Some(s) = self.upscaled_scale() {
            self.triangle_at(s, p, a, b, c, gouraud);
        }
        self.triangle_at(1, p, a, b, c, gouraud);
    }

    /// Rasteriza o triângulo com os vértices e a área de desenho
    /// multiplicados por `s` (1 = VRAM 1×, senão a cópia ampliada).
    fn triangle_at(
        &mut self,
        s: i32,
        p: &Primitive,
        a: Vertex,
        b: Vertex,
        c: Vertex,
        gouraud: bool,
    ) {
        let xs = [a.x, b.x, c.x];
        let ys = [a.y, b.y, c.y];
        let (min_x, max_x) = (xs.iter().min().copied(), xs.iter().max().copied());
        let (min_y, max_y) = (ys.iter().min().copied(), ys.iter().max().copied());
        let (Some(min_x), Some(max_x), Some(min_y), Some(max_y)) = (min_x, max_x, min_y, max_y)
        else {
            return;
        };
        if max_x - min_x >= 1024 || max_y - min_y >= 512 {
            return;
        }
        let scaled = |v: Vertex| Vertex {
            x: v.x * s,
            y: v.y * s,
            ..v
        };
        let (a, b, c) = (scaled(a), scaled(b), scaled(c));
        let (min_x, max_x, min_y, max_y) = (min_x * s, max_x * s, min_y * s, max_y * s);
        let area = i64::from(b.x - a.x) * i64::from(c.y - a.y)
            - i64::from(b.y - a.y) * i64::from(c.x - a.x);
        if area == 0 {
            return;
        }
        // Ordem anti-horária (área positiva) para os testes de borda.
        let (b, c) = if area < 0 { (c, b) } else { (b, c) };

        // Gradientes como no chip: com os vértices ordenados por y (A, B,
        // C), divisão truncada e FRAC bits de fração.
        let mut sorted = [a, b, c];
        if sorted[2].y < sorted[1].y {
            sorted.swap(1, 2);
        }
        if sorted[1].y < sorted[0].y {
            sorted.swap(0, 1);
        }
        if sorted[2].y < sorted[1].y {
            sorted.swap(1, 2);
        }
        let [sa, sb, sc] = sorted;
        let denom = i128::from(sb.x - sa.x) * i128::from(sc.y - sb.y)
            - i128::from(sc.x - sb.x) * i128::from(sb.y - sa.y);
        let gradient = |f: fn(&Vertex) -> i32| -> (i64, i64) {
            let (fa, fb, fc) = (f(&sa), f(&sb), f(&sc));
            let dx = i128::from(fb - fa) * i128::from(sc.y - sb.y)
                - i128::from(fc - fb) * i128::from(sb.y - sa.y);
            let dy = i128::from(sb.x - sa.x) * i128::from(fc - fb)
                - i128::from(sc.x - sb.x) * i128::from(fb - fa);
            (((dx << FRAC) / denom) as i64, ((dy << FRAC) / denom) as i64)
        };
        let shade = if gouraud {
            [
                gradient(|v| v.color[0]),
                gradient(|v| v.color[1]),
                gradient(|v| v.color[2]),
            ]
        } else {
            [(0, 0); 3]
        };
        let uv = [gradient(|v| v.u), gradient(|v| v.v)];
        // Os atributos partem do vértice mais à esquerda.
        let core = if sb.x <= sa.x {
            if sc.x <= sb.x { sc } else { sb }
        } else if sc.x < sa.x {
            sc
        } else {
            sa
        };

        // Bordas: a→b, b→c, c→a. Regra topo-esquerda: borda de cima (y
        // igual, indo para a esquerda) ou da esquerda (subindo) inclui o
        // pixel sobre ela; as outras não.
        let edges = [(a, b), (b, c), (c, a)];
        let edge_value = |(p0, p1): (Vertex, Vertex), x: i32, y: i32| -> i64 {
            i64::from(p1.x - p0.x) * i64::from(y - p0.y)
                - i64::from(p1.y - p0.y) * i64::from(x - p0.x)
        };
        let bias = edges.map(|(p0, p1)| {
            let top = p0.y == p1.y && p1.x > p0.x;
            let left = p1.y < p0.y;
            if top || left { 0 } else { -1 }
        });

        // Cada pixel 1× da área vira um bloco s×s.
        let left = min_x.max(self.area_left * s);
        let right = max_x.min(self.area_right * s + s - 1);
        let top = min_y.max(self.area_top * s);
        let bottom = max_y.min(self.area_bottom * s + s - 1);
        if left > right || top > bottom {
            return;
        }
        let step_x = edges.map(|(p0, p1)| -i64::from(p1.y - p0.y));

        // Valor de um atributo no ponto (x, y), arredondado ao meio.
        let half = 1i64 << (FRAC - 1);
        let at = |base: i32, (dx, dy): (i64, i64), x: i32, y: i32| -> i64 {
            (i64::from(base) << FRAC)
                + dx * i64::from(x - core.x)
                + dy * i64::from(y - core.y)
                + half
        };

        for y in top..=bottom {
            let mut w = [0, 1, 2].map(|i| edge_value(edges[i], left, y) + bias[i]);
            let mut color = [0, 1, 2].map(|i| at(core.color[i], shade[i], left, y));
            let mut u = at(core.u, uv[0], left, y);
            let mut v = at(core.v, uv[1], left, y);
            for x in left..=right {
                if w[0] >= 0 && w[1] >= 0 && w[2] >= 0 {
                    let rgb = color.map(|c| (c >> FRAC) as i32);
                    let texel = (u >> FRAC) as i32 & 0xFF;
                    let texel_v = (v >> FRAC) as i32 & 0xFF;
                    self.shade(p, s, (x, y), rgb, (texel, texel_v));
                }
                for i in 0..3 {
                    w[i] += step_x[i];
                    color[i] += shade[i].0;
                }
                u += uv[0].0;
                v += uv[1].0;
            }
        }
    }

    /// Texel (u, v) da textura, já com a janela de textura; 0 = transparente.
    fn texel(&self, t: &Texture, u: i32, v: i32) -> u16 {
        let w = self.texture_window;
        let (mask_x, mask_y) = ((w & 0x1F) as i32 * 8, ((w >> 5) & 0x1F) as i32 * 8);
        let (off_x, off_y) = (((w >> 10) & 0x1F) as i32 * 8, ((w >> 15) & 0x1F) as i32 * 8);
        let u = (u & !mask_x) | (off_x & mask_x);
        let v = (v & !mask_y) | (off_y & mask_y);
        let y = (t.base_y + v) as usize;
        match t.depth {
            Depth::Bits4 => {
                let word = self.pixel((t.base_x + u / 4) as usize, y);
                let index = (word >> ((u & 3) * 4)) & 0xF;
                self.clut_cache[usize::from(index)]
            }
            Depth::Bits8 => {
                let word = self.pixel((t.base_x + u / 2) as usize, y);
                let index = (word >> ((u & 1) * 8)) & 0xFF;
                self.clut_cache[usize::from(index)]
            }
            Depth::Bits15 => self.pixel((t.base_x + u) as usize, y),
        }
    }

    /// Cor final do pixel (`x`, `y`) na escala `s` e escrita.
    fn shade(
        &mut self,
        p: &Primitive,
        s: i32,
        (x, y): (i32, i32),
        rgb: [i32; 3],
        (u, v): (i32, i32),
    ) {
        // O dithering segue a grade 1× (mesmo padrão na imagem ampliada).
        let (dx, dy) = if s == 1 { (x, y) } else { (x / s, y / s) };
        if let Some((color, semi)) = self.color(p, dx, dy, rgb, u, v) {
            self.put(p, s, x, y, color, semi);
        }
    }

    /// Cor de 15 bits do pixel 1× (`x`, `y`) e se ele é semitransparente;
    /// `None` para texel transparente.
    fn color(
        &self,
        p: &Primitive,
        x: i32,
        y: i32,
        rgb: [i32; 3],
        u: i32,
        v: i32,
    ) -> Option<(u16, bool)> {
        let dither = if p.dither {
            DITHER[(y & 3) as usize][(x & 3) as usize]
        } else {
            0
        };
        let to5 = |c: i32| ((c + dither).clamp(0, 255) >> 3) as u16;
        Some(match &p.texture {
            Some(t) => {
                let texel = self.texel(t, u, v);
                if texel == 0 {
                    return None;
                }
                let color = if t.raw {
                    texel & 0x7FFF
                } else {
                    let channel = |shift: u16, c: i32| {
                        let t5 = i32::from((texel >> shift) & 0x1F);
                        to5((t5 * c) >> 4) << shift
                    };
                    channel(0, rgb[0]) | channel(5, rgb[1]) | channel(10, rgb[2])
                };
                (
                    color | (texel & 0x8000),
                    p.semi_transparent && texel & 0x8000 != 0,
                )
            }
            None => (
                to5(rgb[0]) | (to5(rgb[1]) << 5) | (to5(rgb[2]) << 10),
                p.semi_transparent,
            ),
        })
    }

    /// Escreve a cor no pixel (`x`, `y`) da escala `s`, com a máscara e a
    /// semitransparência sobre o que já está lá.
    fn put(&mut self, p: &Primitive, s: i32, x: i32, y: i32, mut color: u16, semi: bool) {
        let (mask_check, mask_set) = (self.mask_check, self.mask_set);
        let pixel = if s == 1 {
            let (px, py) = (x as usize, y as usize);
            &mut self.vram[(py % VRAM_HEIGHT) * VRAM_WIDTH + (px % VRAM_WIDTH)]
        } else {
            match self.upscaled.as_mut().and_then(|up| up.pixel_mut(x, y)) {
                Some(pixel) => pixel,
                None => return,
            }
        };
        let background = *pixel;
        if mask_check && background & 0x8000 != 0 {
            return;
        }
        if semi {
            color = blend(background, color, p.blend) | (color & 0x8000);
        }
        if mask_set {
            color |= 0x8000;
        }
        *pixel = color;
    }

    fn lines(&mut self, words: &[u32]) {
        let command = words[0] >> 24;
        let gouraud = command & 0x10 != 0;
        let primitive = Primitive {
            texture: None,
            semi_transparent: command & 0x02 != 0,
            blend: (self.draw_mode >> 5) & 3,
            dither: self.dither_enabled(),
        };
        let is_terminator = |w: u32| w & 0xF000_F000 == 0x5000_5000;
        let mut points = Vec::new();
        let mut color = words[0];
        let mut i = 1;
        loop {
            if gouraud && !points.is_empty() {
                match words.get(i) {
                    Some(&w) if !(self.polyline && is_terminator(w)) => color = w,
                    _ => break,
                }
                i += 1;
            }
            match words.get(i) {
                Some(&w) if !(self.polyline && points.len() >= 2 && is_terminator(w)) => {
                    let (x, y) = self.vertex(w);
                    points.push(Vertex {
                        x,
                        y,
                        color: rgb_of(color),
                        u: 0,
                        v: 0,
                    });
                }
                _ => break,
            }
            i += 1;
            if !self.polyline && points.len() == 2 {
                break;
            }
        }
        for pair in points.windows(2) {
            self.line(&primitive, pair[0], pair[1]);
        }
    }

    fn line(&mut self, p: &Primitive, a: Vertex, b: Vertex) {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        if dx.abs() >= 1024 || dy.abs() >= 512 {
            return;
        }
        let steps = dx.abs().max(dy.abs());
        // Posição com 32 bits de fração; o passo é arredondado para longe
        // do zero e o início fica meio pixel menos um pouco (como no chip),
        // o que decide em qual pixel cada degrau da linha cai.
        let position_step = |d: i32| -> i64 {
            if steps == 0 {
                return 0;
            }
            let delta = i64::from(d) << 32;
            let k = i64::from(steps);
            let rounding = if delta > 0 {
                k - 1
            } else if delta < 0 {
                1 - k
            } else {
                0
            };
            (delta + rounding) / k
        };
        let (sx, sy) = (position_step(dx), position_step(dy));
        // x sempre um pouco antes do meio; y só quando a linha sobe.
        let start = |v: i32| (i64::from(v) << 32) + (1 << 31);
        let mut x = start(a.x) - 1024;
        let mut y = start(a.y) - if sy < 0 { 1024 } else { 0 };
        let to_fixed = |v: i32| i64::from(v) << FRAC;
        let color_step = |d: i32| {
            if steps == 0 {
                0
            } else {
                to_fixed(d) / i64::from(steps)
            }
        };
        let sc = [0, 1, 2].map(|i| color_step(b.color[i] - a.color[i]));
        let half = 1i64 << (FRAC - 1);
        let mut color = [0, 1, 2].map(|i| to_fixed(a.color[i]) + half);
        let upscale = self.upscaled_scale();
        for _ in 0..=steps {
            let (px, py) = ((x >> 32) as i32, (y >> 32) as i32);
            if (self.area_left..=self.area_right).contains(&px)
                && (self.area_top..=self.area_bottom).contains(&py)
            {
                let rgb = color.map(|c| (c >> FRAC) as i32);
                if let Some((value, semi)) = self.color(p, px, py, rgb, 0, 0) {
                    // Na cópia ampliada a linha ocupa os blocos dos pixels 1×.
                    if let Some(s) = upscale {
                        for y in py * s..(py + 1) * s {
                            for x in px * s..(px + 1) * s {
                                self.put(p, s, x, y, value, semi);
                            }
                        }
                    }
                    self.put(p, 1, px, py, value, semi);
                }
            }
            x += sx;
            y += sy;
            for i in 0..3 {
                color[i] += sc[i];
            }
        }
    }

    fn rectangle(&mut self, words: &[u32]) {
        let command = words[0] >> 24;
        let textured = command & 0x04 != 0;
        let raw = command & 0x01 != 0;
        let (x0, y0) = self.vertex(words[1]);
        let mut at = 2;
        let (mut u0, mut v0, mut clut) = (0, 0, 0);
        if textured {
            u0 = (words[at] & 0xFF) as i32;
            v0 = ((words[at] >> 8) & 0xFF) as i32;
            clut = words[at] >> 16;
            at += 1;
        }
        let (w, h) = match (command >> 3) & 3 {
            0 => (
                (words[at] & 0x3FF) as i32,
                ((words[at] >> 16) & 0x1FF) as i32,
            ),
            1 => (1, 1),
            2 => (8, 8),
            _ => (16, 16),
        };
        let texture = textured.then(|| self.texture_from(clut, self.draw_mode, raw));
        if let Some(t) = &texture {
            self.update_clut_cache(t, clut);
        }
        let color = if textured && raw {
            [128; 3]
        } else {
            rgb_of(words[0])
        };
        let primitive = Primitive {
            texture,
            semi_transparent: command & 0x02 != 0,
            blend: (self.draw_mode >> 5) & 3,
            dither: false,
        };
        // Espelhado em X, o chip começa no texel ímpar (u0 | 1).
        if self.flip_x {
            u0 |= 1;
        }
        let rect = (x0, y0, w, h, u0, v0);
        if let Some(s) = self.upscaled_scale() {
            self.rectangle_at(s, &primitive, rect, color);
        }
        self.rectangle_at(1, &primitive, rect, color);
    }

    /// Retângulo (`x0`, `y0`, `w`, `h`, `u0`, `v0`) na escala `s`: cada
    /// pixel 1× vira um bloco s×s com o mesmo texel.
    fn rectangle_at(
        &mut self,
        s: i32,
        p: &Primitive,
        (x0, y0, w, h, u0, v0): (i32, i32, i32, i32, i32, i32),
        color: [i32; 3],
    ) {
        let (x0s, y0s) = (x0 * s, y0 * s);
        let left = x0s.max(self.area_left * s);
        let right = ((x0 + w) * s - 1).min(self.area_right * s + s - 1);
        let top = y0s.max(self.area_top * s);
        let bottom = ((y0 + h) * s - 1).min(self.area_bottom * s + s - 1);
        if left > right {
            return;
        }
        for y in top..=bottom {
            let dy = (y - y0s) / s;
            let v = if self.flip_y { v0 - dy } else { v0 + dy } & 0xFF;
            // Coluna 1× e posição dentro do bloco, sem dividir por pixel.
            let (mut dx, mut sub) = ((left - x0s) / s, (left - x0s) % s);
            for x in left..=right {
                let u = if self.flip_x { u0 - dx } else { u0 + dx } & 0xFF;
                self.shade(p, s, (x, y), color, (u, v));
                sub += 1;
                if sub == s {
                    sub = 0;
                    dx += 1;
                }
            }
        }
    }

    /// GP0 02h: preenche um retângulo (sem área de desenho nem máscara).
    fn fill(&mut self, words: &[u32]) {
        let color = rgb15(words[0]);
        let x = words[1] & 0x3F0;
        let y = (words[1] >> 16) & 0x1FF;
        let w = ((words[2] & 0x3FF) + 0xF) & !0xF;
        let h = (words[2] >> 16) & 0x1FF;
        for dy in 0..h {
            for dx in 0..w {
                self.set_pixel((x + dx) as usize, (y + dy) as usize, color);
            }
        }
        if let Some(up) = &mut self.upscaled {
            up.fill((x as usize, y as usize, w as usize, h as usize), color);
        }
    }

    /// GP0 80h: cópia VRAM → VRAM, pixel a pixel (com a máscara).
    fn copy(&mut self, words: &[u32]) {
        let (sx, sy) = (words[1] & 0x3FF, (words[1] >> 16) & 0x1FF);
        let (dx, dy) = (words[2] & 0x3FF, (words[2] >> 16) & 0x1FF);
        let w = ((words[3].wrapping_sub(1)) & 0x3FF) + 1;
        let h = (((words[3] >> 16).wrapping_sub(1)) & 0x1FF) + 1;
        // Com o destino à direita da origem o chip copia cada linha da
        // direita para a esquerda (visível quando as áreas se sobrepõem).
        let reverse = sx < dx;
        for y in 0..h {
            for x in 0..w {
                let x = if reverse { w - 1 - x } else { x };
                let (from_x, from_y) = ((sx + x) as usize, (sy + y) as usize);
                let (to_x, to_y) = ((dx + x) as usize, (dy + y) as usize);
                let value = self.pixel(from_x, from_y);
                if self.write_masked(to_x, to_y, value)
                    && let Some(up) = &mut self.upscaled
                {
                    let wrap_xy = |x: usize, y: usize| (x % VRAM_WIDTH, y % VRAM_HEIGHT);
                    let mask = if self.mask_set { 0x8000 } else { 0 };
                    up.copy_block(wrap_xy(from_x, from_y), wrap_xy(to_x, to_y), mask);
                }
            }
        }
    }
}

/// Retângulo de uma transferência (posição e tamanho com as voltas do chip).
fn transfer_rect(words: &[u32]) -> (u32, u32, u32, u32) {
    let x = words[1] & 0x3FF;
    let y = (words[1] >> 16) & 0x1FF;
    let w = ((words[2].wrapping_sub(1)) & 0x3FF) + 1;
    let h = (((words[2] >> 16).wrapping_sub(1)) & 0x1FF) + 1;
    (x, y, w, h)
}

/// Semitransparência entre o fundo `b` e a cor `f` (15 bits).
fn blend(b: u16, f: u16, mode: u32) -> u16 {
    let channel = |shift: u16| {
        let bc = i32::from((b >> shift) & 0x1F);
        let fc = i32::from((f >> shift) & 0x1F);
        let v = match mode {
            0 => (bc + fc) >> 1,
            1 => bc + fc,
            2 => bc - fc,
            _ => bc + (fc >> 2),
        };
        (v.clamp(0, 31) as u16) << shift
    };
    channel(0) | channel(5) | channel(10)
}

#[cfg(test)]
mod tests;
