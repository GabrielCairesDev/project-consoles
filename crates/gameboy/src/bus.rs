//! Mapa de memória e passagem do tempo.
//!
//! Cada acesso da CPU custa um M-ciclo (4 T): `read`/`write` primeiro
//! avançam timer, PPU, serial e DMA, e depois fazem o acesso. Assim o
//! tempo de cada instrução sai dos próprios acessos, como no hardware.

use chips::state::{Reader, StateError, Writer};

use crate::cart::Cart;
use crate::joypad::Joypad;
use crate::ppu::Ppu;
use crate::timer::Timer;

/// Bits de IE/IF.
pub const INT_VBLANK: u8 = 0x01;
pub const INT_STAT: u8 = 0x02;
pub const INT_TIMER: u8 = 0x04;
pub const INT_SERIAL: u8 = 0x08;
pub const INT_JOYPAD: u8 = 0x10;

/// Ciclos (T) por bit da serial com relógio interno (8192 Hz).
const SERIAL_BIT_CYCLES: u32 = 512;
const DMA_LENGTH: u8 = 160;

#[derive(Debug, Clone, Default)]
struct Serial {
    data: u8,
    control: u8,
    /// Ciclos até o fim da transferência em andamento.
    remaining: u32,
    /// Bytes enviados (para ler o resultado de ROMs de teste).
    output: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
struct Dma {
    /// Byte alto do endereço de origem da cópia em andamento.
    source: Option<u8>,
    index: u8,
    /// Escrita em FF46 esperando o próximo ciclo para começar.
    requested: Option<u8>,
    /// Ciclo de preparação: começa a copiar no próximo.
    starting: Option<u8>,
    /// Último valor escrito em FF46 (lido de volta).
    register: u8,
    /// Último byte copiado: é o que a CPU lê se disputar o mesmo barramento.
    last: u8,
}

/// Barramentos do DMG: o externo (cartucho e WRAM) e o de vídeo (VRAM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    External,
    Video,
    /// OAM, I/O e HRAM: dentro do chip.
    Internal,
}

fn lane(addr: u16) -> Lane {
    match addr {
        0x8000..=0x9FFF => Lane::Video,
        0xFE00..=0xFFFF => Lane::Internal,
        _ => Lane::External,
    }
}

#[derive(Debug, Clone)]
pub struct Bus {
    pub cart: Cart,
    pub ppu: Ppu,
    pub timer: Timer,
    pub joypad: Joypad,
    wram: Box<[u8; 0x2000]>,
    hram: [u8; 0x7F],
    /// Registradores de som ainda sem a APU (só guardados).
    sound: [u8; 0x30],
    serial: Serial,
    dma: Dma,
    pub interrupt_flag: u8,
    pub interrupt_enable: u8,
    /// Ciclos (T) desde a ligação.
    pub cycles: u64,
}

impl Bus {
    pub fn new(cart: Cart) -> Self {
        Self {
            cart,
            ppu: Ppu::default(),
            timer: Timer::default(),
            joypad: Joypad::default(),
            wram: Box::new([0; 0x2000]),
            hram: [0; 0x7F],
            sound: [0; 0x30],
            serial: Serial::default(),
            dma: Dma::default(),
            interrupt_flag: 0x01,
            interrupt_enable: 0,
            cycles: 0,
        }
    }

    /// Interrupções habilitadas e pendentes.
    pub fn pending(&self) -> u8 {
        self.interrupt_enable & self.interrupt_flag & 0x1F
    }

    /// Avança um M-ciclo sem acesso à memória.
    pub fn idle(&mut self) {
        self.cycles += 4;
        if self.timer.tick() {
            self.interrupt_flag |= INT_TIMER;
        }
        let ppu = self.ppu.tick();
        if ppu.vblank {
            self.interrupt_flag |= INT_VBLANK;
        }
        if ppu.stat {
            self.interrupt_flag |= INT_STAT;
        }
        self.cart.tick(4);
        self.tick_serial();
        self.tick_dma();
    }

    fn tick_serial(&mut self) {
        if self.serial.remaining == 0 {
            return;
        }
        self.serial.remaining = self.serial.remaining.saturating_sub(4);
        if self.serial.remaining == 0 {
            // Sem cabo: o que chega é 0xFF.
            self.serial.data = 0xFF;
            self.serial.control &= 0x7F;
            self.interrupt_flag |= INT_SERIAL;
        }
    }

    /// Escrita em FF46 no ciclo W: W+1 é a preparação; os 160 bytes são
    /// copiados de W+2 a W+161, e o barramento fica ocupado nesses ciclos.
    fn tick_dma(&mut self) {
        if self.dma.index == DMA_LENGTH {
            self.dma.source = None;
            self.dma.index = 0;
        }
        if let Some(source) = self.dma.starting.take() {
            self.dma.source = Some(source);
            self.dma.index = 0;
        }
        self.dma.starting = self.dma.requested.take();
        if let Some(source) = self.dma.source {
            let addr = u16::from(source) << 8 | u16::from(self.dma.index);
            let value = self.dma_read(addr);
            self.dma.last = value;
            self.ppu.oam[usize::from(self.dma.index)] = value;
            self.dma.index += 1;
        }
    }

    /// Origem do DMA: de E000 em diante vê a WRAM (o DMA não enxerga OAM/I/O).
    fn dma_read(&self, addr: u16) -> u8 {
        match addr {
            0xE000..=0xFFFF => self.wram[usize::from(addr - 0xE000) & 0x1FFF],
            _ => self.peek(addr),
        }
    }

    fn dma_active(&self) -> bool {
        self.dma.source.is_some()
    }

    /// Durante o DMA, a CPU que usa o mesmo barramento da origem recebe o
    /// byte que o DMA está copiando (e suas escritas se perdem).
    fn dma_conflict(&self, addr: u16) -> bool {
        match self.dma.source {
            Some(source) => {
                let source = lane(u16::from(source) << 8);
                let target = lane(addr);
                target != Lane::Internal && target == source
            }
            None => false,
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        self.idle();
        if self.dma_conflict(addr) {
            return self.dma.last;
        }
        self.peek(addr)
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        self.idle();
        if self.dma_conflict(addr) {
            return;
        }
        self.poke(addr, value);
    }

    /// Leitura sem passar tempo nem restrição de DMA.
    pub fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => {
                if self.ppu.vram_accessible() {
                    self.ppu.vram[usize::from(addr - 0x8000)]
                } else {
                    0xFF
                }
            }
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[usize::from(addr - 0xC000) & 0x1FFF],
            0xFE00..=0xFE9F => {
                if self.ppu.oam_accessible() && !self.dma_active() {
                    self.ppu.oam[usize::from(addr - 0xFE00)]
                } else {
                    0xFF
                }
            }
            0xFEA0..=0xFEFF => 0x00,
            0xFF00 => self.joypad.read(),
            0xFF01 => self.serial.data,
            0xFF02 => self.serial.control | 0x7E,
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF0F => self.interrupt_flag | 0xE0,
            0xFF10..=0xFF3F => self.sound[usize::from(addr - 0xFF10)],
            0xFF46 => self.dma.register,
            0xFF40..=0xFF4B => self.ppu.read(addr),
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)],
            0xFFFF => self.interrupt_enable,
            _ => 0xFF,
        }
    }

    fn poke(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, value),
            0x8000..=0x9FFF => {
                if self.ppu.vram_accessible() {
                    self.ppu.vram[usize::from(addr - 0x8000)] = value;
                }
            }
            0xA000..=0xBFFF => self.cart.write_ram(addr, value),
            0xC000..=0xFDFF => self.wram[usize::from(addr - 0xC000) & 0x1FFF] = value,
            0xFE00..=0xFE9F => {
                if self.ppu.oam_accessible() && !self.dma_active() {
                    self.ppu.oam[usize::from(addr - 0xFE00)] = value;
                }
            }
            0xFF00 => {
                if self.joypad.write(value) {
                    self.interrupt_flag |= INT_JOYPAD;
                }
            }
            0xFF01 => self.serial.data = value,
            0xFF02 => {
                self.serial.control = value & 0x81;
                if value & 0x81 == 0x81 {
                    self.serial.output.push(self.serial.data);
                    self.serial.remaining = 8 * SERIAL_BIT_CYCLES;
                }
            }
            0xFF04..=0xFF07 => self.timer.write(addr, value),
            0xFF0F => self.interrupt_flag = value & 0x1F,
            0xFF10..=0xFF3F => self.sound[usize::from(addr - 0xFF10)] = value,
            0xFF46 => {
                self.dma.register = value;
                self.dma.requested = Some(value);
            }
            0xFF40..=0xFF4B => {
                let irq = self.ppu.write(addr, value);
                if irq.stat {
                    self.interrupt_flag |= INT_STAT;
                }
            }
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)] = value,
            0xFFFF => self.interrupt_enable = value,
            _ => {}
        }
    }

    /// Bytes enviados pela serial desde a ligação.
    pub fn serial_output(&self) -> &[u8] {
        &self.serial.output
    }

    pub fn save_state(&self, w: &mut Writer) {
        self.cart.save_state(w);
        self.ppu.save_state(w);
        self.timer.save_state(w);
        self.joypad.save_state(w);
        w.slice(&self.wram[..]);
        w.slice(&self.hram);
        w.slice(&self.sound);
        w.u8(self.serial.data);
        w.u8(self.serial.control);
        w.u32(self.serial.remaining);
        w.bool(self.dma.source.is_some());
        w.u8(self.dma.source.unwrap_or(0));
        w.u8(self.dma.index);
        for slot in [self.dma.requested, self.dma.starting] {
            w.bool(slot.is_some());
            w.u8(slot.unwrap_or(0));
        }
        w.u8(self.dma.register);
        w.u8(self.dma.last);
        w.u8(self.interrupt_flag);
        w.u8(self.interrupt_enable);
        w.u64(self.cycles);
    }

    pub fn load_state(&mut self, r: &mut Reader) -> Result<(), StateError> {
        let mut next = self.clone();
        next.cart.load_state(r)?;
        next.ppu.load_state(r)?;
        next.timer.load_state(r)?;
        next.joypad.load_state(r)?;
        r.fill(&mut next.wram[..])?;
        r.fill(&mut next.hram)?;
        r.fill(&mut next.sound)?;
        next.serial.data = r.u8()?;
        next.serial.control = r.u8()? & 0x81;
        next.serial.remaining = r.u32()?;
        let active = r.bool()?;
        let source = r.u8()?;
        next.dma.source = active.then_some(source);
        next.dma.index = r.u8()?;
        let requested = r.bool()?;
        let request = r.u8()?;
        next.dma.requested = requested.then_some(request);
        let starting = r.bool()?;
        let start = r.u8()?;
        next.dma.starting = starting.then_some(start);
        next.dma.register = r.u8()?;
        next.dma.last = r.u8()?;
        next.interrupt_flag = r.u8()? & 0x1F;
        next.interrupt_enable = r.u8()?;
        next.cycles = r.u64()?;
        if next.dma.index > DMA_LENGTH || next.serial.remaining > 8 * SERIAL_BIT_CYCLES {
            return Err(StateError::Invalid("barramento"));
        }
        *self = next;
        Ok(())
    }
}
