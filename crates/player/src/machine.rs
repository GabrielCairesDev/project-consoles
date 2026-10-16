//! Interface única para os núcleos de cada console.
//!
//! Um `enum` em vez de trait: são poucos núcleos, conhecidos em tempo de
//! compilação, e o laço do player não precisa de despacho dinâmico.

use crate::controls::{Action, System};

/// Maior quadro entre os consoles (o Genesis em H40): define a janela.
pub const MAX_FRAME: (usize, usize) = (genesis::SCREEN_MAX_WIDTH, genesis::SCREEN_HEIGHT);

/// Imagem de um quadro: `size.0` pixels válidos em cada linha de `stride`.
pub struct Frame<'a> {
    pub pixels: &'a [u32],
    pub stride: usize,
    pub size: (usize, usize),
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
}

#[derive(Debug)]
pub enum MachineError {
    MasterSystem(sms::RomError),
    Genesis(genesis::RomError),
}

impl std::fmt::Display for MachineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MasterSystem(e) => e.fmt(f),
            Self::Genesis(e) => e.fmt(f),
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
        })
    }

    pub fn frame_rate(&self) -> f64 {
        match self {
            Self::MasterSystem { .. } => sms::FRAME_RATE,
            Self::Genesis { .. } => genesis::FRAME_RATE,
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
        }
    }

    pub fn frame(&self) -> Frame<'_> {
        match self {
            Self::MasterSystem { console, .. } => Frame {
                pixels: console.framebuffer(),
                stride: sms::SCREEN_WIDTH,
                size: (sms::SCREEN_WIDTH, sms::SCREEN_HEIGHT),
            },
            Self::Genesis { console, .. } => Frame {
                pixels: console.framebuffer(),
                stride: genesis::SCREEN_MAX_WIDTH,
                size: console.frame_size(),
            },
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
        }
    }

    /// Solta todos os botões (ex.: a janela perdeu o foco).
    pub fn release_all(&mut self) {
        match self {
            Self::MasterSystem { joypad, .. } => *joypad = sms::Joypad::default(),
            Self::Genesis { pad, .. } => *pad = genesis::Pad::default(),
        }
    }

    pub fn save_state(&self) -> Vec<u8> {
        match self {
            Self::MasterSystem { console, .. } => console.save_state(),
            Self::Genesis { console, .. } => console.save_state(),
        }
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), sms::StateError> {
        match self {
            Self::MasterSystem { console, .. } => console.load_state(bytes),
            Self::Genesis { console, .. } => console.load_state(bytes),
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
