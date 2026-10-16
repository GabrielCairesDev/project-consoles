//! Controlador de interrupções (I_STAT e I_MASK, em 0x1F801070).

/// Fontes de interrupção (bits de I_STAT). A da SIO (8) entra com esse
/// dispositivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Irq {
    Vblank = 0,
    Gpu = 1,
    Cdrom = 2,
    Dma = 3,
    Timer0 = 4,
    Timer1 = 5,
    Timer2 = 6,
    /// /ACK do controle ou do memory card (SIO0).
    Controller = 7,
    Spu = 9,
}

#[derive(Clone, Default)]
pub(crate) struct Interrupts {
    status: u16,
    mask: u16,
}

impl Interrupts {
    pub(crate) fn request(&mut self, irq: Irq) {
        self.status |= 1 << irq as u16;
    }

    /// Linha de interrupção da CPU (bit 10 do CAUSE).
    pub(crate) fn pending(&self) -> bool {
        self.status & self.mask != 0
    }

    pub(crate) fn status(&self) -> u16 {
        self.status
    }

    pub(crate) fn mask(&self) -> u16 {
        self.mask
    }

    /// Escrever em I_STAT reconhece: só os bits em 0 são apagados.
    pub(crate) fn acknowledge(&mut self, value: u16) {
        self.status &= value;
    }

    pub(crate) fn set_mask(&mut self, value: u16) {
        self.mask = value & 0x7FF;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledge_clears_only_zero_bits_and_mask_gates_the_line() {
        let mut irq = Interrupts::default();
        irq.request(Irq::Vblank);
        irq.request(Irq::Dma);
        assert!(!irq.pending(), "sem máscara");
        irq.set_mask(1 << Irq::Dma as u16);
        assert!(irq.pending());
        irq.acknowledge(!(1 << Irq::Dma as u16));
        assert!(!irq.pending());
        assert_eq!(irq.status(), 1, "o V-blank continua pendente");
    }
}
