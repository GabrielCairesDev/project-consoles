//! Barramento do Mega Drive: mapa de memória do 68000 e do Z80.
//!
//! | 68000                | O que é                                   |
//! |----------------------|-------------------------------------------|
//! | 000000–3FFFFF        | ROM (e SRAM em 200000 quando mapeada)     |
//! | A00000–A0FFFF        | Espaço do Z80 (RAM, YM2612, banco)         |
//! | A10000–A1001F        | Versão e portas de controle               |
//! | A11100 / A11200      | BUSREQ e RESET do Z80                      |
//! | A130F1               | Controle da SRAM (jogos > 2 MB)            |
//! | C00000–C0001F        | VDP (dados, controle, HV, PSG)             |
//! | E00000–FFFFFF        | 64 KB de RAM, espelhada                    |

use chips::psg::Psg;
use chips::state::{Reader, StateError, Writer};
use chips::z80;

use crate::io::Port;
use crate::m68k;
use crate::vdp::Vdp;
use crate::ym2612::Ym2612;

const RAM_SIZE: usize = 0x10000;
const Z80_RAM_SIZE: usize = 0x2000;
const SRAM_BASE: u32 = 0x20_0000;
const SRAM_SIZE: usize = 0x1_0000;
/// Ciclos de master clock por linha (NTSC).
pub(crate) const MCLK_PER_LINE: u32 = 3420;
/// O 68000 roda a master clock / 7.
pub(crate) const MCLK_PER_M68K: u32 = 7;
/// Registrador de versão: exportação (Genesis), NTSC, sem Sega CD, sem TMSS.
const VERSION: u8 = 0xA0;

/// Bytes (VRAM) ou palavras (CRAM/VSRAM) por linha no DMA 68000 → VDP:
/// [tela ativa, blank] × [H32, H40].
const DMA_RATE: [[u32; 2]; 2] = [[16, 18], [167, 205]];

#[derive(Debug, Clone)]
pub(crate) struct System {
    rom: Vec<u8>,
    ram: Box<[u8; RAM_SIZE]>,
    sram: Option<Box<[u8; SRAM_SIZE]>>,
    /// SRAM visível em 200000 (sempre, em ROMs até 2 MB; senão via A130F1).
    sram_mapped: bool,
    pub vdp: Vdp,
    pub ports: [Port; 2],
    pub psg: Psg,
    pub ym: Ym2612,
    z80_ram: Box<[u8; Z80_RAM_SIZE]>,
    /// O 68000 pediu o barramento do Z80.
    pub z80_busreq: bool,
    /// Linha RESET do Z80 ativa (o Z80 liga em reset).
    pub z80_reset: bool,
    /// Banco de 32 KB do 68000 visto pelo Z80 em 8000–FFFF.
    z80_bank: u32,
    /// Interrupção do Z80 (acompanha o V-int por uma linha).
    pub z80_int: bool,
    /// Ciclos de 68000 parados por DMA, a descontar pelo console.
    pub dma_stall: u32,
    /// Posição na linha atual (0..1), para o contador H.
    pub line_fraction: f32,
}

impl System {
    pub fn new(rom: Vec<u8>, psg: Psg, ym: Ym2612) -> Self {
        let sram = has_sram(&rom).then(|| Box::new([0u8; SRAM_SIZE]));
        let sram_mapped = rom.len() <= SRAM_BASE as usize;
        Self {
            rom,
            ram: Box::new([0; RAM_SIZE]),
            sram,
            sram_mapped,
            vdp: Vdp::new(),
            ports: [Port::new(true), Port::new(false)],
            psg,
            ym,
            z80_ram: Box::new([0; Z80_RAM_SIZE]),
            z80_busreq: false,
            z80_reset: true,
            z80_bank: 0,
            z80_int: false,
            dma_stall: 0,
            line_fraction: 0.0,
        }
    }

    /// O Z80 pode executar (sem reset e sem o 68000 com o barramento).
    pub fn z80_running(&self) -> bool {
        !self.z80_reset && !self.z80_busreq
    }

    fn rom_byte(&self, addr: u32) -> u8 {
        if self.rom.is_empty() {
            return 0xFF;
        }
        self.rom[addr as usize % self.rom.len()]
    }

    fn sram_slot(&self, addr: u32) -> Option<usize> {
        let offset = addr.checked_sub(SRAM_BASE)? as usize;
        (self.sram.is_some() && self.sram_mapped && offset < SRAM_SIZE).then_some(offset)
    }

    // ---- 68000: leitura e escrita de bytes ------------------------------

    fn read8(&mut self, addr: u32) -> u8 {
        match addr {
            0x00_0000..=0x3F_FFFF => match self.sram_slot(addr) {
                Some(i) => self.sram.as_ref().map_or(0xFF, |s| s[i]),
                None => self.rom_byte(addr),
            },
            0xA0_0000..=0xA0_FFFF => self.z80_area_read(addr as u16),
            0xA1_0000..=0xA1_001F => self.io_read(addr),
            0xA1_1100..=0xA1_1101 => {
                // Bit 0 do byte alto: 0 = barramento do Z80 cedido.
                if addr & 1 == 0 {
                    u8::from(!self.z80_busreq)
                } else {
                    0
                }
            }
            0xC0_0000..=0xDF_FFFF => {
                let word = self.vdp_read(addr);
                if addr & 1 == 0 {
                    (word >> 8) as u8
                } else {
                    word as u8
                }
            }
            0xE0_0000..=0xFF_FFFF => self.ram[(addr & 0xFFFF) as usize],
            _ => 0xFF,
        }
    }

    fn write8(&mut self, addr: u32, value: u8) {
        match addr {
            0x00_0000..=0x3F_FFFF => {
                if let Some(i) = self.sram_slot(addr)
                    && let Some(s) = self.sram.as_mut()
                {
                    s[i] = value;
                }
            }
            0xA0_0000..=0xA0_FFFF => self.z80_area_write(addr as u16, value),
            0xA1_0000..=0xA1_001F => self.io_write(addr, value),
            0xA1_1100 => self.z80_busreq = value & 1 != 0,
            0xA1_1200 => self.set_z80_reset(value & 1 == 0),
            0xA1_30F1 => self.sram_mapped = value & 1 != 0 || self.rom.len() <= SRAM_BASE as usize,
            0xC0_0000..=0xDF_FFFF => {
                if addr & 0x1F == 0x11 || addr & 0x1F == 0x13 {
                    self.psg.write(value);
                } else {
                    self.vdp_write(addr, u16::from(value) << 8 | u16::from(value));
                }
            }
            0xE0_0000..=0xFF_FFFF => self.ram[(addr & 0xFFFF) as usize] = value,
            _ => {}
        }
    }

    fn set_z80_reset(&mut self, asserted: bool) {
        if asserted && !self.z80_reset {
            self.ym.reset();
        }
        self.z80_reset = asserted;
    }

    // ---- VDP ------------------------------------------------------------

    fn vdp_read(&mut self, addr: u32) -> u16 {
        match addr & 0x1F {
            0x00..=0x03 => self.vdp.read_data(),
            0x04..=0x07 => self.vdp.read_status(),
            0x08..=0x0F => self.vdp.hv_counter(self.line_fraction),
            _ => 0xFFFF,
        }
    }

    fn vdp_write(&mut self, addr: u32, value: u16) {
        match addr & 0x1F {
            0x00..=0x03 => self.vdp.write_data(value),
            0x04..=0x07 => {
                self.vdp.write_control(value);
                if let Some(request) = self.vdp.take_dma_request() {
                    self.run_dma(request);
                }
            }
            0x10..=0x17 => self.psg.write(value as u8),
            _ => {}
        }
    }

    /// DMA 68000 → VDP: copia agora e registra quanto tempo o 68000 fica parado.
    fn run_dma(&mut self, request: crate::vdp::DmaRequest) {
        let vram = self.vdp.target_is_vram();
        let mut source = request.source;
        for _ in 0..request.words {
            let word = u16::from(self.read8(source)) << 8 | u16::from(self.read8(source | 1));
            self.vdp.dma_write(word);
            // A origem dá a volta dentro do bloco de 128 KB.
            source = (source & 0xFE_0000) | (source.wrapping_add(2) & 0x1_FFFF);
        }
        self.vdp.finish_dma(source);

        let blank = usize::from(self.vdp.in_blank());
        let rate = DMA_RATE[blank][usize::from(self.vdp.h40())];
        let units = request.words * if vram { 2 } else { 1 };
        self.dma_stall += units * MCLK_PER_LINE / rate / MCLK_PER_M68K;
    }

    // ---- I/O ------------------------------------------------------------

    fn io_read(&self, addr: u32) -> u8 {
        match addr & 0x1F {
            0x00 | 0x01 => VERSION,
            0x02 | 0x03 => self.ports[0].read_data(),
            0x04 | 0x05 => self.ports[1].read_data(),
            0x08 | 0x09 => self.ports[0].control(),
            0x0A | 0x0B => self.ports[1].control(),
            0x06 | 0x07 => 0x7F,
            _ => 0,
        }
    }

    fn io_write(&mut self, addr: u32, value: u8) {
        match addr & 0x1F {
            0x02 | 0x03 => self.ports[0].write_data(value),
            0x04 | 0x05 => self.ports[1].write_data(value),
            0x08 | 0x09 => self.ports[0].write_control(value),
            0x0A | 0x0B => self.ports[1].write_control(value),
            _ => {}
        }
    }

    // ---- Espaço do Z80 (visto pelos dois processadores) ------------------

    fn z80_area_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.z80_ram[usize::from(addr) % Z80_RAM_SIZE],
            0x4000..=0x5FFF => self.ym.read_status(),
            _ => 0xFF,
        }
    }

    fn z80_area_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x3FFF => self.z80_ram[usize::from(addr) % Z80_RAM_SIZE] = value,
            0x4000..=0x5FFF => self.ym.write(addr & 3, value),
            0x6000..=0x60FF => {
                // Registrador de banco serial: um bit por escrita, 9 bits.
                self.z80_bank = ((self.z80_bank >> 1) | (u32::from(value & 1) << 8)) & 0x1FF;
            }
            0x7F11 | 0x7F13 | 0x7F15 | 0x7F17 => self.psg.write(value),
            _ => {}
        }
    }

    // ---- Estado salvo -----------------------------------------------------

    pub fn save_state(&self, w: &mut Writer) {
        w.slice(self.ram.as_slice());
        w.bool(self.sram.is_some());
        if let Some(s) = &self.sram {
            w.slice(s.as_slice());
        }
        w.bool(self.sram_mapped);
        w.slice(self.z80_ram.as_slice());
        w.bool(self.z80_busreq);
        w.bool(self.z80_reset);
        w.u32(self.z80_bank);
        w.bool(self.z80_int);
        self.vdp.save_state(w);
        for p in &self.ports {
            p.save_state(w);
        }
        self.psg.save_state(w);
        self.ym.save_state(w);
    }

    pub fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        r.fill(self.ram.as_mut_slice())?;
        if r.bool()? != self.sram.is_some() {
            return Err(StateError::Invalid("SRAM"));
        }
        if let Some(s) = self.sram.as_mut() {
            r.fill(s.as_mut_slice())?;
        }
        self.sram_mapped = r.bool()?;
        r.fill(self.z80_ram.as_mut_slice())?;
        self.z80_busreq = r.bool()?;
        self.z80_reset = r.bool()?;
        self.z80_bank = r.u32()? & 0x1FF;
        self.z80_int = r.bool()?;
        self.vdp.load_state(r)?;
        for p in &mut self.ports {
            p.load_state(r)?;
        }
        self.psg.load_state(r)?;
        self.ym.load_state(r)?;
        self.dma_stall = 0;
        Ok(())
    }
}

impl System {
    /// RAM do cartucho com bateria, se houver.
    pub fn sram(&self) -> Option<&[u8]> {
        self.sram.as_deref().map(|s| &s[..])
    }

    /// Carrega a RAM do cartucho; recusa (`false`) sem SRAM ou com outro
    /// tamanho.
    pub fn load_sram(&mut self, bytes: &[u8]) -> bool {
        match self.sram.as_mut() {
            Some(s) if bytes.len() == s.len() => {
                s.copy_from_slice(bytes);
                true
            }
            _ => false,
        }
    }
}

/// Cabeçalho "RA" em 0x1B0 indica RAM de cartucho com bateria. ROMs acima
/// de 2 MB (ex.: Sonic 3 & Knuckles) podem ligá-la por A130F1 sem declarar.
fn has_sram(rom: &[u8]) -> bool {
    rom.get(0x1B0..0x1B2) == Some(b"RA") || rom.len() > SRAM_BASE as usize
}

impl m68k::Bus for System {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.read8(addr)
    }

    fn read_word(&mut self, addr: u32) -> u16 {
        match addr {
            0xC0_0000..=0xDF_FFFF => self.vdp_read(addr),
            // Área do Z80: o byte aparece nas duas metades.
            0xA0_0000..=0xA0_FFFF => {
                let b = self.read8(addr);
                u16::from(b) << 8 | u16::from(b)
            }
            0xA1_1100 => u16::from(!self.z80_busreq) << 8,
            _ => u16::from(self.read8(addr)) << 8 | u16::from(self.read8(addr | 1)),
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        self.write8(addr, value);
    }

    fn write_word(&mut self, addr: u32, value: u16) {
        match addr {
            0xC0_0000..=0xDF_FFFF => {
                if addr & 0x1F >= 0x10 {
                    self.psg.write(value as u8);
                } else {
                    self.vdp_write(addr, value);
                }
            }
            // Palavra na área do Z80, BUSREQ e RESET: vale o byte alto.
            0xA0_0000..=0xA0_FFFF | 0xA1_1100 | 0xA1_1200 => self.write8(addr, (value >> 8) as u8),
            _ => {
                self.write8(addr, (value >> 8) as u8);
                self.write8(addr | 1, value as u8);
            }
        }
    }

    fn interrupt_level(&self) -> u8 {
        self.vdp.interrupt_level()
    }

    fn acknowledge_interrupt(&mut self, level: u8) {
        self.vdp.acknowledge(level);
    }
}

/// Visão do barramento pelo Z80 de som.
pub(crate) struct Z80Bus<'a>(pub &'a mut System);

impl z80::Bus for Z80Bus<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => {
                if (0x7F00..=0x7F1F).contains(&addr) {
                    return 0xFF;
                }
                self.0.z80_area_read(addr)
            }
            _ => {
                let target = (self.0.z80_bank << 15) | u32::from(addr & 0x7FFF);
                self.0.read8(target)
            }
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF => self.0.z80_area_write(addr, value),
            _ => {
                let target = (self.0.z80_bank << 15) | u32::from(addr & 0x7FFF);
                self.0.write8(target, value);
            }
        }
    }

    fn input(&mut self, _port: u16) -> u8 {
        0xFF
    }

    fn output(&mut self, _port: u16, _value: u8) {}

    fn int_line(&self) -> bool {
        self.0.z80_int
    }
}

#[cfg(test)]
mod tests;
