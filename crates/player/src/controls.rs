//! Mapeamento de teclas para os controles do Master System.
//!
//! As teclas usam os nomes do padrão W3C UI Events (`KeyboardEvent.code`,
//! como `"ArrowUp"` e `"KeyZ"`): a interface web captura exatamente esses
//! nomes e eles correspondem às variantes de [`KeyCode`] do winit. Só as
//! teclas de [`SUPPORTED_KEYS`] são aceitas; Esc fica reservado para fechar
//! o jogo.

use std::fmt;

use winit::keyboard::KeyCode;

/// Ação do controle (ou do console, no caso do Pause).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Button1,
    Button2,
    Pause,
}

impl Action {
    pub const ALL: [Action; 7] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::Button1,
        Action::Button2,
        Action::Pause,
    ];

    /// Identificador estável, usado nas preferências e na interface.
    pub fn id(self) -> &'static str {
        match self {
            Action::Up => "up",
            Action::Down => "down",
            Action::Left => "left",
            Action::Right => "right",
            Action::Button1 => "button1",
            Action::Button2 => "button2",
            Action::Pause => "pause",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Teclas que podem ser atribuídas, com o código correspondente do winit.
pub const SUPPORTED_KEYS: &[(&str, KeyCode)] = &[
    ("ArrowUp", KeyCode::ArrowUp),
    ("ArrowDown", KeyCode::ArrowDown),
    ("ArrowLeft", KeyCode::ArrowLeft),
    ("ArrowRight", KeyCode::ArrowRight),
    ("Enter", KeyCode::Enter),
    ("Space", KeyCode::Space),
    ("Tab", KeyCode::Tab),
    ("Backspace", KeyCode::Backspace),
    ("ShiftLeft", KeyCode::ShiftLeft),
    ("ShiftRight", KeyCode::ShiftRight),
    ("ControlLeft", KeyCode::ControlLeft),
    ("ControlRight", KeyCode::ControlRight),
    ("AltLeft", KeyCode::AltLeft),
    ("AltRight", KeyCode::AltRight),
    ("KeyA", KeyCode::KeyA),
    ("KeyB", KeyCode::KeyB),
    ("KeyC", KeyCode::KeyC),
    ("KeyD", KeyCode::KeyD),
    ("KeyE", KeyCode::KeyE),
    ("KeyF", KeyCode::KeyF),
    ("KeyG", KeyCode::KeyG),
    ("KeyH", KeyCode::KeyH),
    ("KeyI", KeyCode::KeyI),
    ("KeyJ", KeyCode::KeyJ),
    ("KeyK", KeyCode::KeyK),
    ("KeyL", KeyCode::KeyL),
    ("KeyM", KeyCode::KeyM),
    ("KeyN", KeyCode::KeyN),
    ("KeyO", KeyCode::KeyO),
    ("KeyP", KeyCode::KeyP),
    ("KeyQ", KeyCode::KeyQ),
    ("KeyR", KeyCode::KeyR),
    ("KeyS", KeyCode::KeyS),
    ("KeyT", KeyCode::KeyT),
    ("KeyU", KeyCode::KeyU),
    ("KeyV", KeyCode::KeyV),
    ("KeyW", KeyCode::KeyW),
    ("KeyX", KeyCode::KeyX),
    ("KeyY", KeyCode::KeyY),
    ("KeyZ", KeyCode::KeyZ),
    ("Digit0", KeyCode::Digit0),
    ("Digit1", KeyCode::Digit1),
    ("Digit2", KeyCode::Digit2),
    ("Digit3", KeyCode::Digit3),
    ("Digit4", KeyCode::Digit4),
    ("Digit5", KeyCode::Digit5),
    ("Digit6", KeyCode::Digit6),
    ("Digit7", KeyCode::Digit7),
    ("Digit8", KeyCode::Digit8),
    ("Digit9", KeyCode::Digit9),
    ("Numpad0", KeyCode::Numpad0),
    ("Numpad1", KeyCode::Numpad1),
    ("Numpad2", KeyCode::Numpad2),
    ("Numpad3", KeyCode::Numpad3),
    ("Numpad4", KeyCode::Numpad4),
    ("Numpad5", KeyCode::Numpad5),
    ("Numpad6", KeyCode::Numpad6),
    ("Numpad7", KeyCode::Numpad7),
    ("Numpad8", KeyCode::Numpad8),
    ("Numpad9", KeyCode::Numpad9),
    ("NumpadEnter", KeyCode::NumpadEnter),
    ("Comma", KeyCode::Comma),
    ("Period", KeyCode::Period),
    ("Slash", KeyCode::Slash),
    ("Semicolon", KeyCode::Semicolon),
    ("Quote", KeyCode::Quote),
    ("BracketLeft", KeyCode::BracketLeft),
    ("BracketRight", KeyCode::BracketRight),
    ("Backslash", KeyCode::Backslash),
    ("Minus", KeyCode::Minus),
    ("Equal", KeyCode::Equal),
];

/// Nome canônico (`&'static`) de uma tecla suportada.
fn supported(name: &str) -> Option<(&'static str, KeyCode)> {
    SUPPORTED_KEYS.iter().copied().find(|(n, _)| *n == name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingError {
    UnknownAction(String),
    UnsupportedKey(String),
    Missing(Action),
    Duplicate(String),
    Malformed(String),
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAction(id) => write!(f, "ação de controle desconhecida: {id}"),
            Self::UnsupportedKey(key) => write!(f, "tecla não suportada: {key}"),
            Self::Missing(action) => write!(f, "falta a tecla de {}", action.id()),
            Self::Duplicate(key) => write!(f, "a tecla {key} está em mais de uma ação"),
            Self::Malformed(part) => write!(f, "mapeamento inválido: {part}"),
        }
    }
}

impl std::error::Error for BindingError {}

/// Uma tecla por ação, sem repetições.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    keys: [(&'static str, KeyCode); Action::ALL.len()],
}

impl Default for Bindings {
    /// Setas, Z, X e Enter.
    fn default() -> Self {
        Self {
            keys: [
                ("ArrowUp", KeyCode::ArrowUp),
                ("ArrowDown", KeyCode::ArrowDown),
                ("ArrowLeft", KeyCode::ArrowLeft),
                ("ArrowRight", KeyCode::ArrowRight),
                ("KeyZ", KeyCode::KeyZ),
                ("KeyX", KeyCode::KeyX),
                ("Enter", KeyCode::Enter),
            ],
        }
    }
}

impl Bindings {
    /// Monta a partir de pares `(ação, tecla)`; exige todas as ações, teclas
    /// suportadas e nenhuma tecla repetida.
    pub fn from_pairs<'a>(
        pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, BindingError> {
        let mut keys: [Option<(&'static str, KeyCode)>; Action::ALL.len()] = Default::default();
        for (action_id, key) in pairs {
            let action = Action::from_id(action_id)
                .ok_or_else(|| BindingError::UnknownAction(action_id.to_owned()))?;
            let key = supported(key).ok_or_else(|| BindingError::UnsupportedKey(key.to_owned()))?;
            keys[action.index()] = Some(key);
        }
        let mut resolved = Self::default().keys;
        for action in Action::ALL {
            resolved[action.index()] = keys[action.index()].ok_or(BindingError::Missing(action))?;
        }
        for (i, (name, _)) in resolved.iter().enumerate() {
            if resolved[..i].iter().any(|(other, _)| other == name) {
                return Err(BindingError::Duplicate((*name).to_owned()));
            }
        }
        Ok(Self { keys: resolved })
    }

    /// Nome da tecla atribuída à ação.
    pub fn key(&self, action: Action) -> &'static str {
        self.keys[action.index()].0
    }

    /// Ação acionada pela tecla, se houver.
    pub fn action_for(&self, code: KeyCode) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|a| self.keys[a.index()].1 == code)
    }

    /// Pares `(ação, tecla)` na ordem de [`Action::ALL`].
    pub fn pairs(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        Action::ALL.into_iter().map(|a| (a.id(), self.key(a)))
    }

    /// Formato de linha de comando: `up=ArrowUp,down=ArrowDown,...`.
    pub fn to_arg(&self) -> String {
        self.pairs()
            .map(|(action, key)| format!("{action}={key}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn parse_arg(arg: &str) -> Result<Self, BindingError> {
        let pairs = arg
            .split(',')
            .map(|part| {
                part.split_once('=')
                    .ok_or_else(|| BindingError::Malformed(part.to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_pairs(pairs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_arrows_z_x_enter() {
        let b = Bindings::default();
        assert_eq!(b.key(Action::Up), "ArrowUp");
        assert_eq!(b.key(Action::Button1), "KeyZ");
        assert_eq!(b.key(Action::Pause), "Enter");
        assert_eq!(b.action_for(KeyCode::KeyX), Some(Action::Button2));
        assert_eq!(b.action_for(KeyCode::KeyQ), None);
    }

    #[test]
    fn default_keys_are_supported() {
        for (_, key) in Bindings::default().pairs() {
            assert!(supported(key).is_some(), "{key}");
        }
    }

    #[test]
    fn arg_round_trips() {
        let custom = Bindings::from_pairs([
            ("up", "KeyW"),
            ("down", "KeyS"),
            ("left", "KeyA"),
            ("right", "KeyD"),
            ("button1", "KeyJ"),
            ("button2", "KeyK"),
            ("pause", "Space"),
        ])
        .expect("mapeamento válido");
        assert_eq!(Bindings::parse_arg(&custom.to_arg()), Ok(custom.clone()));
        assert_eq!(custom.action_for(KeyCode::KeyJ), Some(Action::Button1));
    }

    #[test]
    fn rejects_invalid_mappings() {
        let mut pairs: Vec<(&str, &str)> = Bindings::default().pairs().collect();
        pairs[1].1 = "ArrowUp";
        assert_eq!(
            Bindings::from_pairs(pairs.clone()),
            Err(BindingError::Duplicate("ArrowUp".to_owned()))
        );

        pairs[1].1 = "Escape";
        assert_eq!(
            Bindings::from_pairs(pairs.clone()),
            Err(BindingError::UnsupportedKey("Escape".to_owned()))
        );

        assert_eq!(
            Bindings::from_pairs([("up", "KeyW")]),
            Err(BindingError::Missing(Action::Down))
        );
        assert_eq!(
            Bindings::from_pairs([("jump", "KeyW")]),
            Err(BindingError::UnknownAction("jump".to_owned()))
        );
        assert!(matches!(
            Bindings::parse_arg("up"),
            Err(BindingError::Malformed(_))
        ));
    }
}
