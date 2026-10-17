//! Interface única para os núcleos de cada console.
//!
//! Um `enum` em vez de trait: são poucos núcleos, conhecidos em tempo de
//! compilação, e o laço do player não precisa de despacho dinâmico.

use crate::controls::{Action, System};
use crate::resample::Resampler;

/// Maior quadro entre os consoles de cartucho (o Genesis em H40): define a
/// janela deles.
pub const MAX_FRAME: (usize, usize) = (genesis::SCREEN_MAX_WIDTH, genesis::SCREEN_HEIGHT);
/// Quadro de referência do PlayStation (320×240); todos os modos são
/// esticados para 4:3, como na TV.
const PLAYSTATION_FRAME: (usize, usize) = (320, 240);

/// Imagem de um quadro: `size.0` pixels válidos em cada linha de `stride`.
pub struct Frame<'a> {
    pub pixels: &'a [u32],
    pub stride: usize,
    pub size: (usize, usize),
    /// Proporção da TV para a qual o console foi feito; `None` em telas
    /// próprias de pixels quadrados (Game Boy). O usuário escolhe se usa.
    pub tv_aspect: Option<(usize, usize)>,
}

pub enum Machine {
    MasterSystem {
        console: Box<sms::Console>,
        joypad: sms::Joypad,
    },
    Genesis {
        console: Box<genesis::Console>,
        pad: genesis::Pad,
    },
    GameBoy {
        console: Box<gameboy::Console>,
        buttons: gameboy::Buttons,
    },
    PlayStation {
        console: Box<psx::Console>,
        buttons: psx::Buttons,
        /// O SPU gera a 44,1 kHz; o dispositivo pode usar outra taxa.
        resampler: Resampler,
        native: Vec<f32>,
    },
}

#[derive(Debug)]
pub enum MachineError {
    MasterSystem(sms::RomError),
    Genesis(genesis::RomError),
    GameBoy(gameboy::RomError),
    Bios(psx::BiosError),
    /// Console de disco montado como cartucho (erro de quem chamou).
    NotACartridge,
}

impl std::fmt::Display for MachineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MasterSystem(e) => e.fmt(f),
            Self::Genesis(e) => e.fmt(f),
            Self::GameBoy(e) => e.fmt(f),
            Self::Bios(e) => e.fmt(f),
            Self::NotACartridge => f.write_str("este console usa disco, não cartucho"),
        }
    }
}

impl Machine {
    pub fn new(system: System, rom: Vec<u8>, sample_rate: u32) -> Result<Self, MachineError> {
        Ok(match system {
            System::MasterSystem => Self::MasterSystem {
                console: Box::new(
                    sms::Console::new(rom, sample_rate).map_err(MachineError::MasterSystem)?,
                ),
                joypad: sms::Joypad::default(),
            },
            System::Genesis => Self::Genesis {
                console: Box::new(
                    genesis::Console::new(rom, sample_rate).map_err(MachineError::Genesis)?,
                ),
                pad: genesis::Pad::default(),
            },
            System::GameBoy => Self::GameBoy {
                console: Box::new(
                    gameboy::Console::new(rom, sample_rate).map_err(MachineError::GameBoy)?,
                ),
                buttons: gameboy::Buttons::default(),
            },
            System::PlayStation => return Err(MachineError::NotACartridge),
        })
    }

    /// Liga o PlayStation com o BIOS do usuário e o disco no drive.
    pub fn playstation(
        bios: &[u8],
        disc: psx::Disc,
        sample_rate: u32,
    ) -> Result<Self, MachineError> {
        let mut console = Box::new(psx::Console::new(bios).map_err(MachineError::Bios)?);
        console.insert_disc(disc);
        Ok(Self::PlayStation {
            console,
            buttons: psx::Buttons::default(),
            resampler: Resampler::new(psx::SAMPLE_RATE, sample_rate),
            native: Vec::new(),
        })
    }

    pub fn frame_rate(&self) -> f64 {
        match self {
            Self::MasterSystem { .. } => sms::FRAME_RATE,
            Self::Genesis { .. } => genesis::FRAME_RATE,
            Self::GameBoy { .. } => gameboy::FRAME_RATE,
            // O jogo escolhe NTSC ou PAL; muda depois do BIOS.
            Self::PlayStation { console, .. } => console.frame_rate(),
        }
    }

    /// Quadro de referência para o tamanho da janela.
    pub fn base_frame(&self) -> (usize, usize) {
        match self {
            Self::PlayStation { .. } => PLAYSTATION_FRAME,
            _ => MAX_FRAME,
        }
    }

    /// Executa um quadro com os botões atuais.
    pub fn run_frame(&mut self) {
        match self {
            Self::MasterSystem { console, joypad } => {
                console.set_input(*joypad);
                console.run_frame();
            }
            Self::Genesis { console, pad } => {
                console.set_input(*pad);
                console.run_frame();
            }
            Self::GameBoy { console, buttons } => {
                console.set_input(*buttons);
                console.run_frame();
            }
            Self::PlayStation {
                console, buttons, ..
            } => {
                console.set_input(*buttons);
                console.run_frame();
            }
        }
    }

    pub fn frame(&self) -> Frame<'_> {
        match self {
            Self::MasterSystem { console, .. } => Frame {
                pixels: console.framebuffer(),
                stride: sms::SCREEN_WIDTH,
                size: (sms::SCREEN_WIDTH, sms::SCREEN_HEIGHT),
                tv_aspect: Some((4, 3)),
            },
            Self::Genesis { console, .. } => Frame {
                pixels: console.framebuffer(),
                stride: genesis::SCREEN_MAX_WIDTH,
                size: console.frame_size(),
                tv_aspect: Some((4, 3)),
            },
            Self::GameBoy { console, .. } => Frame {
                pixels: console.framebuffer(),
                stride: gameboy::SCREEN_WIDTH,
                size: (gameboy::SCREEN_WIDTH, gameboy::SCREEN_HEIGHT),
                tv_aspect: None,
            },
            Self::PlayStation { console, .. } => {
                let (pixels, size) = console.framebuffer();
                Frame {
                    pixels,
                    stride: size.0,
                    size,
                    // Os pixels do PS1 não são quadrados: 256 a 640 colunas
                    // e 240 ou 480 linhas ocupam a mesma tela 4:3.
                    tv_aspect: Some((4, 3)),
                }
            }
        }
    }

    /// Acrescenta o áudio gerado a `out`, estéreo intercalado (L, R).
    pub fn drain_audio(&mut self, out: &mut Vec<f32>, mono_scratch: &mut Vec<f32>) {
        match self {
            Self::MasterSystem { console, .. } => {
                mono_scratch.clear();
                console.drain_audio(mono_scratch);
                out.extend(mono_scratch.iter().flat_map(|&s| [s, s]));
            }
            Self::Genesis { console, .. } => console.drain_audio(out),
            Self::GameBoy { console, .. } => console.drain_audio(out),
            Self::PlayStation {
                console,
                resampler,
                native,
                ..
            } => {
                native.clear();
                console.drain_audio(native);
                resampler.process(native, out);
            }
        }
    }

    /// Aplica uma ação; ações de outro console são ignoradas.
    pub fn set_action(&mut self, action: Action, pressed: bool) {
        match self {
            Self::MasterSystem { console, joypad } => {
                if action == Action::Pause {
                    // Pause não é botão do controle: vira NMI ao apertar.
                    if pressed {
                        console.press_pause();
                    }
                } else if let Some(button) = sms_button(joypad, action) {
                    *button = pressed;
                }
            }
            Self::Genesis { pad, .. } => {
                if let Some(button) = genesis_button(pad, action) {
                    *button = pressed;
                }
            }
            Self::GameBoy { buttons, .. } => {
                if let Some(button) = game_boy_button(buttons, action) {
                    *button = pressed;
                }
            }
            Self::PlayStation { buttons, .. } => {
                if let Some(button) = playstation_button(buttons, action) {
                    *button = pressed;
                }
            }
        }
    }

    /// Solta todos os botões (ex.: a janela perdeu o foco).
    pub fn release_all(&mut self) {
        match self {
            Self::MasterSystem { joypad, .. } => *joypad = sms::Joypad::default(),
            Self::Genesis { pad, .. } => *pad = genesis::Pad::default(),
            Self::GameBoy { buttons, .. } => *buttons = gameboy::Buttons::default(),
            Self::PlayStation { buttons, .. } => *buttons = psx::Buttons::default(),
        }
    }

    /// O núcleo sabe salvar o estado ("continuar de onde parou"). O do
    /// PlayStation ainda não sabe.
    pub fn supports_state(&self) -> bool {
        !matches!(self, Self::PlayStation { .. })
    }

    /// Estado salvo, se o núcleo souber salvar ([`Machine::supports_state`]).
    pub fn save_state(&self) -> Option<Vec<u8>> {
        match self {
            Self::MasterSystem { console, .. } => Some(console.save_state()),
            Self::Genesis { console, .. } => Some(console.save_state()),
            Self::GameBoy { console, .. } => Some(console.save_state()),
            Self::PlayStation { .. } => None,
        }
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), sms::StateError> {
        match self {
            Self::MasterSystem { console, .. } => console.load_state(bytes),
            Self::Genesis { console, .. } => console.load_state(bytes),
            Self::GameBoy { console, .. } => console.load_state(bytes),
            Self::PlayStation { .. } => Err(sms::StateError::NotAState),
        }
    }

    /// O jogo pode ter save próprio. No Master System sempre pode: o
    /// cabeçalho não declara bateria, e a RAM só vira save quando o jogo a
    /// usa ([`Machine::battery_ram`]). No PlayStation o save fica no memory
    /// card, que vale igual.
    pub fn has_battery(&self) -> bool {
        match self {
            Self::MasterSystem { .. } | Self::PlayStation { .. } => true,
            Self::Genesis { .. } | Self::GameBoy { .. } => self.battery_ram().is_some(),
        }
    }

    /// O save do próprio jogo, se houver o que gravar: a RAM do cartucho com
    /// bateria ou, no PlayStation, o memory card inteiro (só depois que o
    /// jogo grava nele).
    pub fn battery_ram(&self) -> Option<&[u8]> {
        match self {
            Self::MasterSystem { console, .. } => console.battery_ram(),
            Self::Genesis { console, .. } => console.battery_ram(),
            Self::GameBoy { console, .. } => console.battery_ram(),
            Self::PlayStation { console, .. } => {
                console.memory_card_written().then(|| console.memory_card())
            }
        }
    }

    /// Carrega o save do jogo; `false` se o cartucho (ou o memory card) não
    /// aceita esses bytes.
    pub fn load_battery_ram(&mut self, bytes: &[u8]) -> bool {
        match self {
            Self::MasterSystem { console, .. } => console.load_battery_ram(bytes),
            Self::Genesis { console, .. } => console.load_battery_ram(bytes),
            Self::GameBoy { console, .. } => console.load_battery_ram(bytes),
            Self::PlayStation { console, .. } => console.load_memory_card(bytes).is_ok(),
        }
    }
}

fn sms_button(joypad: &mut sms::Joypad, action: Action) -> Option<&mut bool> {
    Some(match action {
        Action::Up => &mut joypad.up,
        Action::Down => &mut joypad.down,
        Action::Left => &mut joypad.left,
        Action::Right => &mut joypad.right,
        Action::Button1 => &mut joypad.button1,
        Action::Button2 => &mut joypad.button2,
        _ => return None,
    })
}

fn genesis_button(pad: &mut genesis::Pad, action: Action) -> Option<&mut bool> {
    Some(match action {
        Action::Up => &mut pad.up,
        Action::Down => &mut pad.down,
        Action::Left => &mut pad.left,
        Action::Right => &mut pad.right,
        Action::A => &mut pad.a,
        Action::B => &mut pad.b,
        Action::C => &mut pad.c,
        Action::X => &mut pad.x,
        Action::Y => &mut pad.y,
        Action::Z => &mut pad.z,
        Action::Start => &mut pad.start,
        Action::Mode => &mut pad.mode,
        _ => return None,
    })
}

fn game_boy_button(buttons: &mut gameboy::Buttons, action: Action) -> Option<&mut bool> {
    Some(match action {
        Action::Up => &mut buttons.up,
        Action::Down => &mut buttons.down,
        Action::Left => &mut buttons.left,
        Action::Right => &mut buttons.right,
        Action::A => &mut buttons.a,
        Action::B => &mut buttons.b,
        Action::Start => &mut buttons.start,
        Action::Select => &mut buttons.select,
        _ => return None,
    })
}

fn playstation_button(buttons: &mut psx::Buttons, action: Action) -> Option<&mut bool> {
    Some(match action {
        Action::Up => &mut buttons.up,
        Action::Down => &mut buttons.down,
        Action::Left => &mut buttons.left,
        Action::Right => &mut buttons.right,
        Action::Cross => &mut buttons.cross,
        Action::Circle => &mut buttons.circle,
        Action::Square => &mut buttons.square,
        Action::Triangle => &mut buttons.triangle,
        Action::L1 => &mut buttons.l1,
        Action::R1 => &mut buttons.r1,
        Action::L2 => &mut buttons.l2,
        Action::R2 => &mut buttons.r2,
        Action::Start => &mut buttons.start,
        Action::Select => &mut buttons.select,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_system_actions_map_to_joypad() {
        let mut joypad = sms::Joypad::default();
        *sms_button(&mut joypad, Action::Left).expect("botão") = true;
        *sms_button(&mut joypad, Action::Button1).expect("botão") = true;
        assert!(sms_button(&mut joypad, Action::Start).is_none());
        assert_eq!(
            joypad,
            sms::Joypad {
                left: true,
                button1: true,
                ..sms::Joypad::default()
            }
        );
    }

    #[test]
    fn genesis_actions_cover_every_button() {
        let mut pad = genesis::Pad::default();
        for &action in System::Genesis.actions() {
            *genesis_button(&mut pad, action).expect("botão do Genesis") = true;
        }
        assert!(pad.up && pad.a && pad.c && pad.x && pad.z && pad.start && pad.mode);
        assert!(genesis_button(&mut pad, Action::Pause).is_none());
    }

    #[test]
    fn game_boy_actions_cover_every_button() {
        let mut buttons = gameboy::Buttons::default();
        for &action in System::GameBoy.actions() {
            *game_boy_button(&mut buttons, action).expect("botão do Game Boy") = true;
        }
        assert!(buttons.up && buttons.a && buttons.b && buttons.start && buttons.select);
        assert!(game_boy_button(&mut buttons, Action::C).is_none());
    }

    #[test]
    fn playstation_actions_cover_every_button() {
        let mut buttons = psx::Buttons::default();
        for &action in System::PlayStation.actions() {
            *playstation_button(&mut buttons, action).expect("botão do PlayStation") = true;
        }
        let all = psx::Buttons {
            up: true,
            down: true,
            left: true,
            right: true,
            cross: true,
            circle: true,
            square: true,
            triangle: true,
            l1: true,
            r1: true,
            l2: true,
            r2: true,
            start: true,
            select: true,
        };
        assert_eq!(buttons, all);
        assert!(playstation_button(&mut buttons, Action::A).is_none());
    }

    #[test]
    fn master_system_audio_is_duplicated_to_stereo() {
        let mut machine =
            Machine::new(System::MasterSystem, vec![0x18, 0xFE], 44_100).expect("ROM válida");
        machine.run_frame();
        let (mut out, mut scratch) = (Vec::new(), Vec::new());
        machine.drain_audio(&mut out, &mut scratch);
        assert!(!out.is_empty() && out.len() == scratch.len() * 2);
        assert!(out.chunks(2).all(|f| f[0] == f[1]));
    }
}
