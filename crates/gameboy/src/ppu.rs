//! PPU: tempo de cada linha, registradores do LCD, VRAM, OAM e interrupções.
//!
//! Cada linha tem 456 ciclos: busca na OAM (modo 2, 80 ciclos), desenho
//! (modo 3) e H-blank (modo 0). As linhas 144–153 são o V-blank (modo 1).
//!
//! O desenho é feito por linha inteira no fim do modo 3, não pixel a pixel
//! (FIFO): basta para os jogos e para o dmg-acid2; efeitos que mudam
//! registradores no meio do modo 3 não aparecem.

use chips::state::{Reader, StateError, Writer};

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 144;
const DOTS_PER_LINE: u16 = 456;
const LINES: u8 = 154;
const OAM_SCAN_DOTS: u16 = 80;
const DRAW_DOTS: u16 = 172;
/// Ponto em que a primeira linha começa ao ligar o LCD (calibrado pela
/// sequência de LY do `lcdon_timing` do Mooneye).
const LCD_ON_FIRST_DOT: u16 = 4;

/// Interrupções geradas pela PPU num passo.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Interrupts {
    pub vblank: bool,
    pub stat: bool,
}

#[derive(Debug, Clone)]
pub struct Ppu {
    pub vram: Box<[u8; 0x2000]>,
    pub oam: Box<[u8; 0xA0]>,
    lcdc: u8,
    /// Bits 3–6 de STAT (habilitação das fontes de interrupção).
    stat_enable: u8,
    scy: u8,
    scx: u8,
    ly: u8,
    lyc: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    wy: u8,
    wx: u8,
    dot: u16,
    mode: u8,
    /// Linha de interrupção do STAT no passo anterior (dispara na subida).
    stat_line: bool,
    /// Linha interna da janela: só avança nas linhas em que ela aparece.
    window_line: u8,
    /// O LY chegou ao WY neste quadro (a janela pode aparecer).
    window_reached: bool,
    /// O H-blank já pediu interrupção nesta linha (4 pontos antes de o
    /// STAT mostrar o modo 0).
    hblank_irq: bool,
    /// Primeira linha depois de ligar o LCD.
    first_line: bool,
    /// Duração do modo 3 da linha atual.
    draw_dots: u16,
    /// Quadro em desenho e o último completo (trocados no V-blank).
    back: Vec<u32>,
    frame: Vec<u32>,
}

/// Até 10 sprites por linha, escolhidos na ordem da OAM.
const MAX_SPRITES_PER_LINE: usize = 10;

impl Default for Ppu {
    fn default() -> Self {
        Self {
            vram: Box::new([0; 0x2000]),
            oam: Box::new([0; 0xA0]),
            lcdc: 0x91,
            stat_enable: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            dot: 0,
            mode: 2,
            stat_line: false,
            window_line: 0,
            window_reached: false,
            draw_dots: DRAW_DOTS,
            hblank_irq: false,
            first_line: false,
            back: vec![SHADES[0]; WIDTH * HEIGHT],
            frame: vec![SHADES[0]; WIDTH * HEIGHT],
        }
    }
}

/// Tons de cinza neutros, do mais claro ao mais escuro.
pub const SHADES: [u32; 4] = [0x00E8_E8E8, 0x00A0_A0A0, 0x0058_5858, 0x0010_1010];

impl Ppu {
    fn lcd_on(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    /// Imagem do último quadro (160×144, `0x00RRGGBB`).
    pub fn frame(&self) -> &[u32] {
        &self.frame
    }

    /// Ponto 0 de uma linha visível: a busca de OAM começa (o STAT ainda
    /// mostra o modo 0, mas a interrupção do modo 2 já vale).
    fn oam_scan_starting(&self) -> bool {
        self.dot == 0 && usize::from(self.ly) < HEIGHT && !self.first_line
    }

    fn compute_stat_line(&self) -> bool {
        let e = self.stat_enable;
        ((self.mode == 0 && !self.oam_scan_starting() || self.hblank_irq) && e & 0x08 != 0)
            || (self.mode == 1 && e & 0x10 != 0)
            || ((self.mode == 2 || self.oam_scan_starting()) && e & 0x20 != 0)
            // No DMG a fonte do modo 2 também dispara ao entrar no V-blank.
            || (usize::from(self.ly) == HEIGHT && self.dot == 0 && e & 0x20 != 0)
            || (self.ly == self.lyc && e & 0x40 != 0)
    }

    fn update_stat(&mut self, irq: &mut Interrupts) {
        let line = self.lcd_on() && self.compute_stat_line();
        if line && !self.stat_line {
            irq.stat = true;
        }
        self.stat_line = line;
    }

    /// Avança um M-ciclo (4 pontos).
    pub fn tick(&mut self) -> Interrupts {
        let mut irq = Interrupts::default();
        if !self.lcd_on() {
            return irq;
        }
        self.dot += 4;
        if self.dot >= DOTS_PER_LINE {
            self.dot = 0;
            self.hblank_irq = false;
            self.first_line = false;
            self.ly = (self.ly + 1) % LINES;
            if usize::from(self.ly) == HEIGHT {
                self.mode = 1;
                irq.vblank = true;
                std::mem::swap(&mut self.frame, &mut self.back);
            } else if usize::from(self.ly) < HEIGHT {
                // O STAT só mostra o modo 2 no M-ciclo seguinte; a
                // interrupção do modo 2 é já (ver `compute_stat_line`).
                self.mode = 0;
                if self.ly == 0 {
                    self.window_line = 0;
                    self.window_reached = false;
                }
            }
        } else if usize::from(self.ly) < HEIGHT {
            // O modo que a CPU lê muda 1 ponto depois do fim de cada fase
            // (a interrupção do H-blank, no ponto exato); a CPU só percebe
            // no M-ciclo seguinte, pois aqui o tempo anda de 4 em 4.
            if self.mode == 0 && !self.hblank_irq && !self.first_line && self.dot > 0 {
                self.mode = 2;
            }
            if (self.mode == 2 || self.first_line && self.mode == 0 && !self.hblank_irq)
                && self.dot > OAM_SCAN_DOTS
            {
                self.mode = 3;
                self.draw_dots = self.draw_length();
            } else if self.mode == 3 {
                let end = OAM_SCAN_DOTS + self.draw_dots;
                if self.dot >= end {
                    self.hblank_irq = true;
                }
                if self.dot > end {
                    self.mode = 0;
                    self.render_line();
                }
            }
        }
        self.update_stat(&mut irq);
        irq
    }

    /// Duração do modo 3: 172 pontos, mais o descarte dos pixels do SCX
    /// e o tempo de buscar cada sprite da linha.
    fn draw_length(&self) -> u16 {
        let mut dots = DRAW_DOTS + u16::from(self.scx & 7);
        if self.lcdc & 0x02 == 0 {
            return dots;
        }
        let height: i16 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        // Colunas de 8 pixels do fundo que já pagaram a espera da busca.
        let mut columns_paid = [false; 32];
        let mut count = 0;
        for i in 0..40 {
            let y = i16::from(self.oam[i * 4]) - 16;
            if !(y..y + height).contains(&i16::from(self.ly)) {
                continue;
            }
            count += 1;
            let x = u16::from(self.oam[i * 4 + 1]);
            // Fora da tela à direita: ocupa a vaga, mas não atrasa.
            if x < 168 {
                let position = x + u16::from(self.scx & 7);
                let column = usize::from(position / 8);
                dots += 6;
                if !columns_paid[column] {
                    columns_paid[column] = true;
                    dots += 5u16.saturating_sub(position % 8);
                }
            }
            if count == MAX_SPRITES_PER_LINE {
                break;
            }
        }
        dots
    }

    /// Índice de cor (0–3) da coluna `x` da linha de tile em `addr` (cada
    /// linha de tile tem 2 bytes: bits baixos e altos da cor).
    fn tile_pixel(&self, addr: usize, x: u8) -> u8 {
        let lo = self.vram[addr];
        let hi = self.vram[addr + 1];
        let bit = 7 - (x & 7);
        ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1)
    }

    /// Endereço (na VRAM) da linha `row` do tile de fundo/janela `index`.
    fn bg_tile_addr(&self, index: u8, row: u8) -> usize {
        let base = if self.lcdc & 0x10 != 0 {
            usize::from(index) * 16
        } else {
            // Modo 8800: índice com sinal a partir de 0x9000.
            (0x1000 + i32::from(index as i8) * 16) as usize
        };
        base + usize::from(row & 7) * 2
    }

    /// Cor (0–3) do fundo/janela na posição `(x, y)` do mapa `map_base`.
    fn map_pixel(&self, map_base: usize, x: u8, y: u8) -> u8 {
        let index = self.vram[map_base + usize::from(y / 8) * 32 + usize::from(x / 8)];
        self.tile_pixel(self.bg_tile_addr(index, y), x)
    }

    fn shade(palette: u8, color: u8) -> u32 {
        SHADES[usize::from((palette >> (color * 2)) & 3)]
    }

    /// Desenha a linha `ly` no quadro em andamento.
    fn render_line(&mut self) {
        let ly = self.ly;
        let mut bg_colors = [0u8; WIDTH];

        if ly == self.wy {
            self.window_reached = true;
        }
        if self.lcdc & 0x01 != 0 {
            let bg_map = if self.lcdc & 0x08 != 0 {
                0x1C00
            } else {
                0x1800
            };
            let y = ly.wrapping_add(self.scy);
            for (x, color) in bg_colors.iter_mut().enumerate() {
                *color = self.map_pixel(bg_map, (x as u8).wrapping_add(self.scx), y);
            }
            let window_x = i16::from(self.wx) - 7;
            if self.lcdc & 0x20 != 0 && self.window_reached && window_x < WIDTH as i16 {
                let win_map = if self.lcdc & 0x40 != 0 {
                    0x1C00
                } else {
                    0x1800
                };
                for x in window_x.max(0)..WIDTH as i16 {
                    let wx = (x - window_x) as u8;
                    bg_colors[x as usize] = self.map_pixel(win_map, wx, self.window_line);
                }
                self.window_line += 1;
            }
        }

        let row = usize::from(ly) * WIDTH;
        for (x, &color) in bg_colors.iter().enumerate() {
            self.back[row + x] = Self::shade(self.bgp, color);
        }
        if self.lcdc & 0x02 != 0 {
            self.render_sprites(ly, &bg_colors);
        }
    }

    fn render_sprites(&mut self, ly: u8, bg_colors: &[u8; WIDTH]) {
        let height: i16 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let mut sprites: Vec<(u8, usize)> = Vec::with_capacity(MAX_SPRITES_PER_LINE);
        for i in 0..40 {
            let y = i16::from(self.oam[i * 4]) - 16;
            if (y..y + height).contains(&i16::from(ly)) {
                sprites.push((self.oam[i * 4 + 1], i));
                if sprites.len() == MAX_SPRITES_PER_LINE {
                    break;
                }
            }
        }
        // No DMG ganha o menor X; no empate, o que vem antes na OAM. Desenha
        // do menos prioritário para o mais, que fica por cima.
        sprites.sort_by_key(|&(x, i)| (x, i));
        let row = usize::from(ly) * WIDTH;
        for &(sprite_x, i) in sprites.iter().rev() {
            let y = i16::from(self.oam[i * 4]) - 16;
            let attrs = self.oam[i * 4 + 3];
            let mut line = (i16::from(ly) - y) as u8;
            if attrs & 0x40 != 0 {
                line = (height as u8 - 1) - line;
            }
            let mut tile = self.oam[i * 4 + 2];
            if height == 16 {
                tile &= 0xFE;
            }
            let addr = usize::from(tile) * 16 + usize::from(line) * 2;
            let palette = if attrs & 0x10 != 0 {
                self.obp1
            } else {
                self.obp0
            };
            for px in 0..8u8 {
                let x = i16::from(sprite_x) - 8 + i16::from(px);
                if !(0..WIDTH as i16).contains(&x) {
                    continue;
                }
                let column = if attrs & 0x20 != 0 { 7 - px } else { px };
                let color = self.tile_pixel(addr, column);
                if color == 0 {
                    continue;
                }
                let x = x as usize;
                if attrs & 0x80 != 0 && bg_colors[x] != 0 {
                    continue;
                }
                self.back[row + x] = Self::shade(palette, color);
            }
        }
    }

    /// A CPU não acessa a VRAM durante o desenho.
    pub fn vram_accessible(&self) -> bool {
        !self.lcd_on() || self.mode != 3
    }

    /// A CPU não acessa a OAM durante a busca de sprites e o desenho.
    pub fn oam_accessible(&self) -> bool {
        !self.lcd_on() || self.mode < 2
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF40 => self.lcdc,
            0xFF41 => {
                let coincidence = if self.ly == self.lyc { 0x04 } else { 0 };
                let mode = if self.lcd_on() { self.mode } else { 0 };
                0x80 | self.stat_enable | coincidence | mode
            }
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            _ => 0xFF,
        }
    }

    /// Escreve um registrador; devolve as interrupções que a escrita causou.
    pub fn write(&mut self, addr: u16, value: u8) -> Interrupts {
        let mut irq = Interrupts::default();
        match addr {
            0xFF40 => {
                let was_on = self.lcd_on();
                self.lcdc = value;
                if was_on && !self.lcd_on() {
                    self.ly = 0;
                    self.dot = 0;
                    self.mode = 0;
                    self.stat_line = false;
                    // Com o LCD desligado a tela fica em branco.
                    self.frame.fill(SHADES[0]);
                } else if !was_on && self.lcd_on() {
                    // A primeira linha depois de ligar não faz a busca de
                    // OAM (fica em modo 0) e começa adiantada.
                    self.ly = 0;
                    self.dot = LCD_ON_FIRST_DOT;
                    self.mode = 0;
                    self.first_line = true;
                }
            }
            0xFF41 => self.stat_enable = value & 0x78,
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {}
            0xFF45 => self.lyc = value,
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            _ => {}
        }
        self.update_stat(&mut irq);
        irq
    }

    pub fn save_state(&self, w: &mut Writer) {
        w.slice(&self.vram[..]);
        w.slice(&self.oam[..]);
        for v in [
            self.lcdc,
            self.stat_enable,
            self.scy,
            self.scx,
            self.ly,
            self.lyc,
            self.bgp,
            self.obp0,
            self.obp1,
            self.wy,
            self.wx,
            self.mode,
        ] {
            w.u8(v);
        }
        w.u16(self.dot);
        w.bool(self.stat_line);
        w.u8(self.window_line);
        w.bool(self.window_reached);
    }

    pub fn load_state(&mut self, r: &mut Reader) -> Result<(), StateError> {
        let mut next = self.clone();
        r.fill(&mut next.vram[..])?;
        r.fill(&mut next.oam[..])?;
        next.lcdc = r.u8()?;
        next.stat_enable = r.u8()? & 0x78;
        next.scy = r.u8()?;
        next.scx = r.u8()?;
        next.ly = r.u8()?;
        next.lyc = r.u8()?;
        next.bgp = r.u8()?;
        next.obp0 = r.u8()?;
        next.obp1 = r.u8()?;
        next.wy = r.u8()?;
        next.wx = r.u8()?;
        next.mode = r.u8()?;
        next.dot = r.u16()?;
        next.stat_line = r.bool()?;
        next.window_line = r.u8()?;
        next.window_reached = r.bool()?;
        if next.ly >= LINES
            || next.mode > 3
            || next.dot >= DOTS_PER_LINE
            || !next.dot.is_multiple_of(4)
        {
            return Err(StateError::Invalid("PPU"));
        }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
