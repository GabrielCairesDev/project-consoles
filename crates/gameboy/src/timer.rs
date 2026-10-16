//! Timer: DIV, TIMA, TMA e TAC.
//!
//! O DIV é o byte alto de um contador interno de 16 bits que anda a cada
//! ciclo (T). O TIMA conta na borda de descida de um bit desse contador
//! (escolhido pelo TAC) com o timer ligado; por isso escrever no DIV ou no
//! TAC pode contar um passo extra, como no hardware.

use chips::state::{Reader, StateError, Writer};

/// Contador interno logo após a boot ROM do DMG (DIV = 0xAB). É 4 ciclos
/// antes do valor medido no PC 0x100 (0xABCC) porque aqui o acesso da CPU
/// acontece no fim do M-ciclo; validado pelo `boot_div` do Mooneye.
const POST_BOOT_COUNTER: u16 = 0xABC8;

#[derive(Debug, Clone)]
pub struct Timer {
    counter: u16,
    tima: u8,
    tma: u8,
    tac: u8,
    /// O TIMA estourou no ciclo anterior: neste ciclo recebe o TMA.
    overflow: bool,
    /// Ciclo em que o TMA está sendo carregado: escrita no TIMA é ignorada
    /// e escrita no TMA também vai para o TIMA.
    reloading: bool,
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            counter: POST_BOOT_COUNTER,
            tima: 0,
            tma: 0,
            tac: 0xF8,
            overflow: false,
            reloading: false,
        }
    }
}

impl Timer {
    /// Bit do contador observado, combinado com o timer ligado.
    fn signal(&self) -> bool {
        const BITS: [u16; 4] = [9, 3, 5, 7];
        self.tac & 0x04 != 0 && self.counter >> BITS[usize::from(self.tac & 3)] & 1 != 0
    }

    fn increment(&mut self) {
        let (value, overflowed) = self.tima.overflowing_add(1);
        self.tima = value;
        if overflowed {
            self.overflow = true;
        }
    }

    /// Avança um M-ciclo (4 T); devolve se a interrupção do timer ocorreu.
    pub fn tick(&mut self) -> bool {
        let mut interrupt = false;
        self.reloading = false;
        if self.overflow {
            self.overflow = false;
            self.tima = self.tma;
            self.reloading = true;
            interrupt = true;
        }
        for _ in 0..4 {
            let before = self.signal();
            self.counter = self.counter.wrapping_add(1);
            if before && !self.signal() {
                self.increment();
            }
        }
        interrupt
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.counter >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            _ => self.tac | 0xF8,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF04 => {
                let before = self.signal();
                self.counter = 0;
                if before {
                    self.increment();
                }
            }
            0xFF05 => {
                if !self.reloading {
                    self.tima = value;
                    // Escrever durante o ciclo do estouro cancela a recarga.
                    self.overflow = false;
                }
            }
            0xFF06 => {
                self.tma = value;
                if self.reloading {
                    self.tima = value;
                }
            }
            _ => {
                let before = self.signal();
                self.tac = value | 0xF8;
                if before && !self.signal() {
                    self.increment();
                }
            }
        }
    }

    /// Contador interno (o DIV é o byte alto); a APU usa o bit 12.
    pub fn counter(&self) -> u16 {
        self.counter
    }

    /// STOP zera o divisor.
    pub fn reset_divider(&mut self) {
        self.write(0xFF04, 0);
    }

    pub fn save_state(&self, w: &mut Writer) {
        w.u16(self.counter);
        w.u8(self.tima);
        w.u8(self.tma);
        w.u8(self.tac);
        w.bool(self.overflow);
        w.bool(self.reloading);
    }

    pub fn load_state(&mut self, r: &mut Reader) -> Result<(), StateError> {
        *self = Self {
            counter: r.u16()?,
            tima: r.u8()?,
            tma: r.u8()?,
            tac: r.u8()? | 0xF8,
            overflow: r.bool()?,
            reloading: r.bool()?,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer(tac: u8) -> Timer {
        let mut t = Timer {
            counter: 0,
            ..Timer::default()
        };
        t.write(0xFF07, tac);
        t
    }

    #[test]
    fn div_is_upper_byte_of_counter() {
        let mut t = timer(0);
        for _ in 0..64 {
            t.tick();
        }
        assert_eq!(t.read(0xFF04), 1);
        t.write(0xFF04, 0x55);
        assert_eq!(t.read(0xFF04), 0);
    }

    #[test]
    fn tac_selects_frequency() {
        // 01 = a cada 16 T (4 M-ciclos).
        let mut t = timer(0x05);
        for _ in 0..40 {
            t.tick();
        }
        assert_eq!(t.read(0xFF05), 10);
        // 00 = a cada 1024 T.
        let mut t = timer(0x04);
        for _ in 0..256 {
            t.tick();
        }
        assert_eq!(t.read(0xFF05), 1);
        // Desligado não conta.
        let mut t = timer(0x01);
        for _ in 0..40 {
            t.tick();
        }
        assert_eq!(t.read(0xFF05), 0);
    }

    #[test]
    fn overflow_reloads_one_cycle_later_with_interrupt() {
        let mut t = timer(0x05);
        t.write(0xFF06, 0x80);
        t.write(0xFF05, 0xFF);
        let mut cycles = 0;
        while t.read(0xFF05) == 0xFF {
            assert!(!t.tick());
            cycles += 1;
        }
        assert_eq!(t.read(0xFF05), 0, "fica em 0 por um ciclo");
        assert!(cycles <= 4);
        assert!(t.tick(), "interrupção na recarga");
        assert_eq!(t.read(0xFF05), 0x80);
    }

    #[test]
    fn writing_tima_during_overflow_cancels_reload() {
        let mut t = timer(0x05);
        t.write(0xFF05, 0xFF);
        while t.read(0xFF05) != 0 {
            t.tick();
        }
        t.write(0xFF05, 0x10);
        assert!(!t.tick());
        assert_eq!(t.read(0xFF05), 0x10);
    }

    #[test]
    fn writing_tma_during_reload_also_sets_tima() {
        let mut t = timer(0x05);
        t.write(0xFF05, 0xFF);
        while t.read(0xFF05) != 0 {
            t.tick();
        }
        assert!(t.tick());
        t.write(0xFF05, 0x33);
        assert_eq!(t.read(0xFF05), 0, "TIMA ignorado no ciclo da recarga");
        t.write(0xFF06, 0x44);
        assert_eq!(t.read(0xFF05), 0x44);
    }

    #[test]
    fn div_reset_on_falling_edge_counts_once() {
        let mut t = timer(0x05);
        // Bit 3 ligado: zerar o DIV gera a borda de descida.
        t.counter = 0x0008;
        t.write(0xFF04, 0);
        assert_eq!(t.read(0xFF05), 1);
    }
}
