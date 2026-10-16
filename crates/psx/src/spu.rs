//! SPU (0x1F801C00–0x1F801FFF).
//!
//! Por enquanto só os registradores: o SPUSTAT espelha o modo do SPUCNT,
//! que é o que o BIOS espera ao iniciar. O som vem na etapa do SPU.

const REGISTERS: usize = 0x200;
const SPUCNT: usize = 0x1AA / 2;
const SPUSTAT: usize = 0x1AE / 2;

#[derive(Clone)]
pub(crate) struct Spu {
    registers: Box<[u16; REGISTERS]>,
}

impl Default for Spu {
    fn default() -> Self {
        Self {
            registers: Box::new([0; REGISTERS]),
        }
    }
}

impl Spu {
    pub(crate) fn read16(&self, offset: u32) -> u16 {
        let index = (offset as usize / 2) % REGISTERS;
        if index == SPUSTAT {
            return self.registers[SPUCNT] & 0x3F;
        }
        self.registers[index]
    }

    pub(crate) fn write16(&mut self, offset: u32, value: u16) {
        let index = (offset as usize / 2) % REGISTERS;
        self.registers[index] = value;
    }

    /// Palavra escrita pelo DMA (canal 4).
    pub(crate) fn dma_write(&mut self, _word: u32) {}

    /// Palavra lida pelo DMA (canal 4).
    pub(crate) fn dma_read(&mut self) -> u32 {
        0
    }
}
