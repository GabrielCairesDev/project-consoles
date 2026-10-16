//! Portas de controle (A10003/A10005) com controle de 3 ou 6 botões.
//!
//! O jogo alterna a linha TH (bit 6) e lê grupos de botões diferentes em
//! cada fase. O controle de 6 botões conta as bordas de descida de TH: na
//! terceira, informa sua presença (bits 0–3 em 0) e, na fase seguinte,
//! devolve X, Y, Z e Mode. Sem atividade por ~1,5 ms, a sequência reinicia.

use chips::state::{Reader, StateError, Writer};

/// Ciclos do 68000 sem mudança em TH até o controle de 6 botões reiniciar.
const SIX_BUTTON_TIMEOUT: u32 = 11_500;

/// Botões pressionados no controle do jogador 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pad {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub a: bool,
    pub b: bool,
    pub c: bool,
    pub start: bool,
    pub x: bool,
    pub y: bool,
    pub z: bool,
    pub mode: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Port {
    pub pad: Pad,
    /// Controle de 6 botões (o de 3 ignora o contador).
    pub six_button: bool,
    connected: bool,
    data: u8,
    control: u8,
    /// Bordas de descida de TH desde o último reinício (0–3).
    phase: u8,
    idle_cycles: u32,
}

impl Port {
    pub fn new(connected: bool) -> Self {
        Self {
            pad: Pad::default(),
            six_button: true,
            connected,
            data: 0x7F,
            control: 0,
            phase: 0,
            idle_cycles: 0,
        }
    }

    fn th(&self) -> bool {
        // TH configurado como saída usa o valor escrito; como entrada, fica em 1.
        if self.control & 0x40 != 0 {
            self.data & 0x40 != 0
        } else {
            true
        }
    }

    pub fn write_data(&mut self, value: u8) {
        let before = self.th();
        self.data = value;
        self.th_changed(before);
    }

    pub fn write_control(&mut self, value: u8) {
        let before = self.th();
        self.control = value;
        self.th_changed(before);
    }

    fn th_changed(&mut self, before: bool) {
        let after = self.th();
        if before && !after && self.six_button {
            self.phase = (self.phase + 1).min(4);
        }
        if before != after {
            self.idle_cycles = 0;
        }
    }

    pub fn control(&self) -> u8 {
        self.control
    }

    /// Avança o temporizador do controle de 6 botões.
    pub fn tick(&mut self, cpu_cycles: u32) {
        self.idle_cycles = self.idle_cycles.saturating_add(cpu_cycles);
        if self.idle_cycles >= SIX_BUTTON_TIMEOUT {
            self.phase = 0;
        }
    }

    pub fn read_data(&self) -> u8 {
        let th = self.th();
        // Bits ativos em 0; o bit 7 e os bits configurados como saída
        // devolvem o que foi escrito.
        let pressed = |on: bool, bit: u8| if on { 0 } else { bit };
        let p = &self.pad;
        let input = if !self.connected {
            0x7F
        } else if th {
            if self.phase == 3 {
                // Fase extra do 6 botões: Z, Y, X, Mode nos bits 0–3.
                0x40 | pressed(p.z, 1)
                    | pressed(p.y, 2)
                    | pressed(p.x, 4)
                    | pressed(p.mode, 8)
                    | pressed(p.b, 0x10)
                    | pressed(p.c, 0x20)
            } else {
                0x40 | pressed(p.up, 1)
                    | pressed(p.down, 2)
                    | pressed(p.left, 4)
                    | pressed(p.right, 8)
                    | pressed(p.b, 0x10)
                    | pressed(p.c, 0x20)
            }
        } else if self.phase == 3 && self.six_button {
            // Presença do 6 botões: direcionais em 0.
            pressed(p.a, 0x10) | pressed(p.start, 0x20)
        } else if self.phase >= 4 && self.six_button {
            0x0F | pressed(p.a, 0x10) | pressed(p.start, 0x20)
        } else {
            pressed(p.up, 1) | pressed(p.down, 2) | pressed(p.a, 0x10) | pressed(p.start, 0x20)
        };
        let outputs = self.control & 0x7F;
        (self.data & (0x80 | outputs)) | (input & !outputs & 0x7F)
    }

    pub fn save_state(&self, w: &mut Writer) {
        w.u8(self.data);
        w.u8(self.control);
        w.u8(self.phase);
        w.u32(self.idle_cycles);
    }

    pub fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        self.data = r.u8()?;
        self.control = r.u8()?;
        self.phase = r.u8()?.min(4);
        self.idle_cycles = r.u32()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port() -> Port {
        let mut p = Port::new(true);
        p.write_control(0x40); // TH como saída
        p
    }

    #[test]
    fn three_button_phases() {
        let mut p = port();
        p.six_button = false;
        p.pad = Pad {
            up: true,
            b: true,
            a: true,
            start: true,
            ..Pad::default()
        };
        p.write_data(0x40);
        assert_eq!(p.read_data() & 0x3F, 0x3F & !(0x01 | 0x10));
        p.write_data(0x00);
        // TH = 0: Up (apertado = 0), Down (2), bits 2–3 em 0, A e Start apertados.
        assert_eq!(p.read_data() & 0x3F, 0x02);
    }

    #[test]
    fn six_button_reports_presence_then_extra_buttons() {
        let mut p = port();
        p.pad = Pad {
            x: true,
            mode: true,
            ..Pad::default()
        };
        for _ in 0..2 {
            p.write_data(0x40);
            p.write_data(0x00);
        }
        p.write_data(0x40);
        p.write_data(0x00);
        // Terceira descida: direcionais em 0 indicam 6 botões.
        assert_eq!(p.read_data() & 0x0F, 0);
        p.write_data(0x40);
        assert_eq!(p.read_data() & 0x0F, 0x0F & !(0x04 | 0x08));
    }

    #[test]
    fn six_button_sequence_times_out() {
        let mut p = port();
        for _ in 0..3 {
            p.write_data(0x40);
            p.write_data(0x00);
        }
        p.tick(SIX_BUTTON_TIMEOUT);
        p.write_data(0x40);
        p.write_data(0x00);
        // Depois do tempo limite, volta à fase normal (Up/Down visíveis).
        assert_eq!(p.read_data() & 0x03, 0x03);
    }

    #[test]
    fn disconnected_port_reads_all_released() {
        let p = Port::new(false);
        assert_eq!(p.read_data() & 0x7F, 0x7F);
    }
}
