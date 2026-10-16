//! Controlador do CD-ROM (0x1F801800–0x1F801803).
//!
//! Por enquanto só o registrador de índice e status, sem disco: as filas
//! vazias e nada ocupado. O controlador completo vem na etapa do CD.

#[derive(Clone, Default)]
pub(crate) struct Cdrom {
    index: u8,
}

impl Cdrom {
    pub(crate) fn read(&mut self, offset: u32) -> u8 {
        match offset {
            // Índice, fila de parâmetros vazia e com espaço.
            0 => self.index | 0x18,
            _ => 0,
        }
    }

    pub(crate) fn write(&mut self, offset: u32, value: u8) {
        if offset == 0 {
            self.index = value & 3;
        }
    }

    /// Palavra lida pelo DMA (canal 3).
    pub(crate) fn dma_read(&mut self) -> u32 {
        0
    }
}
