//! YM2612 (OPN2). Nesta etapa só guarda os registradores e responde ao
//! status como "livre", para os drivers de som rodarem; a síntese FM vem
//! na etapa 3.

use chips::state::{Reader, StateError, Writer};

#[derive(Debug, Clone)]
pub struct Ym2612 {
    /// Dois bancos de 256 registradores (portas 0 e 1).
    regs: [[u8; 256]; 2],
    address: [u8; 2],
}

impl Default for Ym2612 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ym2612 {
    pub fn new() -> Self {
        Self {
            regs: [[0; 256]; 2],
            address: [0; 2],
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// `port`: 0/2 = endereço (banco 0/1), 1/3 = dado.
    pub fn write(&mut self, port: u16, value: u8) {
        let bank = usize::from(port >> 1) & 1;
        if port & 1 == 0 {
            self.address[bank] = value;
        } else {
            self.regs[bank][usize::from(self.address[bank])] = value;
        }
    }

    /// Status: bit 7 ocupado, bits 1–0 estouro dos timers B e A.
    pub fn read_status(&self) -> u8 {
        0
    }

    pub fn save_state(&self, w: &mut Writer) {
        w.slice(&self.regs[0]);
        w.slice(&self.regs[1]);
        w.slice(&self.address);
    }

    pub fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        r.fill(&mut self.regs[0])?;
        r.fill(&mut self.regs[1])?;
        r.fill(&mut self.address)?;
        Ok(())
    }
}
