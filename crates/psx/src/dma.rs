//! Registradores do controlador de DMA (0x1F801080–0x1F8010F7).
//!
//! As transferências são feitas pelo barramento ([`crate::bus::Bus`]), que
//! tem acesso à RAM e aos dispositivos.

/// Canais na ordem do hardware.
pub(crate) const GPU: usize = 2;
pub(crate) const CDROM: usize = 3;
pub(crate) const SPU: usize = 4;
pub(crate) const OTC: usize = 6;

const CHCR_START: u32 = 1 << 24;
const CHCR_TRIGGER: u32 = 1 << 28;

#[derive(Clone, Copy, Default)]
pub(crate) struct Channel {
    pub(crate) madr: u32,
    pub(crate) bcr: u32,
    pub(crate) chcr: u32,
}

impl Channel {
    /// 0 = de uma vez, 1 = em blocos, 2 = lista ligada.
    pub(crate) fn sync_mode(&self) -> u32 {
        (self.chcr >> 9) & 3
    }

    pub(crate) fn reads_ram(&self) -> bool {
        self.chcr & 1 != 0
    }

    pub(crate) fn decrement(&self) -> bool {
        self.chcr & 2 != 0
    }

    /// Palavras a transferir nos modos 0 e 1.
    pub(crate) fn word_count(&self) -> u32 {
        let block = match self.bcr & 0xFFFF {
            0 => 0x1_0000,
            n => n,
        };
        if self.sync_mode() == 0 {
            block
        } else {
            block * (self.bcr >> 16)
        }
    }
}

#[derive(Clone)]
pub(crate) struct Dma {
    pub(crate) channels: [Channel; 7],
    dpcr: u32,
    dicr: u32,
}

impl Default for Dma {
    fn default() -> Self {
        Self {
            channels: [Channel::default(); 7],
            dpcr: 0x0765_4321,
            dicr: 0,
        }
    }
}

impl Dma {
    fn master_irq(&self) -> bool {
        let flags = (self.dicr >> 24) & 0x7F;
        let enabled = (self.dicr >> 16) & 0x7F;
        self.dicr & (1 << 15) != 0 || (self.dicr & (1 << 23) != 0 && flags & enabled != 0)
    }

    fn with_master_bit(&self) -> u32 {
        (self.dicr & 0x7FFF_FFFF) | (u32::from(self.master_irq()) << 31)
    }

    /// O canal pediu para começar e está habilitado no DPCR.
    fn ready(&self, ch: usize) -> bool {
        let c = &self.channels[ch];
        let enabled = self.dpcr & (8 << (ch * 4)) != 0;
        // No modo 0 o trigger dá a partida, a não ser que o dispositivo
        // peça a transferência sozinho (o SPU em modo DMA).
        let trigger = c.sync_mode() != 0 || c.chcr & CHCR_TRIGGER != 0 || ch == SPU;
        enabled && c.chcr & CHCR_START != 0 && trigger
    }

    pub(crate) fn read(&self, offset: u32) -> u32 {
        let ch = (offset >> 4) as usize;
        match (ch, offset & 0xF) {
            (7, 0x0) => self.dpcr,
            (7, 0x4) => self.with_master_bit(),
            (0..=6, 0x0) => self.channels[ch].madr,
            (0..=6, 0x4) => self.channels[ch].bcr,
            (0..=6, 0x8) => self.channels[ch].chcr,
            _ => 0,
        }
    }

    /// Escreve um registrador. Devolve o canal que deve transferir agora e
    /// se a linha de interrupção subiu.
    pub(crate) fn write(&mut self, offset: u32, value: u32) -> (Option<usize>, bool) {
        let ch = (offset >> 4) as usize;
        let before = self.master_irq();
        match (ch, offset & 0xF) {
            (7, 0x0) => self.dpcr = value,
            (7, 0x4) => {
                let flags = (self.dicr & 0x7F00_0000) & !(value & 0x7F00_0000);
                self.dicr = (value & 0x00FF_803F) | flags;
            }
            (0..=6, 0x0) => self.channels[ch].madr = value & 0x00FF_FFFF,
            (0..=6, 0x4) => self.channels[ch].bcr = value,
            (OTC, 0x8) => {
                // O canal da tabela de ordenação só anda para trás.
                self.channels[OTC].chcr = (value & (CHCR_START | CHCR_TRIGGER | 1 << 30)) | 2;
            }
            (0..=5, 0x8) => self.channels[ch].chcr = value & 0x7177_0703,
            _ => {}
        }
        let rising = !before && self.master_irq();
        let start = match (ch, offset & 0xF) {
            (0..=6, 0x8) if self.ready(ch) => Some(ch),
            (7, 0x0) => (0..7).find(|&c| self.ready(c)),
            _ => None,
        };
        (start, rising)
    }

    /// Fim da transferência; `true` se a linha de interrupção subiu.
    pub(crate) fn finish(&mut self, ch: usize) -> bool {
        let before = self.master_irq();
        self.channels[ch].chcr &= !(CHCR_START | CHCR_TRIGGER);
        if self.dicr & (1 << (16 + ch)) != 0 {
            self.dicr |= 1 << (24 + ch);
        }
        !before && self.master_irq()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_channel_needs_trigger_and_enable() {
        let mut dma = Dma::default();
        dma.write(0x60, 0x100);
        dma.write(0x64, 4);
        assert_eq!(dma.write(0x68, CHCR_START).0, None, "modo 0 sem trigger");
        assert_eq!(
            dma.write(0x68, CHCR_START | CHCR_TRIGGER).0,
            None,
            "desabilitado"
        );
        dma.write(0x70, 0x0800_0000);
        assert_eq!(dma.write(0x68, CHCR_START | CHCR_TRIGGER).0, Some(OTC));
    }

    #[test]
    fn spu_starts_in_mode_0_without_trigger() {
        let mut dma = Dma::default();
        dma.write(0x70, 0x0008_0000);
        assert_eq!(dma.write(0x48, CHCR_START | 1).0, Some(SPU));
    }

    #[test]
    fn irq_flag_raises_master_once_and_acknowledges() {
        let mut dma = Dma::default();
        dma.write(0x74, (1 << 23) | (1 << (16 + GPU)));
        assert!(dma.finish(GPU));
        assert!(!dma.finish(GPU), "já estava alta");
        assert_ne!(dma.read(0x74) >> 31, 0);
        dma.write(0x74, (1 << 23) | (1 << (16 + GPU)) | (1 << (24 + GPU)));
        assert_eq!(dma.read(0x74) >> 31, 0);
    }

    #[test]
    fn block_word_count() {
        let c = Channel {
            madr: 0,
            bcr: (3 << 16) | 0x10,
            chcr: 1 << 9,
        };
        assert_eq!(c.word_count(), 48);
    }
}
