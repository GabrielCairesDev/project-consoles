//! Cartuchos: ROM, RAM externa e os controladores de banco (MBC).
//!
//! O tipo vem do cabeçalho (0x147). Suportados: só ROM (com ou sem RAM),
//! MBC1 (inclusive multicart), MBC2, MBC3 (com relógio) e MBC5.

use std::fmt;

use chips::state::{Reader, StateError, Writer};

const ROM_BANK: usize = 0x4000;
const RAM_BANK: usize = 0x2000;
const HEADER_END: usize = 0x150;
/// RAM de 512 × 4 bits do MBC2.
const MBC2_RAM: usize = 512;
/// Ciclos (T) por segundo, para o relógio do MBC3.
const CLOCK_HZ: u32 = 4_194_304;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartError {
    /// ROM menor que o cabeçalho.
    TooSmall(usize),
    /// Tipo de cartucho (byte 0x147) não suportado.
    UnsupportedType(u8),
}

impl fmt::Display for CartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall(size) => write!(f, "a ROM tem só {size} bytes, menos que o cabeçalho"),
            Self::UnsupportedType(kind) => {
                write!(f, "tipo de cartucho 0x{kind:02X} ainda não suportado")
            }
        }
    }
}

impl std::error::Error for CartError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mapper {
    RomOnly,
    Mbc1 { multicart: bool },
    Mbc2,
    Mbc3,
    Mbc5,
}

/// Relógio do MBC3. Avança com o tempo emulado, não com o relógio do
/// computador: o jogo fica determinístico e o tempo para com o jogo fechado.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Rtc {
    /// Segundos, minutos, horas, dia (8 bits baixos), controle (bit 0 =
    /// dia bit 8, bit 6 = parado, bit 7 = estouro do dia).
    regs: [u8; 5],
    latched: [u8; 5],
    latch_armed: bool,
    /// Ciclos acumulados dentro do segundo atual.
    subsecond: u32,
}

impl Rtc {
    fn tick(&mut self, cycles: u32) {
        if self.regs[4] & 0x40 != 0 {
            return;
        }
        self.subsecond += cycles;
        while self.subsecond >= CLOCK_HZ {
            self.subsecond -= CLOCK_HZ;
            self.advance_second();
        }
    }

    fn advance_second(&mut self) {
        // Contadores com valores fora da faixa seguem até estourar o
        // próprio tamanho, como no hardware.
        self.regs[0] = (self.regs[0] + 1) & 0x3F;
        if self.regs[0] != 60 {
            return;
        }
        self.regs[0] = 0;
        self.regs[1] = (self.regs[1] + 1) & 0x3F;
        if self.regs[1] != 60 {
            return;
        }
        self.regs[1] = 0;
        self.regs[2] = (self.regs[2] + 1) & 0x1F;
        if self.regs[2] != 24 {
            return;
        }
        self.regs[2] = 0;
        let day = (u16::from(self.regs[4] & 1) << 8 | u16::from(self.regs[3])) + 1;
        self.regs[3] = day as u8;
        self.regs[4] = (self.regs[4] & !1) | ((day >> 8) & 1) as u8;
        if day > 0x1FF {
            self.regs[4] |= 0x80;
        }
    }

    fn write_latch(&mut self, value: u8) {
        if value == 0 {
            self.latch_armed = true;
        } else {
            if value == 1 && self.latch_armed {
                self.latched = self.regs;
            }
            self.latch_armed = false;
        }
    }

    fn write(&mut self, reg: usize, value: u8) {
        const MASKS: [u8; 5] = [0x3F, 0x3F, 0x1F, 0xFF, 0xC1];
        self.regs[reg] = value & MASKS[reg];
        if reg == 0 {
            self.subsecond = 0;
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cart {
    rom: Vec<u8>,
    ram: Vec<u8>,
    mapper: Mapper,
    battery: bool,
    rtc: Option<Rtc>,
    ram_enabled: bool,
    /// Registrador de banco baixo da ROM (largura depende do MBC).
    rom_bank: u16,
    /// MBC1: 2 bits altos; MBC3/MBC5: banco da RAM (ou registrador do RTC).
    upper: u8,
    /// MBC1: modo de banco (0 = ROM, 1 = RAM/ROM alta).
    mode: bool,
    rom_banks: usize,
}

impl Cart {
    pub fn new(mut rom: Vec<u8>) -> Result<Self, CartError> {
        if rom.len() < HEADER_END {
            return Err(CartError::TooSmall(rom.len()));
        }
        let kind = rom[0x147];
        let (mapper, has_ram, battery, rtc) = match kind {
            0x00 => (Mapper::RomOnly, false, false, false),
            0x08 => (Mapper::RomOnly, true, false, false),
            0x09 => (Mapper::RomOnly, true, true, false),
            0x01..=0x03 => (
                Mapper::Mbc1 {
                    multicart: is_mbc1_multicart(&rom),
                },
                kind >= 0x02,
                kind == 0x03,
                false,
            ),
            0x05 | 0x06 => (Mapper::Mbc2, true, kind == 0x06, false),
            0x0F => (Mapper::Mbc3, false, true, true),
            0x10 => (Mapper::Mbc3, true, true, true),
            0x11 => (Mapper::Mbc3, false, false, false),
            0x12 => (Mapper::Mbc3, true, false, false),
            0x13 => (Mapper::Mbc3, true, true, false),
            0x19..=0x1E => (
                Mapper::Mbc5,
                matches!(kind, 0x1A | 0x1B | 0x1D | 0x1E),
                matches!(kind, 0x1B | 0x1E),
                false,
            ),
            other => return Err(CartError::UnsupportedType(other)),
        };
        let ram_size = match (mapper, has_ram) {
            (Mapper::Mbc2, _) => MBC2_RAM,
            (_, false) => 0,
            (_, true) => match rom[0x149] {
                0x00 => 0,
                0x01 => 0x800,
                0x02 => RAM_BANK,
                0x03 => 4 * RAM_BANK,
                0x04 => 16 * RAM_BANK,
                0x05 => 8 * RAM_BANK,
                _ => RAM_BANK,
            },
        };
        // Bancos em potência de dois; a ROM é completada com 0xFF.
        let rom_banks = rom.len().div_ceil(ROM_BANK).next_power_of_two().max(2);
        rom.resize(rom_banks * ROM_BANK, 0xFF);
        Ok(Self {
            rom,
            ram: vec![0; ram_size],
            mapper,
            battery,
            rtc: rtc.then(Rtc::default),
            ram_enabled: false,
            rom_bank: 1,
            upper: 0,
            mode: false,
            rom_banks,
        })
    }

    /// O cartucho guarda a RAM com bateria (o jogo salva nela).
    pub fn has_battery(&self) -> bool {
        self.battery && !self.ram.is_empty()
    }

    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    /// Carrega a RAM salva; tamanho diferente é recusado (`false`).
    pub fn load_ram(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() != self.ram.len() {
            return false;
        }
        self.ram.copy_from_slice(bytes);
        true
    }

    /// Avança o relógio do cartucho (só o MBC3 com RTC tem).
    pub fn tick(&mut self, cycles: u32) {
        if let Some(rtc) = &mut self.rtc {
            rtc.tick(cycles);
        }
    }

    fn rom_at(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank & (self.rom_banks - 1);
        self.rom[bank * ROM_BANK + (addr as usize & (ROM_BANK - 1))]
    }

    /// Bits do MBC1 que vão para o banco: 5 normalmente, 4 no multicart.
    fn mbc1_shift(multicart: bool) -> u32 {
        if multicart { 4 } else { 5 }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        let high = addr >= 0x4000;
        let bank = match self.mapper {
            Mapper::RomOnly => usize::from(high),
            Mapper::Mbc1 { multicart } => {
                let shift = Self::mbc1_shift(multicart);
                let upper = usize::from(self.upper) << shift;
                if high {
                    let low = usize::from(self.rom_bank) & ((1 << shift) - 1);
                    upper | low
                } else if self.mode {
                    upper
                } else {
                    0
                }
            }
            Mapper::Mbc2 | Mapper::Mbc3 | Mapper::Mbc5 => {
                if high {
                    usize::from(self.rom_bank)
                } else {
                    0
                }
            }
        };
        self.rom_at(bank, addr)
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match self.mapper {
            Mapper::RomOnly => {}
            Mapper::Mbc1 { .. } => match addr {
                0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    // O banco 0 vira 1 olhando os 5 bits completos.
                    let bank = value & 0x1F;
                    self.rom_bank = u16::from(bank.max(1));
                }
                0x4000..=0x5FFF => self.upper = value & 0x03,
                _ => self.mode = value & 1 != 0,
            },
            Mapper::Mbc2 => {
                if addr < 0x4000 {
                    if addr & 0x0100 == 0 {
                        self.ram_enabled = value & 0x0F == 0x0A;
                    } else {
                        self.rom_bank = u16::from((value & 0x0F).max(1));
                    }
                }
            }
            Mapper::Mbc3 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
                0x2000..=0x3FFF => self.rom_bank = u16::from((value & 0x7F).max(1)),
                0x4000..=0x5FFF => self.upper = value & 0x0F,
                _ => {
                    if let Some(rtc) = &mut self.rtc {
                        rtc.write_latch(value);
                    }
                }
            },
            Mapper::Mbc5 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
                0x2000..=0x2FFF => self.rom_bank = (self.rom_bank & 0x100) | u16::from(value),
                0x3000..=0x3FFF => {
                    self.rom_bank = (self.rom_bank & 0xFF) | (u16::from(value & 1) << 8);
                }
                0x4000..=0x5FFF => self.upper = value & 0x0F,
                _ => {}
            },
        }
    }

    /// Índice na RAM para `addr` (A000–BFFF), se a RAM está acessível.
    fn ram_index(&self, addr: u16) -> Option<usize> {
        if !self.ram_enabled && self.mapper != Mapper::RomOnly || self.ram.is_empty() {
            return None;
        }
        let offset = usize::from(addr - 0xA000);
        let bank = match self.mapper {
            Mapper::RomOnly | Mapper::Mbc2 => 0,
            Mapper::Mbc1 { .. } => {
                if self.mode {
                    usize::from(self.upper)
                } else {
                    0
                }
            }
            Mapper::Mbc3 | Mapper::Mbc5 => usize::from(self.upper),
        };
        let index = match self.mapper {
            Mapper::Mbc2 => offset % MBC2_RAM,
            _ => bank * RAM_BANK + offset,
        };
        Some(index % self.ram.len())
    }

    fn rtc_register(&self) -> Option<usize> {
        let reg = usize::from(self.upper);
        (self.mapper == Mapper::Mbc3 && self.rtc.is_some() && (0x08..=0x0C).contains(&reg))
            .then(|| reg - 0x08)
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if let Some(reg) = self.rtc_register() {
            return match (&self.rtc, self.ram_enabled) {
                (Some(rtc), true) => rtc.latched[reg],
                _ => 0xFF,
            };
        }
        match self.ram_index(addr) {
            Some(i) if self.mapper == Mapper::Mbc2 => self.ram[i] | 0xF0,
            Some(i) => self.ram[i],
            None => 0xFF,
        }
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if let Some(reg) = self.rtc_register() {
            if self.ram_enabled
                && let Some(rtc) = &mut self.rtc
            {
                rtc.write(reg, value);
            }
            return;
        }
        if let Some(i) = self.ram_index(addr) {
            self.ram[i] = if self.mapper == Mapper::Mbc2 {
                value & 0x0F
            } else {
                value
            };
        }
    }

    pub fn save_state(&self, w: &mut Writer) {
        w.u32(self.ram.len() as u32);
        w.slice(&self.ram);
        w.bool(self.ram_enabled);
        w.u16(self.rom_bank);
        w.u8(self.upper);
        w.bool(self.mode);
        w.bool(self.rtc.is_some());
        if let Some(rtc) = &self.rtc {
            w.slice(&rtc.regs);
            w.slice(&rtc.latched);
            w.bool(rtc.latch_armed);
            w.u32(rtc.subsecond);
        }
    }

    pub fn load_state(&mut self, r: &mut Reader) -> Result<(), StateError> {
        if r.u32()? as usize != self.ram.len() {
            return Err(StateError::Invalid("RAM do cartucho"));
        }
        let mut ram = vec![0; self.ram.len()];
        r.fill(&mut ram)?;
        let ram_enabled = r.bool()?;
        let rom_bank = r.u16()?;
        let upper = r.u8()?;
        let mode = r.bool()?;
        if r.bool()? != self.rtc.is_some() {
            return Err(StateError::Invalid("relógio do cartucho"));
        }
        let rtc = match self.rtc {
            Some(_) => {
                let mut rtc = Rtc::default();
                r.fill(&mut rtc.regs)?;
                r.fill(&mut rtc.latched)?;
                rtc.latch_armed = r.bool()?;
                rtc.subsecond = r.u32()?;
                if rtc.subsecond >= CLOCK_HZ {
                    return Err(StateError::Invalid("relógio do cartucho"));
                }
                Some(rtc)
            }
            None => None,
        };
        self.ram = ram;
        self.ram_enabled = ram_enabled;
        self.rom_bank = rom_bank;
        self.upper = upper;
        self.mode = mode;
        self.rtc = rtc;
        Ok(())
    }
}

/// Multicart MBC1 (ex.: coletâneas de 1 MB): o logo da Nintendo se repete
/// no começo do segundo jogo, no banco 0x10.
fn is_mbc1_multicart(rom: &[u8]) -> bool {
    const LOGO: std::ops::Range<usize> = 0x104..0x134;
    rom.len() == 64 * ROM_BANK
        && rom.get(0x10 * ROM_BANK + LOGO.start..0x10 * ROM_BANK + LOGO.end) == rom.get(LOGO)
}

#[cfg(test)]
mod tests;
