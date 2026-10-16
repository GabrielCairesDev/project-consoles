//! Mapeamento de teclas para os controles de cada console.
//!
//! As teclas usam os nomes do padrão W3C UI Events (`KeyboardEvent.code`,
//! como `"ArrowUp"` e `"KeyZ"`): a interface web captura exatamente esses
//! nomes e eles correspondem às variantes de [`KeyCode`] do winit. Só as
//! teclas de [`SUPPORTED_KEYS`] são aceitas; Esc fica reservado para fechar
//! o jogo.

use std::fmt;

use winit::keyboard::KeyCode;

/// Console com núcleo próprio; define as ações do controle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum System {
    MasterSystem,
    Genesis,
}

impl System {
    pub const ALL: [System; 2] = [System::MasterSystem, System::Genesis];

    /// Identificador do console no catálogo do launcher.
    pub fn id(self) -> &'static str {
        match self {
            System::MasterSystem => "master-system",
            System::Genesis => "mega-drive",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Ações do controle, na ordem exibida.
    pub fn actions(self) -> &'static [Action] {
        self.layout().0
    }

    /// Teclas padrão, na ordem de [`System::actions`].
    fn default_keys(self) -> &'static [&'static str] {
        self.layout().1
    }

    fn layout(self) -> (&'static [Action], &'static [&'static str]) {
        use Action::*;
        match self {
            System::MasterSystem => (
                &[Up, Down, Left, Right, Button1, Button2, Pause],
                &[
                    "ArrowUp",
                    "ArrowDown",
                    "ArrowLeft",
                    "ArrowRight",
                    "KeyZ",
                    "KeyX",
                    "Enter",
                ],
            ),
            System::Genesis => (
                &[Up, Down, Left, Right, A, B, C, X, Y, Z, Start, Mode],
                &[
                    "ArrowUp",
                    "ArrowDown",
                    "ArrowLeft",
                    "ArrowRight",
                    "KeyZ",
                    "KeyX",
                    "KeyC",
                    "KeyA",
                    "KeyS",
                    "KeyD",
                    "Enter",
                    "ShiftRight",
                ],
            ),
        }
    }
}

/// Ação do controle (ou do console, no caso do Pause do Master System).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Button1,
    Button2,
    Pause,
    A,
    B,
    C,
    X,
    Y,
    Z,
    Start,
    Mode,
}

impl Action {
    pub const ALL: [Action; 15] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::Button1,
        Action::Button2,
        Action::Pause,
        Action::A,
        Action::B,
        Action::C,
        Action::X,
        Action::Y,
        Action::Z,
        Action::Start,
        Action::Mode,
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
            Action::A => "a",
            Action::B => "b",
            Action::C => "c",
            Action::X => "x",
            Action::Y => "y",
            Action::Z => "z",
            Action::Start => "start",
            Action::Mode => "mode",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
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

/// Uma tecla por ação do console, sem repetições.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    system: System,
    /// Na ordem de [`System::actions`].
    keys: Vec<(&'static str, KeyCode)>,
}

impl Bindings {
    /// Teclas padrão do console (Master System: setas, Z, X e Enter).
    pub fn defaults(system: System) -> Self {
        let keys = system
            .default_keys()
            .iter()
            .map(|name| supported(name).expect("teclas padrão são suportadas"))
            .collect();
        Self { system, keys }
    }

    /// Monta a partir de pares `(ação, tecla)`; exige todas as ações do
    /// console, teclas suportadas e nenhuma tecla repetida.
    pub fn from_pairs<'a>(
        system: System,
        pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, BindingError> {
        let actions = system.actions();
        let mut keys: Vec<Option<(&'static str, KeyCode)>> = vec![None; actions.len()];
        for (action_id, key) in pairs {
            let index = Action::from_id(action_id)
                .and_then(|action| actions.iter().position(|&a| a == action))
                .ok_or_else(|| BindingError::UnknownAction(action_id.to_owned()))?;
            let key = supported(key).ok_or_else(|| BindingError::UnsupportedKey(key.to_owned()))?;
            keys[index] = Some(key);
        }
        let resolved = keys
            .into_iter()
            .zip(actions)
            .map(|(key, &action)| key.ok_or(BindingError::Missing(action)))
            .collect::<Result<Vec<_>, _>>()?;
        for (i, (name, _)) in resolved.iter().enumerate() {
            if resolved[..i].iter().any(|(other, _)| other == name) {
                return Err(BindingError::Duplicate((*name).to_owned()));
            }
        }
        Ok(Self {
            system,
            keys: resolved,
        })
    }

    pub fn system(&self) -> System {
        self.system
    }

    /// Nome da tecla atribuída à ação (`None` se não é ação deste console).
    pub fn key(&self, action: Action) -> Option<&'static str> {
        let index = self.system.actions().iter().position(|&a| a == action)?;
        Some(self.keys[index].0)
    }

    /// Ação acionada pela tecla, se houver.
    pub fn action_for(&self, code: KeyCode) -> Option<Action> {
        let index = self.keys.iter().position(|&(_, c)| c == code)?;
        Some(self.system.actions()[index])
    }

    /// Pares `(ação, tecla)` na ordem de [`System::actions`].
    pub fn pairs(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.system
            .actions()
            .iter()
            .zip(&self.keys)
            .map(|(action, (key, _))| (action.id(), *key))
    }

    /// Formato de linha de comando: `up=ArrowUp,down=ArrowDown,...`.
    pub fn to_arg(&self) -> String {
        self.pairs()
            .map(|(action, key)| format!("{action}={key}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn parse_arg(system: System, arg: &str) -> Result<Self, BindingError> {
        let pairs = arg
            .split(',')
            .map(|part| {
                part.split_once('=')
                    .ok_or_else(|| BindingError::Malformed(part.to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_pairs(system, pairs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMS: System = System::MasterSystem;

    #[test]
    fn defaults_are_arrows_z_x_enter() {
        let b = Bindings::defaults(SMS);
        assert_eq!(b.key(Action::Up), Some("ArrowUp"));
        assert_eq!(b.key(Action::Button1), Some("KeyZ"));
        assert_eq!(b.key(Action::Pause), Some("Enter"));
        assert_eq!(b.key(Action::Start), None);
        assert_eq!(b.action_for(KeyCode::KeyX), Some(Action::Button2));
        assert_eq!(b.action_for(KeyCode::KeyQ), None);
    }

    #[test]
    fn genesis_defaults_cover_six_buttons() {
        let b = Bindings::defaults(System::Genesis);
        assert_eq!(b.key(Action::A), Some("KeyZ"));
        assert_eq!(b.key(Action::Z), Some("KeyD"));
        assert_eq!(b.key(Action::Mode), Some("ShiftRight"));
        assert_eq!(b.action_for(KeyCode::Enter), Some(Action::Start));
        assert_eq!(b.key(Action::Button1), None);
    }

    #[test]
    fn every_system_has_valid_unique_defaults() {
        for system in System::ALL {
            assert_eq!(system.actions().len(), system.default_keys().len());
            let defaults = Bindings::defaults(system);
            let pairs: Vec<_> = defaults.pairs().collect();
            assert_eq!(Bindings::from_pairs(system, pairs), Ok(defaults));
            assert_eq!(System::from_id(system.id()), Some(system));
        }
    }

    #[test]
    fn arg_round_trips() {
        let custom = Bindings::from_pairs(
            SMS,
            [
                ("up", "KeyW"),
                ("down", "KeyS"),
                ("left", "KeyA"),
                ("right", "KeyD"),
                ("button1", "KeyJ"),
                ("button2", "KeyK"),
                ("pause", "Space"),
            ],
        )
        .expect("mapeamento válido");
        assert_eq!(
            Bindings::parse_arg(SMS, &custom.to_arg()),
            Ok(custom.clone())
        );
        assert_eq!(custom.action_for(KeyCode::KeyJ), Some(Action::Button1));
    }

    #[test]
    fn rejects_invalid_mappings() {
        let mut pairs: Vec<(&str, &str)> = Bindings::defaults(SMS).pairs().collect();
        pairs[1].1 = "ArrowUp";
        assert_eq!(
            Bindings::from_pairs(SMS, pairs.clone()),
            Err(BindingError::Duplicate("ArrowUp".to_owned()))
        );

        pairs[1].1 = "Escape";
        assert_eq!(
            Bindings::from_pairs(SMS, pairs.clone()),
            Err(BindingError::UnsupportedKey("Escape".to_owned()))
        );

        assert_eq!(
            Bindings::from_pairs(SMS, [("up", "KeyW")]),
            Err(BindingError::Missing(Action::Down))
        );
        assert_eq!(
            Bindings::from_pairs(SMS, [("jump", "KeyW")]),
            Err(BindingError::UnknownAction("jump".to_owned()))
        );
        // Ação de outro console.
        assert_eq!(
            Bindings::from_pairs(SMS, [("start", "KeyW")]),
            Err(BindingError::UnknownAction("start".to_owned()))
        );
        assert!(matches!(
            Bindings::parse_arg(SMS, "up"),
            Err(BindingError::Malformed(_))
        ));
    }
}
