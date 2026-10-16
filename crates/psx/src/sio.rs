//! Porta dos controles e memory cards (SIO0, 0x1F801040–0x1F80104F).
//!
//! Por enquanto sem nada conectado: a transmissão fica sempre pronta e
//! nenhum dispositivo responde. Controle e memory card vêm na integração.

#[derive(Clone, Default)]
pub(crate) struct Sio {
    mode: u16,
    control: u16,
    baud: u16,
}

impl Sio {
    pub(crate) fn read(&self, offset: u32) -> u32 {
        match offset {
            // Sem dispositivo, o byte recebido é 0xFF.
            0x0 => 0xFF,
            // TX pronto (bits 0 e 2).
            0x4 => 0b101,
            0x8 => u32::from(self.mode),
            0xA => u32::from(self.control),
            0xE => u32::from(self.baud),
            _ => 0,
        }
    }

    pub(crate) fn write(&mut self, offset: u32, value: u32) {
        match offset {
            0x8 => self.mode = value as u16,
            0xA => self.control = value as u16,
            0xE => self.baud = value as u16,
            _ => {}
        }
    }
}
