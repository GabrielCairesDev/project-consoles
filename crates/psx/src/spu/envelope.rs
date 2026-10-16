//! Gerador de envelope, usado pelo ADSR das vozes e pelos volumes com
//! sweep. Avança uma vez por amostra (44,1 kHz).

/// Velocidade e forma de um trecho do envelope.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) struct Rate {
    pub(super) exponential: bool,
    pub(super) decreasing: bool,
    /// Fase negativa (só nos volumes com sweep): inverte o sentido.
    pub(super) negative: bool,
    /// 0..31: rápido..lento.
    pub(super) shift: u8,
    /// 0..3: passos +7..+4 (ou -8..-5 ao diminuir).
    pub(super) step: u8,
}

impl Rate {
    /// Avança `level` uma amostra; `counter` acumula até o próximo passo.
    pub(super) fn tick(self, level: i16, counter: &mut u32) -> i16 {
        let shift = u32::from(self.shift);
        let mut step = 7 - i32::from(self.step);
        if self.decreasing != self.negative {
            step = !step;
        }
        step <<= 11u32.saturating_sub(shift);
        let mut increment = 0x8000u32 >> shift.saturating_sub(11);
        let level = i32::from(level);
        if self.exponential && !self.decreasing && level > 0x6000 {
            // O "exponencial" ao subir é só um trecho linear mais lento.
            match shift {
                0..=9 => step >>= 2,
                10 => {
                    step >>= 1;
                    increment >>= 1;
                }
                _ => increment >>= 2,
            }
        } else if self.exponential && self.decreasing {
            step = (step * level) >> 15;
        }
        // Com todos os bits ligados, o envelope nunca anda.
        if u32::from(self.step) | shift << 2 != 0x7F {
            increment = increment.max(1);
        }
        *counter += increment;
        if *counter & 0x8000 == 0 {
            return level as i16;
        }
        *counter = 0;
        let new = level + step;
        let new = if !self.decreasing {
            new.clamp(-0x8000, 0x7FFF)
        } else if self.negative {
            new.clamp(-0x8000, 0)
        } else {
            new.clamp(0, 0x7FFF)
        };
        new as i16
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Envelope ADSR de uma voz (registradores ADSR1 e ADSR2).
#[derive(Clone, Copy, Default)]
pub(super) struct Adsr {
    /// ADSR1 nos bits 0–15, ADSR2 nos bits 16–31.
    pub(super) config: u32,
    pub(super) level: i16,
    pub(super) phase: Phase,
    counter: u32,
}

impl Adsr {
    pub(super) fn key_on(&mut self) {
        self.phase = Phase::Attack;
        self.level = 0;
        self.counter = 0;
    }

    pub(super) fn key_off(&mut self) {
        if self.phase != Phase::Off {
            self.phase = Phase::Release;
            self.counter = 0;
        }
    }

    /// Fim de um sample sem repetição: silencia na hora.
    pub(super) fn silence(&mut self) {
        self.phase = Phase::Release;
        self.level = 0;
        self.counter = 0;
    }

    fn sustain_level(&self) -> i16 {
        (((self.config & 0xF) + 1) * 0x800).min(0x7FFF) as i16
    }

    fn rate(&self) -> Rate {
        let c = self.config;
        let bits = |shift: u32, mask: u32| ((c >> shift) & mask) as u8;
        match self.phase {
            Phase::Attack => Rate {
                exponential: c & 0x8000 != 0,
                shift: bits(10, 0x1F),
                step: bits(8, 3),
                ..Rate::default()
            },
            Phase::Decay => Rate {
                exponential: true,
                decreasing: true,
                shift: bits(4, 0xF),
                ..Rate::default()
            },
            Phase::Sustain => Rate {
                exponential: c & (1 << 31) != 0,
                decreasing: c & (1 << 30) != 0,
                shift: bits(24, 0x1F),
                step: bits(22, 3),
                ..Rate::default()
            },
            Phase::Release | Phase::Off => Rate {
                exponential: c & (1 << 21) != 0,
                decreasing: true,
                shift: bits(16, 0x1F),
                ..Rate::default()
            },
        }
    }

    pub(super) fn tick(&mut self) {
        if self.phase == Phase::Off {
            return;
        }
        self.level = self.rate().tick(self.level, &mut self.counter);
        let next = match self.phase {
            Phase::Attack if self.level == 0x7FFF => Phase::Decay,
            Phase::Decay if self.level <= self.sustain_level() => Phase::Sustain,
            Phase::Release if self.level == 0 => Phase::Off,
            phase => phase,
        };
        if next != self.phase {
            self.phase = next;
            self.counter = 0;
        }
    }
}

/// Volume de um lado: fixo ou com sweep (bit 15).
#[derive(Clone, Copy, Default)]
pub(super) struct Volume {
    pub(super) setting: u16,
    pub(super) level: i16,
    counter: u32,
}

impl Volume {
    pub(super) fn set(&mut self, value: u16) {
        self.setting = value;
        self.counter = 0;
        if value & 0x8000 == 0 {
            // Bits 0–14 com sinal, valendo o dobro.
            self.level = (value << 1) as i16;
        }
    }

    pub(super) fn tick(&mut self) {
        let v = self.setting;
        if v & 0x8000 == 0 {
            return;
        }
        let rate = Rate {
            exponential: v & 0x4000 != 0,
            decreasing: v & 0x2000 != 0,
            negative: v & 0x1000 != 0,
            shift: ((v >> 2) & 0x1F) as u8,
            step: (v & 3) as u8,
        };
        self.level = rate.tick(self.level, &mut self.counter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(adsr: &mut Adsr, ticks: usize) {
        for _ in 0..ticks {
            adsr.tick();
        }
    }

    #[test]
    fn fastest_linear_attack_steps_every_sample() {
        let rate = Rate::default();
        let mut counter = 0;
        assert_eq!(rate.tick(0, &mut counter), 7 << 11);
        assert_eq!(rate.tick(0x7000, &mut counter), 0x7FFF, "satura");
    }

    #[test]
    fn slow_rates_step_less_often() {
        // Shift 13: um passo de 7 a cada 4 amostras.
        let rate = Rate {
            shift: 13,
            ..Rate::default()
        };
        let mut counter = 0;
        let levels: Vec<i16> = (0..8)
            .scan(0, |level, _| {
                *level = rate.tick(*level, &mut counter);
                Some(*level)
            })
            .collect();
        assert_eq!(levels, [0, 0, 0, 7, 7, 7, 7, 14]);
    }

    #[test]
    fn all_bits_set_never_steps() {
        let rate = Rate {
            shift: 0x1F,
            step: 3,
            decreasing: true,
            ..Rate::default()
        };
        let mut counter = 0;
        for _ in 0..100_000 {
            assert_eq!(rate.tick(0x1234, &mut counter), 0x1234);
        }
    }

    #[test]
    fn exponential_decay_reaches_zero() {
        let rate = Rate {
            exponential: true,
            decreasing: true,
            ..Rate::default()
        };
        let mut counter = 0;
        let mut level = 0x7FFF;
        let mut steps = 0;
        while level > 0 {
            level = rate.tick(level, &mut counter);
            steps += 1;
            assert!(steps < 10_000);
        }
    }

    #[test]
    fn adsr_goes_through_its_phases() {
        // Ataque rápido, decay rápido até o nível 0x4000, sustain parado e
        // release rápido.
        let mut adsr = Adsr {
            config: 0x0007 | (0x1FC0 << 16),
            ..Adsr::default()
        };
        adsr.key_on();
        run(&mut adsr, 2);
        assert_eq!((adsr.phase, adsr.level), (Phase::Attack, 0x7000));
        run(&mut adsr, 1);
        assert_eq!((adsr.phase, adsr.level), (Phase::Decay, 0x7FFF));
        run(&mut adsr, 100);
        assert_eq!(adsr.phase, Phase::Sustain);
        let sustain = adsr.level;
        assert!(sustain <= 0x4000 && sustain > 0x3000, "{sustain:#x}");
        run(&mut adsr, 1000);
        assert_eq!(adsr.level, sustain, "sustain com taxa parada");
        adsr.key_off();
        run(&mut adsr, 10);
        assert_eq!((adsr.phase, adsr.level), (Phase::Off, 0));
    }

    #[test]
    fn volume_direct_and_sweep() {
        let mut volume = Volume::default();
        volume.set(0x3FFF);
        assert_eq!(volume.level, 0x7FFE);
        volume.set(0x4000);
        assert_eq!(volume.level, -0x8000);
        volume.set(0x0000);
        // Sweep linear crescente, o mais rápido.
        volume.set(0x8000);
        volume.tick();
        assert_eq!(volume.level, 7 << 11);
    }
}
