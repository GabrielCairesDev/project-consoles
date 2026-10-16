//! PPU: tempo de cada linha, registradores do LCD, VRAM, OAM e interrupções.
//!
//! Cada linha tem 456 ciclos: busca na OAM (modo 2, 80 ciclos), desenho
//! (modo 3) e H-blank (modo 0). As linhas 144–153 são o V-blank (modo 1).

use chips::state::{Reader, StateError, Writer};

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 144;
const DOTS_PER_LINE: u16 = 456;
const LINES: u8 = 154;
const OAM_SCAN_DOTS: u16 = 80;
const DRAW_DOTS: u16 = 172;

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
    frame: Vec<u32>,
}

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

    fn compute_stat_line(&self) -> bool {
        let e = self.stat_enable;
        (self.mode == 0 && e & 0x08 != 0)
            || (self.mode == 1 && e & 0x10 != 0)
            || (self.mode == 2 && e & 0x20 != 0)
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
            self.ly = (self.ly + 1) % LINES;
            if usize::from(self.ly) == HEIGHT {
                self.mode = 1;
                irq.vblank = true;
            } else if usize::from(self.ly) < HEIGHT {
                self.mode = 2;
            }
        } else if usize::from(self.ly) < HEIGHT {
            if self.dot == OAM_SCAN_DOTS {
                self.mode = 3;
            } else if self.dot == OAM_SCAN_DOTS + DRAW_DOTS {
                self.mode = 0;
                self.render_line();
            }
        }
        self.update_stat(&mut irq);
        irq
    }

    /// Desenha a linha `ly` (preenchido na etapa de vídeo).
    fn render_line(&mut self) {}

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
                } else if !was_on && self.lcd_on() {
                    self.ly = 0;
                    self.dot = 0;
                    self.mode = 2;
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
