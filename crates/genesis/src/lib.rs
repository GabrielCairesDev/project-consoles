//! Núcleo do Mega Drive / Genesis em safe Rust.
//!
//! [`Console`] liga o 68000 ([`m68k`]), o VDP, o Z80 de som, o PSG e as
//! portas de controle. A API é síncrona e determinística, como a do
//! núcleo do Master System.
//!
//! Escopo atual: NTSC, região exportação, 224 linhas, H32/H40, SRAM em
//! 200000, som estéreo (YM2612 + PSG). Fora: Sega CD, 32X, PAL, modo
//! entrelaçado e SSG-EG do YM2612.

mod bus;
pub mod io;
pub mod m68k;
pub mod vdp;
pub mod ym2612;

use std::fmt;

use bus::{MCLK_PER_LINE, MCLK_PER_M68K, System, Z80Bus};
use chips::psg::Psg;
use chips::state::{self, StateError};
use chips::z80::Z80;
use ym2612::Ym2612;

pub use io::Pad;
pub use vdp::{HEIGHT as SCREEN_HEIGHT, MAX_WIDTH as SCREEN_MAX_WIDTH};

/// Master clock NTSC (Hz).
pub const MASTER_CLOCK: u32 = 53_693_175;
/// O Z80 e o PSG rodam a master clock / 15.
const MCLK_PER_Z80: u32 = 15;
const Z80_CLOCK: u32 = MASTER_CLOCK / MCLK_PER_Z80;
/// Quadros por segundo no NTSC (≈ 59,92).
pub const FRAME_RATE: f64 =
    MASTER_CLOCK as f64 / (MCLK_PER_LINE as f64 * vdp::LINES_PER_FRAME as f64);
/// Parte da linha antes do desenho (H-blank), em master clocks.
const HBLANK_MCLK: u32 = 600;
/// Volume do PSG em relação ao FM na mistura.
const PSG_GAIN: f32 = 0.35;
/// Linha em que o V-int acontece (início do V-blank).
const VBLANK_LINE: u16 = vdp::HEIGHT as u16;

/// Maior ROM aceita: 4 MB de espaço de cartucho, com folga para mappers.
const MAX_ROM_SIZE: usize = 8 * 1024 * 1024;
const SMD_HEADER: usize = 512;
const SMD_BLOCK: usize = 16 * 1024;

const STATE_MAGIC: &[u8; 8] = b"PCMDST\0\0";
const STATE_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RomError {
    Empty,
    TooLarge(usize),
}

impl fmt::Display for RomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a ROM está vazia"),
            Self::TooLarge(size) => {
                write!(f, "a ROM tem {size} bytes; o limite é {MAX_ROM_SIZE} bytes")
            }
        }
    }
}

impl std::error::Error for RomError {}

#[derive(Debug, Clone)]
pub struct Console {
    cpu: m68k::Cpu,
    z80: Z80,
    bus: System,
    /// Saldo de master clocks de cada processador (negativo = adiantado).
    m68k_clock: i64,
    z80_clock: i64,
    /// Estado anterior do RESET do Z80, para reiniciá-lo na borda.
    z80_was_reset: bool,
    rom_hash: u64,
    /// Amostras ainda não misturadas (PSG mono, FM estéreo); não vão para
    /// o estado salvo.
    psg_pending: Vec<f32>,
    fm_pending: Vec<f32>,
}

impl Console {
    /// Liga o console com a ROM e gera áudio na taxa `sample_rate` (Hz).
    pub fn new(rom: Vec<u8>, sample_rate: u32) -> Result<Self, RomError> {
        let rom = normalize_rom(rom)?;
        let rom_hash = state::rom_hash(&rom);
        let mut bus = System::new(
            rom,
            Psg::new(Z80_CLOCK, sample_rate),
            Ym2612::new(sample_rate),
        );
        let mut cpu = m68k::Cpu::new();
        cpu.reset(&mut bus);
        Ok(Self {
            cpu,
            z80: Z80::new(),
            bus,
            m68k_clock: 0,
            z80_clock: 0,
            z80_was_reset: true,
            rom_hash,
            psg_pending: Vec::new(),
            fm_pending: Vec::new(),
        })
    }

    /// Executa um quadro completo (262 linhas).
    pub fn run_frame(&mut self) {
        for line in 0..vdp::LINES_PER_FRAME {
            self.bus.vdp.begin_line(line);
            self.run_slice(0, HBLANK_MCLK);
            self.bus.vdp.render_line();
            if line == VBLANK_LINE {
                self.bus.z80_int = true;
            } else if line == VBLANK_LINE + 1 {
                self.bus.z80_int = false;
            }
            self.run_slice(HBLANK_MCLK, MCLK_PER_LINE);
        }
    }

    /// Roda os dois processadores do trecho `[from, to)` da linha.
    fn run_slice(&mut self, from: u32, to: u32) {
        let span = i64::from(to - from);
        self.m68k_clock += span;
        while self.m68k_clock > 0 {
            let done = i64::from(to) - self.m68k_clock;
            self.bus.line_fraction = (done.max(0) as f32 / MCLK_PER_LINE as f32).min(1.0);
            let cycles = self.cpu.step(&mut self.bus) + std::mem::take(&mut self.bus.dma_stall);
            for port in &mut self.bus.ports {
                port.tick(cycles);
            }
            self.m68k_clock -= i64::from(cycles * MCLK_PER_M68K);
        }

        if self.bus.z80_reset {
            if !self.z80_was_reset {
                self.z80 = Z80::new();
            }
            self.z80_was_reset = true;
        } else if self.z80_was_reset {
            self.z80 = Z80::new();
            self.z80_was_reset = false;
        }
        self.z80_clock += span;
        let mut psg_cycles = 0u32;
        while self.z80_clock > 0 {
            let cycles = if self.bus.z80_running() {
                self.z80.step(&mut Z80Bus(&mut self.bus))
            } else {
                (self.z80_clock as u32).div_ceil(MCLK_PER_Z80)
            };
            psg_cycles += cycles;
            // O YM2612 avança junto com o Z80, para ouvir cada escrita no DAC.
            self.bus.ym.run(cycles * MCLK_PER_Z80);
            self.z80_clock -= i64::from(cycles * MCLK_PER_Z80);
        }
        self.bus.psg.run(psg_cycles);
    }

    /// Imagem do último quadro, linha a linha com [`SCREEN_MAX_WIDTH`]
    /// pixels por linha (`0x00RRGGBB`); só os primeiros
    /// [`Console::frame_size`]`.0` de cada linha são válidos.
    pub fn framebuffer(&self) -> &[u32] {
        self.bus.vdp.framebuffer()
    }

    /// Largura (256 ou 320) e altura do último quadro.
    pub fn frame_size(&self) -> (usize, usize) {
        self.bus.vdp.frame_size()
    }

    /// Move o áudio gerado para `out`: estéreo intercalado (L, R), FM e PSG
    /// misturados, valores em -1..1 (com o nível DC do PSG).
    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        self.bus.psg.drain(&mut self.psg_pending);
        self.bus.ym.drain(&mut self.fm_pending);
        let frames = self.psg_pending.len().min(self.fm_pending.len() / 2);
        out.reserve(frames * 2);
        for (psg, fm) in self
            .psg_pending
            .iter()
            .zip(self.fm_pending.as_chunks::<2>().0)
            .take(frames)
        {
            let p = psg * PSG_GAIN;
            let [l, r] = *fm;
            out.push((l + p).clamp(-1.0, 1.0));
            out.push((r + p).clamp(-1.0, 1.0));
        }
        self.psg_pending.drain(..frames);
        self.fm_pending.drain(..frames * 2);
    }

    pub fn set_input(&mut self, pad: Pad) {
        self.bus.ports[0].pad = pad;
    }

    /// Controle de 6 botões (padrão) ou de 3.
    pub fn set_six_button(&mut self, enabled: bool) {
        self.bus.ports[0].six_button = enabled;
    }

    /// Registradores do 68000, para depuração.
    pub fn cpu_registers(&self) -> m68k::Registers {
        self.cpu.registers()
    }

    /// Registradores do VDP, para depuração.
    pub fn vdp_registers(&self) -> [u8; 24] {
        self.bus.vdp.registers()
    }

    /// O 68000 parou por falha dupla (erro durante uma exceção).
    pub fn cpu_halted(&self) -> bool {
        self.cpu.halted()
    }

    /// Estado completo da máquina; chamar entre quadros.
    pub fn save_state(&self) -> Vec<u8> {
        let mut w = state::Writer::default();
        w.slice(STATE_MAGIC);
        w.u16(STATE_VERSION);
        w.u64(self.rom_hash);
        self.cpu.save_state(&mut w);
        self.z80.save_state(&mut w);
        self.bus.save_state(&mut w);
        w.u64(self.m68k_clock as u64);
        w.u64(self.z80_clock as u64);
        w.bool(self.z80_was_reset);
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
        next.z80.load_state(&mut r)?;
        next.bus.load_state(&mut r)?;
        next.m68k_clock = r.u64()? as i64;
        next.z80_clock = r.u64()? as i64;
        next.z80_was_reset = r.bool()?;
        if next.m68k_clock.abs() > i64::from(MCLK_PER_LINE) * 4
            || next.z80_clock.abs() > i64::from(MCLK_PER_LINE) * 4
        {
            return Err(StateError::Invalid("relógios"));
        }
        r.finish()?;
        *self = next;
        Ok(())
    }
}

/// Valida o tamanho e converte o formato SMD (intercalado, com cabeçalho).
fn normalize_rom(mut rom: Vec<u8>) -> Result<Vec<u8>, RomError> {
    if rom.len() > MAX_ROM_SIZE {
        return Err(RomError::TooLarge(rom.len()));
    }
    if rom.len() % SMD_BLOCK == SMD_HEADER && rom.get(8..10) == Some(&[0xAA, 0xBB]) {
        let body = rom.split_off(SMD_HEADER);
        rom = deinterleave_smd(&body);
    }
    if rom.is_empty() {
        return Err(RomError::Empty);
    }
    Ok(rom)
}

/// Cada bloco de 16 KB do SMD guarda os bytes ímpares na primeira metade e
/// os pares na segunda.
fn deinterleave_smd(body: &[u8]) -> Vec<u8> {
    let mut out = vec![0; body.len()];
    for (block, dst) in body.chunks(SMD_BLOCK).zip(out.chunks_mut(SMD_BLOCK)) {
        let half = block.len() / 2;
        for i in 0..half {
            dst[i * 2 + 1] = block[i];
            dst[i * 2] = block[half + i];
        }
    }
    out
}

#[cfg(test)]
mod tests;
