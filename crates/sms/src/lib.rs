//! Núcleo do Master System em safe Rust.
//!
//! [`Console`] liga a CPU [`z80::Z80`] ao VDP, ao PSG, ao mapper Sega e aos
//! controles. A API é síncrona e determinística: quem a usa (o player)
//! decide o ritmo, a janela e o áudio.
//!
//! Escopo atual: NTSC, 192 linhas, mapper Sega, sem BIOS e sem FM. Jogos com
//! o mapper Codemasters ou que exigem os modos de 224/240 linhas ainda não
//! funcionam corretamente.

mod bus;
pub mod vdp;

use chips::psg;
use chips::state;
pub use chips::z80;

use std::fmt;

use bus::{BANK_SIZE, SmsBus};
use psg::Psg;
use z80::Z80;

pub use chips::state::StateError;
pub use vdp::{HEIGHT as SCREEN_HEIGHT, WIDTH as SCREEN_WIDTH};

/// Clock da CPU no NTSC (Hz).
pub const CPU_CLOCK: u32 = 3_579_545;
/// Ciclos de CPU por linha de vídeo.
pub const CYCLES_PER_LINE: u32 = 228;
/// Quadros por segundo no NTSC (≈ 59,92).
pub const FRAME_RATE: f64 =
    CPU_CLOCK as f64 / (CYCLES_PER_LINE as f64 * vdp::LINES_PER_FRAME as f64);

/// Maior cartucho oficial tem 512 KB; 4 MB dá folga para hacks e homebrew.
const MAX_ROM_SIZE: usize = 4 * 1024 * 1024;
/// Assinatura e versão do formato de estado salvo deste núcleo.
const STATE_MAGIC: &[u8; 8] = b"PCSMSST\0";
const STATE_VERSION: u16 = 1;
/// Cabeçalho de copiadora presente em alguns dumps.
const COPIER_HEADER: usize = 512;

/// Estado dos botões do controle 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Joypad {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub button1: bool,
    pub button2: bool,
}

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

/// Um Master System com um cartucho inserido.
#[derive(Debug, Clone)]
pub struct Console {
    cpu: Z80,
    bus: SmsBus,
    /// Ciclos executados além do fim da linha anterior.
    overshoot: u32,
    /// Identifica a ROM nos estados salvos.
    rom_hash: u64,
}

impl Console {
    /// Liga o console com a ROM e gera áudio na taxa `sample_rate` (Hz).
    pub fn new(rom: Vec<u8>, sample_rate: u32) -> Result<Self, RomError> {
        let rom = normalize_rom(rom)?;
        let rom_hash = state::rom_hash(&rom);
        let mut cpu = Z80::new();
        // Sem BIOS: deixa a pilha onde a BIOS deixaria.
        cpu.sp = 0xDFF0;
        Ok(Self {
            cpu,
            bus: SmsBus::new(rom, Psg::new(CPU_CLOCK, sample_rate)),
            overshoot: 0,
            rom_hash,
        })
    }

    /// Executa um quadro completo (262 linhas).
    pub fn run_frame(&mut self) {
        for line in 0..vdp::LINES_PER_FRAME {
            self.bus.vdp.render_line(line);
            let mut cycles = self.overshoot;
            while cycles < CYCLES_PER_LINE {
                self.bus.line_cycles = cycles;
                let spent = self.cpu.step(&mut self.bus);
                self.bus.psg.run(spent);
                cycles += spent;
            }
            self.overshoot = cycles - CYCLES_PER_LINE;
            self.bus.vdp.end_line();
        }
    }

    /// Imagem do último quadro: 256×192 pixels `0x00RRGGBB`, linha a linha.
    pub fn framebuffer(&self) -> &[u32] {
        self.bus.vdp.framebuffer()
    }

    /// Move o áudio gerado (mono, `f32` em 0.0..1.0) para `out`.
    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        self.bus.psg.drain(out);
    }

    pub fn set_input(&mut self, joypad: Joypad) {
        self.bus.joypad = joypad;
    }

    /// Estado completo da máquina, para continuar o jogo depois. Deve ser
    /// chamado entre quadros (depois de [`Console::run_frame`]).
    pub fn save_state(&self) -> Vec<u8> {
        let mut w = state::Writer::default();
        w.slice(STATE_MAGIC);
        w.u16(STATE_VERSION);
        w.u64(self.rom_hash);
        self.cpu.save_state(&mut w);
        self.bus.save_state(&mut w);
        w.u32(self.overshoot);
        w.bytes
    }

    /// Restaura um estado de [`Console::save_state`] feito com a mesma ROM.
    /// Em caso de erro, o console fica exatamente como estava.
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
        next.overshoot = r.u32()?;
        if next.overshoot >= CYCLES_PER_LINE {
            return Err(StateError::Invalid("ciclos da linha"));
        }
        r.finish()?;
        *self = next;
        Ok(())
    }

    /// Botão Pause do console (gera uma NMI).
    pub fn press_pause(&mut self) {
        self.cpu.request_nmi();
    }
}

/// Remove o cabeçalho de copiadora e completa a ROM até um banco inteiro.
fn normalize_rom(mut rom: Vec<u8>) -> Result<Vec<u8>, RomError> {
    if rom.len() > MAX_ROM_SIZE {
        return Err(RomError::TooLarge(rom.len()));
    }
    if rom.len() % BANK_SIZE == COPIER_HEADER {
        rom.drain(..COPIER_HEADER);
    }
    if rom.is_empty() {
        return Err(RomError::Empty);
    }
    let padded = rom.len().div_ceil(BANK_SIZE) * BANK_SIZE;
    rom.resize(padded, 0xFF);
    Ok(rom)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monta uma ROM com o programa em 0x0000.
    fn rom_with(program: &[u8]) -> Vec<u8> {
        let mut rom = vec![0; BANK_SIZE];
        rom[..program.len()].copy_from_slice(program);
        rom
    }

    #[test]
    fn rejects_empty_and_oversized_roms() {
        assert_eq!(
            Console::new(Vec::new(), 44_100).err(),
            Some(RomError::Empty)
        );
        assert_eq!(
            Console::new(vec![0; COPIER_HEADER], 44_100).err(),
            Some(RomError::Empty)
        );
        let huge = vec![0; MAX_ROM_SIZE + 1];
        assert!(matches!(
            Console::new(huge, 44_100),
            Err(RomError::TooLarge(_))
        ));
    }

    #[test]
    fn strips_copier_header_and_pads_to_bank() {
        let mut rom = vec![0xAA; COPIER_HEADER];
        rom.extend(vec![0x11; BANK_SIZE]);
        let rom = normalize_rom(rom).unwrap();
        assert_eq!(rom.len(), BANK_SIZE);
        assert_eq!(rom[0], 0x11);

        let rom = normalize_rom(vec![0x22; 100]).unwrap();
        assert_eq!(rom.len(), BANK_SIZE);
        assert_eq!(rom[BANK_SIZE - 1], 0xFF);
    }

    /// Programa que liga a tela, pinta a cor 0 de branco e fica em loop:
    /// o quadro inteiro deve sair branco (teste de regressão determinístico).
    #[test]
    fn program_writing_vdp_produces_expected_frame() {
        let program = [
            0xF3, // DI
            0x3E, 0x40, 0xD3, 0xBF, // reg 1 = 40h (tela ligada)
            0x3E, 0x81, 0xD3, 0xBF, //
            0x3E, 0x00, 0xD3, 0xBF, // CRAM endereço 0
            0x3E, 0xC0, 0xD3, 0xBF, //
            0x3E, 0x3F, 0xD3, 0xBE, // cor 0 = branco
            0x18, 0xFE, // JR $
        ];
        let mut console = Console::new(rom_with(&program), 44_100).unwrap();
        console.run_frame();
        console.run_frame();
        assert!(console.framebuffer().iter().all(|&p| p == 0xFFFFFF));
    }

    #[test]
    fn frame_produces_audio_for_one_sixtieth_of_a_second() {
        let mut console = Console::new(rom_with(&[0x18, 0xFE]), 48_000).unwrap();
        console.run_frame();
        let mut audio = Vec::new();
        console.drain_audio(&mut audio);
        let expected = (48_000.0 / FRAME_RATE) as usize;
        assert!(audio.len().abs_diff(expected) <= 2, "{}", audio.len());
    }

    #[test]
    fn vblank_interrupt_reaches_handler() {
        // 0000: IM 1 ; liga IE0 (reg 1 = 20h) ; EI ; HALT ; JR -3
        // 0038: INC B ; IN A,(BFh) (reconhece) ; EI ; RETI
        let mut program = vec![
            0xED, 0x56, 0x3E, 0x20, 0xD3, 0xBF, 0x3E, 0x81, 0xD3, 0xBF, 0xFB, 0x76, 0x18, 0xFD,
        ];
        program.resize(0x38, 0);
        program.extend([0x04, 0xDB, 0xBF, 0xFB, 0xED, 0x4D]);
        let mut console = Console::new(rom_with(&program), 44_100).unwrap();
        for _ in 0..3 {
            console.run_frame();
        }
        assert_eq!(console.cpu.b, 3);
    }

    #[test]
    fn pause_triggers_nmi_handler() {
        // 0000: JR $ ; 0066: LD A,77h ; RETN
        let mut program = vec![0x18, 0xFE];
        program.resize(0x66, 0);
        program.extend([0x3E, 0x77, 0xED, 0x45]);
        let mut console = Console::new(rom_with(&program), 44_100).unwrap();
        console.press_pause();
        console.run_frame();
        assert_eq!(console.cpu.a, 0x77);
        assert_eq!(console.cpu.pc, 0);
    }

    /// Contador em RAM incrementado a cada VBlank, desenhado como cor de
    /// fundo: qualquer divergência após carregar o estado muda o quadro.
    fn counting_rom() -> Vec<u8> {
        // 0000: IM 1 ; liga tela + IE0 ; EI ; HALT ; JR -3
        // 0038: LD HL,C000h ; INC (HL) ; LD A,(HL) ; OUT (BFh) CRAM 0 ; OUT (BEh),A
        //       IN A,(BFh) ; EI ; RETI
        let mut program = vec![
            0xED, 0x56, 0x3E, 0x60, 0xD3, 0xBF, 0x3E, 0x81, 0xD3, 0xBF, 0xFB, 0x76, 0x18, 0xFD,
        ];
        program.resize(0x38, 0);
        program.extend([
            0x21, 0x00, 0xC0, 0x34, 0x7E, 0xF5, 0x3E, 0x00, 0xD3, 0xBF, 0x3E, 0xC0, 0xD3, 0xBF,
            0xF1, 0xD3, 0xBE, 0xDB, 0xBF, 0xFB, 0xED, 0x4D,
        ]);
        rom_with(&program)
    }

    #[test]
    fn save_and_load_state_resume_deterministically() {
        let mut original = Console::new(counting_rom(), 44_100).unwrap();
        for _ in 0..10 {
            original.run_frame();
        }
        let saved = original.save_state();
        for _ in 0..5 {
            original.run_frame();
        }

        let mut resumed = Console::new(counting_rom(), 44_100).unwrap();
        resumed.load_state(&saved).unwrap();
        for _ in 0..5 {
            resumed.run_frame();
        }
        assert_eq!(resumed.framebuffer(), original.framebuffer());
        assert_eq!(resumed.save_state(), original.save_state());
        assert_eq!(z80::Bus::read(&mut resumed.bus, 0xC000), 15);
    }

    #[test]
    fn load_state_rejects_other_rom_and_corruption() {
        let mut console = Console::new(counting_rom(), 44_100).unwrap();
        console.run_frame();
        let saved = console.save_state();

        let mut other = Console::new(rom_with(&[0x18, 0xFE]), 44_100).unwrap();
        assert_eq!(other.load_state(&saved), Err(StateError::WrongRom));

        assert_eq!(console.load_state(b"lixo"), Err(StateError::NotAState));
        assert_eq!(
            console.load_state(&saved[..saved.len() - 1]),
            Err(StateError::Truncated)
        );
        let mut extra = saved.clone();
        extra.push(0);
        assert_eq!(console.load_state(&extra), Err(StateError::TrailingData));

        let mut future = saved.clone();
        future[8] = 99;
        assert_eq!(
            console.load_state(&future),
            Err(StateError::UnsupportedVersion(99))
        );
    }

    #[test]
    fn failed_load_leaves_console_untouched() {
        let mut console = Console::new(counting_rom(), 44_100).unwrap();
        for _ in 0..3 {
            console.run_frame();
        }
        let before = console.save_state();
        // Corrompe o modo de interrupção (logo após os 8+9*2+4 bytes da CPU).
        let mut corrupted = before.clone();
        let im_offset = 8 + 2 + 8 + 8 + 9 * 2 + 2;
        corrupted[im_offset] = 7;
        assert_eq!(
            console.load_state(&corrupted),
            Err(StateError::Invalid("modo de interrupção"))
        );
        assert_eq!(console.save_state(), before);
    }

    #[test]
    fn input_is_visible_on_port_dc() {
        // IN A,(DCh) ; LD (C000h),A ; JR $
        let program = [0xDB, 0xDC, 0x32, 0x00, 0xC0, 0x18, 0xFE];
        let mut console = Console::new(rom_with(&program), 44_100).unwrap();
        console.set_input(Joypad {
            right: true,
            ..Joypad::default()
        });
        console.run_frame();
        assert_eq!(z80::Bus::read(&mut console.bus, 0xC000), 0xF7);
    }
}
