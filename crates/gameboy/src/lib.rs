//! Núcleo do Game Boy (DMG) em safe Rust.
//!
//! [`Console`] liga a CPU SM83, a PPU, o timer, o joypad e o cartucho. A
//! API é síncrona e determinística, como a dos outros núcleos.
//!
//! Escopo: Game Boy clássico, sem boot ROM (os registradores começam no
//! estado em que ela os deixa). Fora: Game Boy Color, Super Game Boy, cabo
//! link, STOP de baixo consumo e os MBCs raros (MBC6/7, HuC, câmera).

mod apu;
mod bus;
mod cart;
mod cpu;
mod joypad;
mod ppu;
mod timer;

use chips::state::{self, StateError};

use bus::Bus;
use cpu::Cpu;

pub use cart::CartError;
pub use joypad::Buttons;
pub use ppu::{HEIGHT as SCREEN_HEIGHT, SHADES, WIDTH as SCREEN_WIDTH};

/// Clock da CPU (Hz).
pub const CPU_CLOCK: u32 = 4_194_304;
/// Ciclos (T) por quadro: 154 linhas × 456.
pub const CYCLES_PER_FRAME: u32 = 70_224;
/// Quadros por segundo (≈ 59,73).
pub const FRAME_RATE: f64 = CPU_CLOCK as f64 / CYCLES_PER_FRAME as f64;

/// Maior ROM aceita (MBC5: 512 bancos de 16 KB).
const MAX_ROM_SIZE: usize = 8 * 1024 * 1024;

const STATE_MAGIC: &[u8; 8] = b"PCGBST\0\0";
const STATE_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RomError {
    TooLarge(usize),
    Cart(CartError),
}

impl std::fmt::Display for RomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(size) => {
                write!(f, "a ROM tem {size} bytes; o limite é {MAX_ROM_SIZE} bytes")
            }
            Self::Cart(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for RomError {}

#[derive(Debug, Clone)]
pub struct Console {
    cpu: Cpu,
    bus: Bus,
    /// Saldo de ciclos do quadro (negativo = adiantado).
    budget: i64,
    rom_hash: u64,
}

impl Console {
    /// Liga o console com a ROM e gera áudio na taxa `sample_rate` (Hz).
    pub fn new(rom: Vec<u8>, sample_rate: u32) -> Result<Self, RomError> {
        if rom.len() > MAX_ROM_SIZE {
            return Err(RomError::TooLarge(rom.len()));
        }
        let rom_hash = state::rom_hash(&rom);
        let cart = cart::Cart::new(rom).map_err(RomError::Cart)?;
        Ok(Self {
            cpu: Cpu::default(),
            bus: Bus::new(cart, apu::Apu::new(sample_rate)),
            budget: 0,
            rom_hash,
        })
    }

    /// Executa um quadro (70224 ciclos).
    pub fn run_frame(&mut self) {
        self.budget += i64::from(CYCLES_PER_FRAME);
        let start = self.bus.cycles;
        while (self.bus.cycles - start) < self.budget as u64 {
            self.cpu.step(&mut self.bus);
        }
        self.budget -= (self.bus.cycles - start) as i64;
    }

    /// Imagem do último quadro: 160×144 pixels `0x00RRGGBB`.
    pub fn framebuffer(&self) -> &[u32] {
        self.bus.ppu.frame()
    }

    /// Move o áudio gerado para `out`: estéreo intercalado (L, R), valores
    /// em -1..1 (com o nível DC do hardware; o player o remove).
    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        self.bus.apu.drain(out);
    }

    pub fn set_input(&mut self, buttons: Buttons) {
        if self.bus.joypad.set(buttons) {
            self.bus.interrupt_flag |= bus::INT_JOYPAD;
        }
    }

    /// RAM do cartucho com bateria (o save do próprio jogo), se houver.
    pub fn battery_ram(&self) -> Option<&[u8]> {
        self.bus.cart.has_battery().then(|| self.bus.cart.ram())
    }

    /// Carrega o save do jogo; tamanho errado ou cartucho sem bateria é
    /// recusado (`false`) e nada muda.
    pub fn load_battery_ram(&mut self, bytes: &[u8]) -> bool {
        self.bus.cart.has_battery() && self.bus.cart.load_ram(bytes)
    }

    /// Bytes enviados pela porta serial (usado por ROMs de teste).
    pub fn serial_output(&self) -> &[u8] {
        self.bus.serial_output()
    }

    /// Registradores da CPU (A, F, B, C, D, E, H, L), para depuração e testes.
    pub fn cpu_registers(&self) -> [u8; 8] {
        let c = &self.cpu;
        [c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l]
    }

    /// Lê a memória sem passar tempo, para depuração.
    pub fn peek(&self, addr: u16) -> u8 {
        self.bus.peek(addr)
    }

    /// PC atual, para depuração.
    pub fn pc(&self) -> u16 {
        self.cpu.pc
    }

    /// Estado completo da máquina; chamar entre quadros.
    pub fn save_state(&self) -> Vec<u8> {
        let mut w = state::Writer::default();
        w.slice(STATE_MAGIC);
        w.u16(STATE_VERSION);
        w.u64(self.rom_hash);
        self.cpu.save_state(&mut w);
        self.bus.save_state(&mut w);
        w.u64(self.budget as u64);
        w.bytes
    }

    /// Restaura um estado de [`Console::save_state`] da mesma ROM; em caso
    /// de erro, o console fica exatamente como estava.
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        let mut r = state::Reader::new(bytes);
        let mut magic = [0; 8];
        r.fill(&mut magic).map_err(|_| StateError::NotAState)?;
        if &magic != STATE_MAGIC {
            return Err(StateError::NotAState);
        }
        let version = r.u16()?;
        if version != STATE_VERSION {
            return Err(StateError::UnsupportedVersion(version));
        }
        if r.u64()? != self.rom_hash {
            return Err(StateError::WrongRom);
        }
        let mut next = self.clone();
        next.cpu.load_state(&mut r)?;
        next.bus.load_state(&mut r)?;
        next.budget = r.u64()? as i64;
        if next.budget.abs() > i64::from(CYCLES_PER_FRAME) {
            return Err(StateError::Invalid("relógio"));
        }
        r.finish()?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
