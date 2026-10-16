//! VDP do Master System (TMS9918 derivado), modo 4, 256×192, NTSC.
//!
//! Renderiza linha a linha: o [`crate::Console`] chama [`Vdp::render_line`]
//! no início de cada linha e [`Vdp::end_line`] no fim, para que mudanças de
//! registradores feitas por interrupções de linha (efeitos raster) valham a
//! partir da linha seguinte, como no hardware.

use crate::state::{Reader, StateError, Writer};

pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 192;
/// Linhas por quadro em NTSC.
pub const LINES_PER_FRAME: u16 = 262;

const VRAM_SIZE: usize = 0x4000;
const CRAM_SIZE: usize = 32;
/// Fim da lista de sprites no modo 192 linhas.
const SPRITE_LIST_END: u8 = 0xD0;
const MAX_SPRITES_PER_LINE: usize = 8;

const STATUS_VBLANK: u8 = 0x80;
const STATUS_OVERFLOW: u8 = 0x40;
const STATUS_COLLISION: u8 = 0x20;

#[derive(Debug, Clone)]
pub struct Vdp {
    vram: Box<[u8; VRAM_SIZE]>,
    cram: [u8; CRAM_SIZE],
    regs: [u8; 16],
    address: u16,
    code: u8,
    latch: Option<u8>,
    read_buffer: u8,
    status: u8,
    line_irq_pending: bool,
    line_counter: u8,
    /// Linha sendo processada (0..262).
    line: u16,
    /// Rolagem vertical travada no início do quadro.
    vscroll: u8,
    framebuffer: Box<[u32; WIDTH * HEIGHT]>,
}

impl Default for Vdp {
    fn default() -> Self {
        Self::new()
    }
}

impl Vdp {
    pub fn new() -> Self {
        Self {
            vram: Box::new([0; VRAM_SIZE]),
            cram: [0; CRAM_SIZE],
            regs: [0; 16],
            address: 0,
            code: 0,
            latch: None,
            read_buffer: 0,
            status: 0,
            line_irq_pending: false,
            line_counter: 0xFF,
            line: 0,
            vscroll: 0,
            framebuffer: Box::new([0; WIDTH * HEIGHT]),
        }
    }

    pub fn framebuffer(&self) -> &[u32] {
        self.framebuffer.as_slice()
    }

    // ---- Portas -----------------------------------------------------------

    pub fn write_control(&mut self, v: u8) {
        match self.latch.take() {
            None => {
                self.latch = Some(v);
                // O byte baixo do endereço é atualizado já na primeira escrita.
                self.address = (self.address & 0x3F00) | u16::from(v);
            }
            Some(low) => {
                self.code = v >> 6;
                self.address = (u16::from(v & 0x3F) << 8) | u16::from(low);
                match self.code {
                    0 => {
                        self.read_buffer = self.vram[self.vram_index()];
                        self.increment_address();
                    }
                    2 => self.regs[usize::from(v & 0x0F)] = low,
                    _ => {}
                }
            }
        }
    }

    pub fn write_data(&mut self, v: u8) {
        self.latch = None;
        if self.code == 3 {
            self.cram[usize::from(self.address) % CRAM_SIZE] = v;
        } else {
            let i = self.vram_index();
            self.vram[i] = v;
        }
        self.read_buffer = v;
        self.increment_address();
    }

    pub fn read_data(&mut self) -> u8 {
        self.latch = None;
        let v = self.read_buffer;
        self.read_buffer = self.vram[self.vram_index()];
        self.increment_address();
        v
    }

    /// Lê o status e limpa as flags e as interrupções pendentes.
    pub fn read_status(&mut self) -> u8 {
        self.latch = None;
        let v = self.status | 0x1F;
        self.status = 0;
        self.line_irq_pending = false;
        v
    }

    /// Contador vertical NTSC 192 linhas: 0x00–0xDA, depois 0xD5–0xFF.
    pub fn v_counter(&self) -> u8 {
        if self.line <= 0xDA {
            self.line as u8
        } else {
            (self.line - 6) as u8
        }
    }

    /// Linha /INT: VBlank (se habilitado em R1) ou linha (se em R0).
    pub fn irq(&self) -> bool {
        let frame = self.status & STATUS_VBLANK != 0 && self.regs[1] & 0x20 != 0;
        let line = self.line_irq_pending && self.regs[0] & 0x10 != 0;
        frame || line
    }

    fn vram_index(&self) -> usize {
        usize::from(self.address) % VRAM_SIZE
    }

    fn increment_address(&mut self) {
        self.address = (self.address + 1) & 0x3FFF;
    }

    // ---- Temporização -----------------------------------------------------

    /// Prepara a linha `line` e, se for visível, a desenha.
    pub fn render_line(&mut self, line: u16) {
        self.line = line;
        if line == 0 {
            self.vscroll = self.regs[9];
        }
        if usize::from(line) < HEIGHT {
            self.draw_line(usize::from(line));
        }
    }

    /// Fim da linha atual: contador de linha e VBlank.
    pub fn end_line(&mut self) {
        let line = self.line;
        if usize::from(line) <= HEIGHT {
            let (next, underflow) = self.line_counter.overflowing_sub(1);
            if underflow {
                self.line_counter = self.regs[10];
                self.line_irq_pending = true;
            } else {
                self.line_counter = next;
            }
        } else {
            self.line_counter = self.regs[10];
        }
        if usize::from(line) == HEIGHT {
            self.status |= STATUS_VBLANK;
        }
    }

    // ---- Renderização -----------------------------------------------------

    fn display_enabled(&self) -> bool {
        self.regs[1] & 0x40 != 0
    }

    fn backdrop(&self) -> u8 {
        16 + (self.regs[7] & 0x0F)
    }

    fn draw_line(&mut self, line: usize) {
        let mut colors = [self.backdrop(); WIDTH];
        if self.display_enabled() {
            let mut priority = [false; WIDTH];
            self.draw_background(line, &mut colors, &mut priority);
            self.draw_sprites(line, &mut colors, &priority);
            if self.regs[0] & 0x20 != 0 {
                colors[..8].fill(self.backdrop());
            }
        }
        let row = &mut self.framebuffer[line * WIDTH..(line + 1) * WIDTH];
        for (out, &c) in row.iter_mut().zip(colors.iter()) {
            *out = cram_to_rgb(self.cram[usize::from(c)]);
        }
    }

    fn draw_background(&self, line: usize, colors: &mut [u8; WIDTH], priority: &mut [bool; WIDTH]) {
        let name_table = usize::from(self.regs[2] & 0x0E) << 10;
        let lock_top = self.regs[0] & 0x40 != 0 && line < 16;
        let lock_right = self.regs[0] & 0x80 != 0;
        let hscroll = if lock_top { 0 } else { self.regs[8] };

        for x in 0..WIDTH {
            let vscroll = if lock_right && x >= 192 {
                0
            } else {
                self.vscroll
            };
            let src_y = (line + usize::from(vscroll)) % 224;
            let src_x = (x as u8).wrapping_sub(hscroll);
            let col = usize::from(src_x >> 3);
            let row = src_y >> 3;

            let entry_addr = (name_table + (row * 32 + col) * 2) % VRAM_SIZE;
            let entry = u16::from(self.vram[entry_addr])
                | (u16::from(self.vram[(entry_addr + 1) % VRAM_SIZE]) << 8);
            let tile = usize::from(entry & 0x01FF);
            let hflip = entry & 0x0200 != 0;
            let vflip = entry & 0x0400 != 0;
            let palette = if entry & 0x0800 != 0 { 16 } else { 0 };
            let front = entry & 0x1000 != 0;

            let py = if vflip { 7 - (src_y & 7) } else { src_y & 7 };
            let px = if hflip {
                7 - usize::from(src_x & 7)
            } else {
                usize::from(src_x & 7)
            };
            let color = self.tile_pixel(tile, py, px);
            colors[x] = palette + color;
            priority[x] = front && color != 0;
        }
    }

    fn draw_sprites(&mut self, line: usize, colors: &mut [u8; WIDTH], bg_priority: &[bool; WIDTH]) {
        let sat = usize::from(self.regs[5] & 0x7E) << 7;
        let tall = self.regs[1] & 0x02 != 0;
        let zoom = if self.regs[1] & 0x01 != 0 { 2 } else { 1 };
        let height = if tall { 16 } else { 8 } * zoom;
        let pattern_base = if self.regs[6] & 0x04 != 0 { 256 } else { 0 };
        let shift_left = self.regs[0] & 0x08 != 0;

        let mut on_line = [0usize; MAX_SPRITES_PER_LINE];
        let mut count = 0;
        for i in 0..64 {
            let y = self.vram[sat + i];
            if y == SPRITE_LIST_END {
                break;
            }
            // O Y gravado é a linha anterior ao topo; valores altos dão a volta.
            let top = (usize::from(y) + 1) % 256;
            let row = (line + 256 - top) % 256;
            if row < height {
                if count == MAX_SPRITES_PER_LINE {
                    self.status |= STATUS_OVERFLOW;
                    break;
                }
                on_line[count] = i;
                count += 1;
            }
        }

        let mut drawn = [false; WIDTH];
        for &i in &on_line[..count] {
            let y = self.vram[sat + i];
            let row = (line + 256 - (usize::from(y) + 1) % 256) % 256 / zoom;
            let x = i32::from(self.vram[sat + 0x80 + i * 2]) - if shift_left { 8 } else { 0 };
            let mut tile = usize::from(self.vram[sat + 0x81 + i * 2]);
            if tall {
                tile &= !1;
            }
            let tile = pattern_base + tile + row / 8;

            for dx in 0..8 * zoom {
                let sx = x + dx as i32;
                if !(0..WIDTH as i32).contains(&sx) {
                    continue;
                }
                let sx = sx as usize;
                let color = self.tile_pixel(tile, row % 8, dx / zoom);
                if color == 0 {
                    continue;
                }
                if drawn[sx] {
                    self.status |= STATUS_COLLISION;
                    continue;
                }
                drawn[sx] = true;
                if !bg_priority[sx] {
                    colors[sx] = 16 + color;
                }
            }
        }
    }

    /// Cor (0–15) do pixel de um tile 4bpp em planos intercalados.
    fn tile_pixel(&self, tile: usize, row: usize, col: usize) -> u8 {
        let addr = (tile * 32 + row * 4) % VRAM_SIZE;
        let bit = 7 - col;
        (0..4).fold(0, |acc, plane| {
            acc | (((self.vram[addr + plane] >> bit) & 1) << plane)
        })
    }
}

impl Vdp {
    pub(crate) fn save_state(&self, w: &mut Writer) {
        w.slice(self.vram.as_slice());
        w.slice(&self.cram);
        w.slice(&self.regs);
        w.u16(self.address);
        w.u8(self.code);
        w.bool(self.latch.is_some());
        w.u8(self.latch.unwrap_or(0));
        w.u8(self.read_buffer);
        w.u8(self.status);
        w.bool(self.line_irq_pending);
        w.u8(self.line_counter);
        w.u16(self.line);
        w.u8(self.vscroll);
    }

    /// O framebuffer não é salvo: o próximo quadro o redesenha por inteiro.
    pub(crate) fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        r.fill(self.vram.as_mut_slice())?;
        r.fill(&mut self.cram)?;
        r.fill(&mut self.regs)?;
        self.address = r.u16()?;
        if self.address > 0x3FFF {
            return Err(StateError::Invalid("endereço do VDP"));
        }
        self.code = r.u8()?;
        if self.code > 3 {
            return Err(StateError::Invalid("código do VDP"));
        }
        let latched = r.bool()?;
        let low = r.u8()?;
        self.latch = latched.then_some(low);
        self.read_buffer = r.u8()?;
        self.status = r.u8()?;
        self.line_irq_pending = r.bool()?;
        self.line_counter = r.u8()?;
        self.line = r.u16()?;
        if self.line >= LINES_PER_FRAME {
            return Err(StateError::Invalid("linha do VDP"));
        }
        self.vscroll = r.u8()?;
        Ok(())
    }
}

/// CRAM `--BBGGRR` para `0x00RRGGBB`.
fn cram_to_rgb(v: u8) -> u32 {
    let level = |c: u8| u32::from(c & 3) * 85;
    (level(v) << 16) | (level(v >> 2) << 8) | level(v >> 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_reg(vdp: &mut Vdp, reg: u8, value: u8) {
        vdp.write_control(value);
        vdp.write_control(0x80 | reg);
    }

    fn write_vram(vdp: &mut Vdp, addr: u16, bytes: &[u8]) {
        vdp.write_control(addr as u8);
        vdp.write_control(0x40 | (addr >> 8) as u8);
        for &b in bytes {
            vdp.write_data(b);
        }
    }

    fn write_cram(vdp: &mut Vdp, index: u8, color: u8) {
        vdp.write_control(index);
        vdp.write_control(0xC0);
        vdp.write_data(color);
    }

    /// Tela ligada, name table em 0x3800, SAT em 0x3F00.
    fn display_on() -> Vdp {
        let mut vdp = Vdp::new();
        set_reg(&mut vdp, 0, 0x04);
        set_reg(&mut vdp, 1, 0x40);
        set_reg(&mut vdp, 2, 0x0E);
        set_reg(&mut vdp, 5, 0x7E);
        write_vram(&mut vdp, 0x3F00, &[SPRITE_LIST_END]);
        vdp
    }

    #[test]
    fn control_port_writes_registers_and_auto_increments() {
        let mut vdp = Vdp::new();
        set_reg(&mut vdp, 7, 0x05);
        assert_eq!(vdp.regs[7], 0x05);
        write_vram(&mut vdp, 0x1234, &[0xAA, 0xBB]);
        assert_eq!(vdp.vram[0x1234], 0xAA);
        assert_eq!(vdp.vram[0x1235], 0xBB);

        // Leitura usa o buffer pré-carregado.
        vdp.write_control(0x34);
        vdp.write_control(0x12);
        assert_eq!(vdp.read_data(), 0xAA);
        assert_eq!(vdp.read_data(), 0xBB);
    }

    #[test]
    fn status_read_resets_latch_and_flags() {
        let mut vdp = Vdp::new();
        vdp.write_control(0x00);
        vdp.status = STATUS_VBLANK;
        assert_eq!(vdp.read_status() & 0x80, 0x80);
        assert_eq!(vdp.read_status() & 0x80, 0);
        assert_eq!(vdp.latch, None);
    }

    #[test]
    fn renders_background_tile_with_palette() {
        let mut vdp = display_on();
        // Tile 1: primeira linha com cor 1 em todos os pixels (plano 0 cheio).
        write_vram(&mut vdp, 32, &[0xFF, 0, 0, 0]);
        // Name table: posição (0,0) usa o tile 1.
        write_vram(&mut vdp, 0x3800, &[0x01, 0x00]);
        write_cram(&mut vdp, 1, 0x03); // vermelho
        vdp.render_line(0);
        assert_eq!(vdp.framebuffer()[0], 0xFF0000);
        assert_eq!(vdp.framebuffer()[7], 0xFF0000);
        assert_eq!(vdp.framebuffer()[8], 0x000000);
    }

    #[test]
    fn horizontal_scroll_moves_background() {
        let mut vdp = display_on();
        write_vram(&mut vdp, 32, &[0x80, 0, 0, 0]); // só o pixel 0
        write_vram(&mut vdp, 0x3800, &[0x01, 0x00]);
        write_cram(&mut vdp, 1, 0x0C); // verde
        set_reg(&mut vdp, 8, 3);
        vdp.render_line(0);
        assert_eq!(vdp.framebuffer()[3], 0x00FF00);
        assert_eq!(vdp.framebuffer()[0], 0);
    }

    #[test]
    fn sprite_is_drawn_over_background() {
        let mut vdp = display_on();
        // Tile 2 (padrão de sprite): primeira linha com cor 1.
        write_vram(&mut vdp, 64, &[0xFF, 0, 0, 0]);
        // Sprite 0: Y = 9 (topo na linha 10), X = 20, tile 2.
        write_vram(&mut vdp, 0x3F00, &[9, SPRITE_LIST_END]);
        write_vram(&mut vdp, 0x3F80, &[20, 2]);
        write_cram(&mut vdp, 17, 0x30); // azul
        vdp.render_line(10);
        let row = &vdp.framebuffer()[10 * WIDTH..11 * WIDTH];
        assert_eq!(row[20], 0x0000FF);
        assert_eq!(row[27], 0x0000FF);
        assert_eq!(row[28], 0);
    }

    #[test]
    fn ninth_sprite_on_line_sets_overflow() {
        let mut vdp = display_on();
        let ys = [0u8; 9];
        write_vram(&mut vdp, 0x3F00, &ys);
        write_vram(&mut vdp, 0x3F09, &[SPRITE_LIST_END]);
        vdp.render_line(1);
        assert_ne!(vdp.read_status() & STATUS_OVERFLOW, 0);
    }

    #[test]
    fn line_counter_raises_interrupt_every_n_lines() {
        let mut vdp = Vdp::new();
        set_reg(&mut vdp, 0, 0x14); // modo 4 + IE1
        set_reg(&mut vdp, 10, 2);
        // Fora da área visível o contador é recarregado.
        vdp.render_line(200);
        vdp.end_line();
        let mut fired = Vec::new();
        for line in 0..10 {
            vdp.render_line(line);
            vdp.end_line();
            if vdp.irq() {
                fired.push(line);
                vdp.read_status();
            }
        }
        assert_eq!(fired, vec![2, 5, 8]);
    }

    #[test]
    fn vblank_interrupt_after_last_visible_line() {
        let mut vdp = Vdp::new();
        set_reg(&mut vdp, 1, 0x20); // IE0
        vdp.render_line(191);
        vdp.end_line();
        assert!(!vdp.irq());
        vdp.render_line(192);
        vdp.end_line();
        assert!(vdp.irq());
        vdp.read_status();
        assert!(!vdp.irq());
    }

    #[test]
    fn v_counter_skips_after_da() {
        let mut vdp = Vdp::new();
        vdp.render_line(0xDA);
        assert_eq!(vdp.v_counter(), 0xDA);
        vdp.render_line(0xDB);
        assert_eq!(vdp.v_counter(), 0xD5);
        vdp.render_line(261);
        assert_eq!(vdp.v_counter(), 0xFF);
    }

    #[test]
    fn cram_color_conversion() {
        assert_eq!(cram_to_rgb(0x3F), 0xFFFFFF);
        assert_eq!(cram_to_rgb(0x01), 0x550000);
        assert_eq!(cram_to_rgb(0x08), 0x00AA00);
    }
}
