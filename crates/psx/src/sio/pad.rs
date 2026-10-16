//! Controle digital (SCPH-1080, ID 0x5A41).

/// Botões apertados no controle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Buttons {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub square: bool,
    pub triangle: bool,
    pub l1: bool,
    pub r1: bool,
    pub l2: bool,
    pub r2: bool,
    pub start: bool,
    pub select: bool,
}

impl Buttons {
    /// Os 16 bits que o controle envia, com 0 para cada botão apertado.
    fn word(self) -> u16 {
        let bits = [
            self.select,
            false, // L3 e R3 só existem no analógico
            false,
            self.start,
            self.up,
            self.right,
            self.down,
            self.left,
            self.l2,
            self.r2,
            self.l1,
            self.r1,
            self.triangle,
            self.circle,
            self.cross,
            self.square,
        ];
        let pressed = bits
            .iter()
            .enumerate()
            .fold(0u16, |word, (bit, &on)| word | (u16::from(on) << bit));
        !pressed
    }
}

#[derive(Clone, Default)]
pub(super) struct Pad {
    pub(super) buttons: Buttons,
    /// Byte da sequência depois do endereço.
    step: u8,
}

impl Pad {
    pub(super) fn reset(&mut self) {
        self.step = 0;
    }

    /// Troca um byte; devolve a resposta e se o controle confirma (/ACK),
    /// pedindo o próximo. Só o comando 0x42 (ler botões) existe no digital.
    pub(super) fn transfer(&mut self, tx: u8) -> (u8, bool) {
        let step = self.step;
        self.step = self.step.saturating_add(1);
        let [low, high] = self.buttons.word().to_le_bytes();
        match step {
            0 if tx == 0x42 => (0x41, true),
            1 => (0x5A, true),
            2 => (low, true),
            3 => (high, false),
            _ => {
                self.step = u8::MAX;
                (0xFF, false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_buttons_sequence() {
        let mut pad = Pad {
            buttons: Buttons {
                start: true,
                cross: true,
                ..Buttons::default()
            },
            ..Pad::default()
        };
        let replies: Vec<_> = [0x42, 0, 0, 0].map(|b| pad.transfer(b)).into();
        assert_eq!(
            replies,
            [(0x41, true), (0x5A, true), (0xF7, true), (0xBF, false)]
        );
    }

    #[test]
    fn unknown_command_is_not_acknowledged() {
        let mut pad = Pad::default();
        assert_eq!(pad.transfer(0x43), (0xFF, false));
        assert_eq!(pad.transfer(0x00), (0xFF, false), "fica calado até soltar");
        pad.reset();
        assert_eq!(pad.transfer(0x42), (0x41, true));
    }
}
