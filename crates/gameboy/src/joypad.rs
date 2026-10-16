//! Registrador P1 (FF00): o jogo escolhe entre ler as setas e os botões.

use chips::state::{Reader, StateError, Writer};

/// Botões apertados.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Buttons {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub a: bool,
    pub b: bool,
    pub start: bool,
    pub select: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Joypad {
    pub buttons: Buttons,
    /// Bits 4 e 5 de P1 (0 = linha selecionada); a boot ROM deixa as duas
    /// selecionadas.
    select: u8,
}

impl Joypad {
    /// Bits 0–3 com 0 para cada botão apertado nas linhas selecionadas.
    fn lines(&self) -> u8 {
        let b = &self.buttons;
        let mut low = 0x0F;
        let pressed = |on: bool, bit: u8| if on { 1 << bit } else { 0 };
        if self.select & 0x10 == 0 {
            low &=
                !(pressed(b.right, 0) | pressed(b.left, 1) | pressed(b.up, 2) | pressed(b.down, 3));
        }
        if self.select & 0x20 == 0 {
            low &=
                !(pressed(b.a, 0) | pressed(b.b, 1) | pressed(b.select, 2) | pressed(b.start, 3));
        }
        low
    }

    pub fn read(&self) -> u8 {
        0xC0 | self.select | self.lines()
    }

    /// Escreve a seleção; devolve se alguma linha caiu (interrupção).
    pub fn write(&mut self, value: u8) -> bool {
        let before = self.lines();
        self.select = value & 0x30;
        before & !self.lines() != 0
    }

    /// Troca os botões; devolve se alguma linha caiu (interrupção).
    pub fn set(&mut self, buttons: Buttons) -> bool {
        let before = self.lines();
        self.buttons = buttons;
        before & !self.lines() != 0
    }

    pub fn save_state(&self, w: &mut Writer) {
        w.u8(self.select);
    }

    pub fn load_state(&mut self, r: &mut Reader) -> Result<(), StateError> {
        self.select = r.u8()? & 0x30;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_row_reports_pressed_buttons_as_zero() {
        let mut pad = Joypad::default();
        pad.set(Buttons {
            right: true,
            start: true,
            ..Buttons::default()
        });
        pad.write(0x30);
        assert_eq!(pad.read() & 0x0F, 0x0F, "nada selecionado");
        pad.write(0x20); // setas
        assert_eq!(pad.read(), 0xE0 | 0x0E);
        pad.write(0x10); // botões
        assert_eq!(pad.read(), 0xD0 | 0x07);
    }

    #[test]
    fn falling_line_requests_interrupt() {
        let mut pad = Joypad::default();
        pad.write(0x10);
        assert!(pad.set(Buttons {
            a: true,
            ..Buttons::default()
        }));
        assert!(!pad.set(Buttons {
            a: true,
            ..Buttons::default()
        }));
        assert!(
            !pad.set(Buttons {
                up: true,
                ..Buttons::default()
            }),
            "setas não estão selecionadas"
        );
    }
}
