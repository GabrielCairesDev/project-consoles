//! PSG SN76489 do Master System: três canais de tom e um de ruído.
//!
//! O chip roda a 1/16 do clock da CPU. A saída é reamostrada para a taxa do
//! dispositivo de áudio pela média das amostras internas de cada período
//! (filtro de caixa), o que já elimina o grosso do aliasing dos tons agudos.

use crate::state::{Reader, StateError, Writer};

/// Valor inicial do LFSR e bit de saída do ruído (16 bits no SMS).
const LFSR_RESET: u16 = 0x8000;
/// Volume máximo por canal; quatro canais somados ficam abaixo de 1.0.
const CHANNEL_AMPLITUDE: f32 = 0.25;

#[derive(Debug, Clone)]
pub struct Psg {
    /// Períodos de 10 bits dos tons; o índice 3 guarda o registro de ruído.
    tone: [u16; 3],
    noise: u8,
    volume: [u8; 4],
    counter: [u16; 4],
    output: [bool; 4],
    lfsr: u16,
    latched_channel: usize,
    latched_volume: bool,
    /// Divisor do clock da CPU (16).
    clock_divider: u32,
    cpu_cycles: u32,
    /// Reamostragem: soma e contagem das amostras internas do período atual.
    acc_sum: f32,
    acc_count: u32,
    /// Ticks internos por amostra de saída, em ponto fixo 32.32.
    step: u64,
    phase: u64,
    samples: Vec<f32>,
    volume_table: [f32; 16],
}

impl Psg {
    pub fn new(cpu_clock: u32, sample_rate: u32) -> Self {
        let chip_clock = u64::from(cpu_clock / 16);
        let step = (chip_clock << 32) / u64::from(sample_rate.max(1));
        // Atenuação de 2 dB por passo; 0xF é silêncio.
        let mut volume_table = [0.0; 16];
        for (i, v) in volume_table.iter_mut().enumerate().take(15) {
            *v = CHANNEL_AMPLITUDE * 10f32.powf(-(i as f32) * 2.0 / 20.0);
        }
        Self {
            tone: [0; 3],
            noise: 0,
            volume: [0x0F; 4],
            counter: [0; 4],
            output: [false; 4],
            lfsr: LFSR_RESET,
            latched_channel: 0,
            latched_volume: false,
            clock_divider: 16,
            cpu_cycles: 0,
            acc_sum: 0.0,
            acc_count: 0,
            step,
            phase: 0,
            samples: Vec::new(),
            volume_table,
        }
    }

    pub fn write(&mut self, v: u8) {
        if v & 0x80 != 0 {
            self.latched_channel = usize::from((v >> 5) & 3);
            self.latched_volume = v & 0x10 != 0;
            let data = v & 0x0F;
            if self.latched_volume {
                self.volume[self.latched_channel] = data;
            } else if self.latched_channel == 3 {
                self.set_noise(data);
            } else {
                let t = &mut self.tone[self.latched_channel];
                *t = (*t & 0x3F0) | u16::from(data);
            }
        } else if self.latched_volume {
            self.volume[self.latched_channel] = v & 0x0F;
        } else if self.latched_channel == 3 {
            self.set_noise(v & 0x0F);
        } else {
            let t = &mut self.tone[self.latched_channel];
            *t = (*t & 0x00F) | (u16::from(v & 0x3F) << 4);
        }
    }

    fn set_noise(&mut self, v: u8) {
        self.noise = v & 0x07;
        self.lfsr = LFSR_RESET;
    }

    /// Avança o chip pelo número de ciclos de CPU decorridos.
    pub fn run(&mut self, cpu_cycles: u32) {
        self.cpu_cycles += cpu_cycles;
        while self.cpu_cycles >= self.clock_divider {
            self.cpu_cycles -= self.clock_divider;
            self.tick();
        }
    }

    fn tick(&mut self) {
        for ch in 0..3 {
            if self.counter[ch] > 0 {
                self.counter[ch] -= 1;
            }
            if self.counter[ch] == 0 {
                self.counter[ch] = self.tone[ch];
                // Períodos 0 e 1 deixam a saída fixa em nível alto (usado
                // para reproduzir amostras PCM pelo volume).
                self.output[ch] = self.tone[ch] <= 1 || !self.output[ch];
            }
        }

        if self.counter[3] > 0 {
            self.counter[3] -= 1;
        }
        if self.counter[3] == 0 {
            self.counter[3] = match self.noise & 3 {
                0 => 0x10,
                1 => 0x20,
                2 => 0x40,
                _ => self.tone[2].max(1),
            };
            self.output[3] = !self.output[3];
            // O LFSR avança na borda de subida do divisor do ruído.
            if self.output[3] {
                let white = self.noise & 0x04 != 0;
                let feedback = if white {
                    (self.lfsr ^ (self.lfsr >> 3)) & 1
                } else {
                    self.lfsr & 1
                };
                self.lfsr = (self.lfsr >> 1) | (feedback << 15);
            }
        }

        let tones: f32 = (0..3)
            .filter(|&ch| self.output[ch])
            .map(|ch| self.volume_table[usize::from(self.volume[ch])])
            .sum();
        let noise = if self.lfsr & 1 != 0 {
            self.volume_table[usize::from(self.volume[3])]
        } else {
            0.0
        };
        self.acc_sum += tones + noise;
        self.acc_count += 1;

        self.phase += 1 << 32;
        if self.phase >= self.step {
            self.phase -= self.step;
            self.samples.push(self.acc_sum / self.acc_count as f32);
            self.acc_sum = 0.0;
            self.acc_count = 0;
        }
    }

    /// Move as amostras geradas (mono, `f32` em 0.0..1.0) para `out`.
    pub fn drain(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.samples);
    }
}

impl Psg {
    pub(crate) fn save_state(&self, w: &mut Writer) {
        for v in self.tone {
            w.u16(v);
        }
        w.u8(self.noise);
        w.slice(&self.volume);
        for v in self.counter {
            w.u16(v);
        }
        for v in self.output {
            w.bool(v);
        }
        w.u16(self.lfsr);
        w.u8(self.latched_channel as u8);
        w.bool(self.latched_volume);
        w.u32(self.cpu_cycles);
    }

    /// O estado do reamostrador não é salvo: recomeça do zero sem efeito
    /// audível.
    pub(crate) fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        for v in &mut self.tone {
            *v = r.u16()? & 0x3FF;
        }
        self.noise = r.u8()? & 0x07;
        r.fill(&mut self.volume)?;
        if self.volume.iter().any(|&v| v > 0x0F) {
            return Err(StateError::Invalid("volume do PSG"));
        }
        for v in &mut self.counter {
            *v = r.u16()?;
        }
        for v in &mut self.output {
            *v = r.bool()?;
        }
        self.lfsr = r.u16()?;
        self.latched_channel = usize::from(r.u8()?);
        if self.latched_channel > 3 {
            return Err(StateError::Invalid("canal do PSG"));
        }
        self.latched_volume = r.bool()?;
        self.cpu_cycles = r.u32()?;
        if self.cpu_cycles >= self.clock_divider {
            return Err(StateError::Invalid("ciclos do PSG"));
        }
        self.acc_sum = 0.0;
        self.acc_count = 0;
        self.phase = 0;
        self.samples.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CPU_CLOCK: u32 = 3_579_545;

    #[test]
    fn latch_and_data_bytes_set_ten_bit_period() {
        let mut psg = Psg::new(CPU_CLOCK, 44_100);
        psg.write(0x80 | 0x0E); // canal 0, tom, nibble baixo = E
        psg.write(0x3F); // 6 bits altos
        assert_eq!(psg.tone[0], 0x3FE);
        psg.write(0x90 | 0x05); // canal 0, volume 5
        assert_eq!(psg.volume[0], 5);
        psg.write(0x0A); // byte de dados continua no volume travado
        assert_eq!(psg.volume[0], 0x0A);
    }

    #[test]
    fn tone_toggles_every_period_ticks() {
        let mut psg = Psg::new(CPU_CLOCK, 44_100);
        psg.write(0x80 | 0x04); // período 4
        psg.write(0x00);
        let mut toggles = 0;
        let mut last = psg.output[0];
        for _ in 0..40 {
            psg.tick();
            if psg.output[0] != last {
                toggles += 1;
                last = psg.output[0];
            }
        }
        assert_eq!(toggles, 10);
    }

    #[test]
    fn noise_write_resets_lfsr() {
        let mut psg = Psg::new(CPU_CLOCK, 44_100);
        psg.write(0xE0 | 0x04); // ruído branco, taxa 0
        for _ in 0..1000 {
            psg.tick();
        }
        assert_ne!(psg.lfsr, LFSR_RESET);
        psg.write(0xE0 | 0x04);
        assert_eq!(psg.lfsr, LFSR_RESET);
    }

    #[test]
    fn white_noise_lfsr_sequence() {
        let mut psg = Psg::new(CPU_CLOCK, 44_100);
        psg.write(0xE4);
        // Duas bordas de subida a partir do reset.
        for _ in 0..(0x10 * 4) {
            psg.tick();
        }
        // 0x8000 -> 0x4000 -> 0x2000 (bits 0 e 3 ainda nulos).
        assert_eq!(psg.lfsr, 0x2000);
    }

    #[test]
    fn full_attenuation_is_silent() {
        let mut psg = Psg::new(CPU_CLOCK, 44_100);
        psg.run(CPU_CLOCK / 60);
        let mut out = Vec::new();
        psg.drain(&mut out);
        assert!(!out.is_empty());
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn produces_samples_at_requested_rate() {
        let mut psg = Psg::new(CPU_CLOCK, 48_000);
        psg.run(CPU_CLOCK);
        let mut out = Vec::new();
        psg.drain(&mut out);
        assert!((47_990..=48_010).contains(&out.len()), "{}", out.len());
    }
}
