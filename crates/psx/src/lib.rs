//! Núcleo do PlayStation (PS1) em safe Rust.
//!
//! [`Console`] liga a CPU R3000A, o barramento e os dispositivos. A API é
//! síncrona e determinística, como a dos outros núcleos.
//!
//! O BIOS é do usuário: o console não liga sem ele (o da Sony é
//! proprietário e não vem junto).
//!
//! Estado atual: CPU, GTE, memória, interrupções, timers, DMA, a GPU, o
//! SPU, o controle digital, o memory card na porta 1, o CD-ROM (imagens
//! `.cue`/`.bin`, com o som CD-DA e XA-ADPCM mixado no SPU) e o MDEC (os
//! vídeos).

mod bus;
mod cdrom;
mod cpu;
mod disc;
mod dma;
mod gpu;
mod gte;
mod irq;
mod mdec;
mod sio;
mod spu;
mod timers;

use bus::{BIOS_SIZE, Bus, RAM_SIZE};
use cpu::Cpu;
pub use disc::{Disc, DiscError};
pub use sio::{Buttons, MEMORY_CARD_SIZE, MemoryCardError};

/// Clock da CPU (Hz): 44100 × 768.
pub const CPU_CLOCK: u32 = 33_868_800;

/// Taxa do áudio gerado pelo SPU (Hz).
pub const SAMPLE_RATE: u32 = 44_100;

/// Ponto em que o BIOS entra no shell; um PS-EXE de teste é carregado aqui.
const SHELL_ENTRY: u32 = 0x8003_0000;
/// Limite da saída de texto guardada.
const MAX_TTY: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BiosError {
    /// O arquivo não tem o tamanho de um BIOS (512 KB).
    WrongSize(usize),
}

impl std::fmt::Display for BiosError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongSize(size) => write!(
                f,
                "o arquivo tem {size} bytes; um BIOS do PlayStation tem {BIOS_SIZE}"
            ),
        }
    }
}

impl std::error::Error for BiosError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExeError {
    /// Não começa com "PS-X EXE".
    NotAnExe,
    /// O programa não cabe na RAM.
    OutOfRange,
}

impl std::fmt::Display for ExeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnExe => f.write_str("o arquivo não é um PS-EXE"),
            Self::OutOfRange => f.write_str("o PS-EXE não cabe na RAM"),
        }
    }
}

impl std::error::Error for ExeError {}

/// Programa PS-EXE validado.
#[derive(Debug, Clone)]
struct Exe {
    pc: u32,
    gp: u32,
    load_address: u32,
    text: Vec<u8>,
    bss: (u32, u32),
    stack: Option<u32>,
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

impl Exe {
    fn parse(bytes: &[u8]) -> Result<Self, ExeError> {
        if bytes.len() < 0x800 || &bytes[..8] != b"PS-X EXE" {
            return Err(ExeError::NotAnExe);
        }
        let load_address = word(bytes, 0x18);
        let size = word(bytes, 0x1C) as usize;
        let text = bytes.get(0x800..0x800 + size).ok_or(ExeError::NotAnExe)?;
        let fits = |addr: u32, len: usize| (addr as usize & 0x1F_FFFF) + len <= RAM_SIZE;
        let bss = (word(bytes, 0x28), word(bytes, 0x2C));
        if !fits(load_address, size) || !fits(bss.0, bss.1 as usize) {
            return Err(ExeError::OutOfRange);
        }
        let stack_base = word(bytes, 0x30);
        Ok(Self {
            pc: word(bytes, 0x10),
            gp: word(bytes, 0x14),
            load_address,
            text: text.to_vec(),
            bss,
            stack: (stack_base != 0).then(|| stack_base.wrapping_add(word(bytes, 0x34))),
        })
    }
}

#[derive(Clone)]
pub struct Console {
    cpu: Cpu,
    bus: Bus,
    /// Programa a carregar quando o BIOS chegar ao shell.
    pending_exe: Option<Exe>,
    tty: Vec<u8>,
}

impl Console {
    /// Liga o console com o BIOS do usuário.
    pub fn new(bios: &[u8]) -> Result<Self, BiosError> {
        let bios: Box<[u8; BIOS_SIZE]> = bios
            .to_vec()
            .into_boxed_slice()
            .try_into()
            .map_err(|_| BiosError::WrongSize(bios.len()))?;
        Ok(Self {
            cpu: Cpu::default(),
            bus: Bus::new(bios),
            pending_exe: None,
            tty: Vec::new(),
        })
    }

    /// Roda o PS-EXE no lugar do shell do BIOS (o BIOS inicia o kernel
    /// antes, como no console). Usado pelos programas de teste.
    pub fn load_exe(&mut self, exe: &[u8]) -> Result<(), ExeError> {
        self.pending_exe = Some(Exe::parse(exe)?);
        Ok(())
    }

    /// Executa até o próximo V-blank (um quadro).
    pub fn run_frame(&mut self) {
        // Limite de segurança: um modo de vídeo sem V-blank não trava.
        let limit = self.bus.now + u64::from(CPU_CLOCK) / 10;
        self.bus.gpu.frame_done = false;
        while !self.bus.gpu.frame_done && self.bus.now < limit {
            while self.bus.now < self.bus.next_event {
                self.hooks();
                self.cpu.step(&mut self.bus);
            }
            self.bus.run_events();
        }
    }

    /// Ganchos do BIOS: saída de texto (putchar) e a carga do PS-EXE.
    #[inline]
    fn hooks(&mut self) {
        let pc = self.cpu.pc() & 0x1FFF_FFFF;
        if pc > 0xB0 && pc != SHELL_ENTRY & 0x1FFF_FFFF {
            return;
        }
        let function = self.cpu.reg(9);
        if (pc == 0xA0 && function == 0x3C) || (pc == 0xB0 && function == 0x3D) {
            self.putchar(self.cpu.reg(4) as u8);
        } else if pc == SHELL_ENTRY & 0x1FFF_FFFF
            && let Some(exe) = self.pending_exe.take()
        {
            self.start_exe(&exe);
        }
    }

    fn putchar(&mut self, byte: u8) {
        if self.tty.len() >= MAX_TTY {
            self.tty.drain(..MAX_TTY / 2);
        }
        self.tty.push(byte);
    }

    fn start_exe(&mut self, exe: &Exe) {
        let base = exe.load_address as usize & 0x1F_FFFF;
        self.bus.ram[base..base + exe.text.len()].copy_from_slice(&exe.text);
        let (bss, bss_size) = (exe.bss.0 as usize & 0x1F_FFFF, exe.bss.1 as usize);
        self.bus.ram[bss..bss + bss_size].fill(0);
        self.cpu.set_register(28, exe.gp);
        if let Some(stack) = exe.stack {
            self.cpu.set_register(29, stack);
            self.cpu.set_register(30, stack);
        }
        self.cpu.jump_to(exe.pc);
    }

    /// Põe o disco no drive. Com um disco, o BIOS dá a partida no jogo.
    pub fn insert_disc(&mut self, disc: Disc) {
        self.bus.cdrom.insert_disc(disc);
    }

    /// Move o áudio gerado para `out`: estéreo intercalado (L, R) a
    /// [`SAMPLE_RATE`], valores em -1..1. Sem leitura, só o último meio
    /// segundo é guardado.
    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        self.bus.spu.drain(out);
    }

    /// Botões apertados no controle da porta 1.
    pub fn set_input(&mut self, buttons: Buttons) {
        self.bus.sio.set_buttons(buttons);
    }

    /// Conteúdo do memory card da porta 1 (128 KB, formato bruto `.mcr`):
    /// o save dos jogos.
    pub fn memory_card(&self) -> &[u8] {
        self.bus.sio.memory_card()
    }

    /// Coloca um memory card salvo na porta 1. Tamanho errado ou cartão não
    /// formatado é recusado e o cartão atual fica como está.
    pub fn load_memory_card(&mut self, bytes: &[u8]) -> Result<(), MemoryCardError> {
        self.bus.sio.load_memory_card(bytes)
    }

    /// Imagem do último quadro, `0x00RRGGBB`, e o tamanho (largura,
    /// altura), que muda com o modo de vídeo.
    pub fn framebuffer(&self) -> (&[u32], (usize, usize)) {
        self.bus.gpu.frame()
    }

    /// VRAM inteira (1024×512 pixels de 15 bits), para depuração e testes.
    pub fn vram(&self) -> &[u16] {
        &self.bus.gpu.renderer.vram
    }

    /// Texto escrito pelo BIOS (putchar), usado pelos programas de teste.
    pub fn tty_output(&self) -> &[u8] {
        &self.tty
    }

    /// Retira o texto escrito até aqui, para acompanhar saídas longas sem
    /// perder o começo.
    pub fn take_tty_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.tty)
    }

    /// Ciclos executados desde que o console ligou.
    pub fn cycles(&self) -> u64 {
        self.bus.now
    }

    /// PC atual, para depuração.
    pub fn pc(&self) -> u32 {
        self.cpu.pc()
    }
}

#[cfg(test)]
mod tests;
