//! APU do DMG: dois canais de onda quadrada (o 1 com sweep), um de onda
//! programável e um de ruído, misturados em estéreo por NR50/NR51.
//!
//! O frame sequencer (512 Hz) avança na borda de descida do bit 12 do
//! contador do timer (bit 4 do DIV), como no hardware: escrever no DIV
//! pode adiantá-lo. A saída é a média de cada M-ciclo dentro de cada
//! amostra, o que funciona como um filtro passa-baixas simples.

use chips::state::{Reader, StateError, Writer};

use crate::CPU_CLOCK;

/// Bits que sempre são lidos como 1 em FF10–FF2F.
const READ_MASKS: [u8; 0x20] = [
    0x80, 0x3F, 0x00, 0xFF, 0xBF, // NR10–NR14
    0xFF, 0x3F, 0x00, 0xFF, 0xBF, // —, NR21–NR24
    0x7F, 0xFF, 0x9F, 0xFF, 0xBF, // NR30–NR34
    0xFF, 0xFF, 0x00, 0x00, 0xBF, // —, NR41–NR44
    0x00, 0x00, 0x70, // NR50–NR52
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

const DUTY: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 0],
];

const NOISE_DIVISORS: [u32; 8] = [8, 16, 32, 48, 64, 80, 96, 112];

#[derive(Debug, Clone, Default)]
struct Envelope {
    initial: u8,
    increase: bool,
    period: u8,
    volume: u8,
    timer: u8,
}

impl Envelope {
    fn write(&mut self, value: u8) {
        self.initial = value >> 4;
        self.increase = value & 0x08 != 0;
        self.period = value & 0x07;
    }

    fn register(&self) -> u8 {
        self.initial << 4 | u8::from(self.increase) << 3 | self.period
    }

    /// O DAC está ligado se volume inicial ou direção não são zero.
    fn dac_on(&self) -> bool {
        self.register() & 0xF8 != 0
    }

    fn trigger(&mut self) {
        self.volume = self.initial;
        self.timer = self.period;
    }

    fn clock(&mut self) {
        if self.period == 0 {
            return;
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = self.period;
            if self.increase && self.volume < 15 {
                self.volume += 1;
            } else if !self.increase && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }
}

/// Contador de duração comum aos quatro canais.
#[derive(Debug, Clone, Default)]
struct Length {
    counter: u16,
    enabled: bool,
}

impl Length {
    /// Um passo; devolve `true` se o canal deve desligar.
    fn clock(&mut self) -> bool {
        if self.enabled && self.counter > 0 {
            self.counter -= 1;
            return self.counter == 0;
        }
        false
    }
}

#[derive(Debug, Clone, Default)]
struct Square {
    enabled: bool,
    duty: u8,
    position: u8,
    frequency: u16,
    timer: u32,
    length: Length,
    envelope: Envelope,
    // Só o canal 1:
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_timer: u8,
    sweep_enabled: bool,
    sweep_shadow: u16,
    /// Já houve um cálculo em modo de subtração desde o trigger.
    sweep_negated: bool,
}

impl Square {
    fn period(&self) -> u32 {
        (2048 - u32::from(self.frequency)) * 4
    }

    fn tick(&mut self, cycles: u32) {
        let mut left = cycles;
        while left >= self.timer {
            left -= self.timer;
            self.timer = self.period();
            self.position = (self.position + 1) & 7;
        }
        self.timer -= left;
    }

    fn output(&self) -> Option<u8> {
        if !self.envelope.dac_on() {
            return None;
        }
        let high = DUTY[usize::from(self.duty)][usize::from(self.position)];
        Some(if self.enabled {
            high * self.envelope.volume
        } else {
            0
        })
    }

    fn sweep_calculate(&mut self) -> u16 {
        let delta = self.sweep_shadow >> self.sweep_shift;
        if self.sweep_negate {
            self.sweep_negated = true;
            self.sweep_shadow.wrapping_sub(delta)
        } else {
            self.sweep_shadow + delta
        }
    }

    fn sweep_clock(&mut self) {
        if self.sweep_timer > 0 {
            self.sweep_timer -= 1;
        }
        if self.sweep_timer != 0 {
            return;
        }
        self.sweep_timer = if self.sweep_period == 0 {
            8
        } else {
            self.sweep_period
        };
        if !self.sweep_enabled || self.sweep_period == 0 {
            return;
        }
        let next = self.sweep_calculate();
        if next > 2047 {
            self.enabled = false;
        } else if self.sweep_shift != 0 {
            self.sweep_shadow = next;
            self.frequency = next;
            // Segunda conta só para checar o estouro.
            if self.sweep_calculate() > 2047 {
                self.enabled = false;
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Wave {
    enabled: bool,
    dac_on: bool,
    volume_code: u8,
    frequency: u16,
    timer: u32,
    position: u8,
    sample: u8,
    length: Length,
    ram: [u8; 16],
}

impl Wave {
    fn period(&self) -> u32 {
        (2048 - u32::from(self.frequency)) * 2
    }

    fn tick(&mut self, cycles: u32) {
        if !self.enabled {
            return;
        }
        let mut left = cycles;
        while left >= self.timer {
            left -= self.timer;
            self.timer = self.period();
            self.position = (self.position + 1) & 31;
            let byte = self.ram[usize::from(self.position / 2)];
            self.sample = if self.position & 1 == 0 {
                byte >> 4
            } else {
                byte & 0x0F
            };
        }
        self.timer -= left;
    }

    fn output(&self) -> Option<u8> {
        if !self.dac_on {
            return None;
        }
        if !self.enabled || self.volume_code == 0 {
            return Some(0);
        }
        Some(self.sample >> (self.volume_code - 1))
    }
}

#[derive(Debug, Clone)]
struct Noise {
    enabled: bool,
    shift: u8,
    narrow: bool,
    divisor: u8,
    timer: u32,
    lfsr: u16,
    length: Length,
    envelope: Envelope,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            enabled: false,
            shift: 0,
            narrow: false,
            divisor: 0,
            timer: 8,
            lfsr: 0x7FFF,
            length: Length::default(),
            envelope: Envelope::default(),
        }
    }
}

impl Noise {
    fn period(&self) -> u32 {
        NOISE_DIVISORS[usize::from(self.divisor)] << self.shift
    }

    fn tick(&mut self, cycles: u32) {
        let mut left = cycles;
        while left >= self.timer {
            left -= self.timer;
            self.timer = self.period();
            let bit = (self.lfsr ^ (self.lfsr >> 1)) & 1;
            self.lfsr = (self.lfsr >> 1) | (bit << 14);
            if self.narrow {
                self.lfsr = (self.lfsr & !0x40) | (bit << 6);
            }
        }
        self.timer -= left;
    }

    fn output(&self) -> Option<u8> {
        if !self.envelope.dac_on() {
            return None;
        }
        let high = u8::from(self.lfsr & 1 == 0);
        Some(if self.enabled {
            high * self.envelope.volume
        } else {
            0
        })
    }
}

#[derive(Debug, Clone)]
pub struct Apu {
    powered: bool,
    ch1: Square,
    ch2: Square,
    ch3: Wave,
    ch4: Noise,
    /// NR50 e NR51.
    volume: u8,
    panning: u8,
    /// Último valor escrito em cada registrador (para leitura).
    regs: [u8; 0x20],
    sequencer_step: u8,
    /// Bit 12 do contador do timer no último M-ciclo.
    div_bit: bool,
    // Reamostragem: acumula a mistura de cada M-ciclo.
    step: u64,
    phase: u64,
    acc: [f32; 2],
    acc_count: u32,
    samples: Vec<f32>,
}

impl Apu {
    pub fn new(sample_rate: u32) -> Self {
        let rate = u64::from(sample_rate.max(1));
        let mut apu = Self {
            powered: true,
            ch1: Square::default(),
            ch2: Square::default(),
            ch3: Wave::default(),
            ch4: Noise::default(),
            volume: 0,
            panning: 0,
            regs: [0; 0x20],
            sequencer_step: 0,
            div_bit: false,
            // Avanço de fase por M-ciclo, em 32.32 bits de amostra.
            step: (u64::from(CPU_CLOCK / 4) << 32) / rate,
            phase: 0,
            acc: [0.0; 2],
            acc_count: 0,
            samples: Vec::new(),
        };
        // Estado em que a boot ROM do DMG deixa o som.
        for (addr, value) in [
            (0xFF10, 0x80),
            (0xFF11, 0xBF),
            (0xFF12, 0xF3),
            (0xFF13, 0xFF),
            (0xFF14, 0x3F),
            (0xFF16, 0x3F),
            (0xFF17, 0x00),
            (0xFF18, 0xFF),
            (0xFF19, 0x3F),
            (0xFF1A, 0x7F),
            (0xFF1B, 0xFF),
            (0xFF1C, 0x9F),
            (0xFF1D, 0xFF),
            (0xFF1E, 0x3F),
            (0xFF20, 0xFF),
            (0xFF21, 0x00),
            (0xFF22, 0x00),
            (0xFF23, 0x3F),
            (0xFF24, 0x77),
            (0xFF25, 0xF3),
        ] {
            apu.write(addr, value, false);
        }
        // O "plim" da boot ROM: canal 1 ligado, já em silêncio.
        apu.ch1.enabled = true;
        apu.ch1.envelope.volume = 0;
        apu
    }

    /// Avança um M-ciclo. `div_bit` é o bit 12 do contador do timer.
    pub fn tick(&mut self, div_bit: bool) {
        if self.powered {
            if self.div_bit && !div_bit {
                self.clock_sequencer();
            }
            self.ch1.tick(4);
            self.ch2.tick(4);
            self.ch3.tick(4);
            self.ch4.tick(4);
        }
        self.div_bit = div_bit;
        self.mix();
    }

    fn clock_sequencer(&mut self) {
        let step = self.sequencer_step;
        if step.is_multiple_of(2) {
            if self.ch1.length.clock() {
                self.ch1.enabled = false;
            }
            if self.ch2.length.clock() {
                self.ch2.enabled = false;
            }
            if self.ch3.length.clock() {
                self.ch3.enabled = false;
            }
            if self.ch4.length.clock() {
                self.ch4.enabled = false;
            }
        }
        if step == 2 || step == 6 {
            self.ch1.sweep_clock();
        }
        if step == 7 {
            self.ch1.envelope.clock();
            self.ch2.envelope.clock();
            self.ch4.envelope.clock();
        }
        self.sequencer_step = (step + 1) & 7;
    }

    /// O próximo passo do sequencer não conta duração (passo atual ímpar
    /// no hardware): ligar a duração agora gera um passo extra.
    fn length_extra_clock(&self) -> bool {
        !self.sequencer_step.is_multiple_of(2)
    }

    fn mix(&mut self) {
        let outputs = [
            self.ch1.output(),
            self.ch2.output(),
            self.ch3.output(),
            self.ch4.output(),
        ];
        let mut sides = [0.0f32; 2];
        for (i, output) in outputs.into_iter().enumerate() {
            // DAC: 0..15 vira +1..-1; DAC desligado não contribui.
            let Some(level) = output else { continue };
            let analog = 1.0 - f32::from(level) / 7.5;
            if self.panning & (0x10 << i) != 0 {
                sides[0] += analog;
            }
            if self.panning & (0x01 << i) != 0 {
                sides[1] += analog;
            }
        }
        let left_volume = f32::from((self.volume >> 4) & 7) + 1.0;
        let right_volume = f32::from(self.volume & 7) + 1.0;
        // 4 canais × volume 8 vão a ±0,5: sobra margem para o nível DC e
        // o volume fica parecido com o dos outros consoles.
        self.acc[0] += sides[0] * left_volume / 64.0;
        self.acc[1] += sides[1] * right_volume / 64.0;
        self.acc_count += 1;
        self.phase += 1 << 32;
        if self.phase >= self.step {
            self.phase -= self.step;
            let n = self.acc_count as f32;
            self.samples.push(self.acc[0] / n);
            self.samples.push(self.acc[1] / n);
            self.acc = [0.0; 2];
            self.acc_count = 0;
        }
    }

    /// Move as amostras geradas (estéreo intercalado) para `out`.
    pub fn drain(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.samples);
    }

    fn status(&self) -> u8 {
        u8::from(self.powered) << 7
            | u8::from(self.ch4.enabled) << 3
            | u8::from(self.ch3.enabled) << 2
            | u8::from(self.ch2.enabled) << 1
            | u8::from(self.ch1.enabled)
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF30..=0xFF3F => self.ch3.ram[usize::from(addr - 0xFF30)],
            0xFF26 => self.status() | 0x70,
            0xFF10..=0xFF2F => {
                let i = usize::from(addr - 0xFF10);
                self.regs[i] | READ_MASKS[i]
            }
            _ => 0xFF,
        }
    }

    /// Escrita da CPU.
    pub fn write_register(&mut self, addr: u16, value: u8) {
        self.write(addr, value, true);
    }

    fn write(&mut self, addr: u16, value: u8, from_cpu: bool) {
        if (0xFF30..=0xFF3F).contains(&addr) {
            self.ch3.ram[usize::from(addr - 0xFF30)] = value;
            return;
        }
        if addr == 0xFF26 {
            let on = value & 0x80 != 0;
            if self.powered && !on {
                self.power_off();
            } else if !self.powered && on {
                self.powered = true;
                self.sequencer_step = 0;
            }
            return;
        }
        if !(0xFF10..=0xFF25).contains(&addr) {
            return;
        }
        // Desligada, a APU ignora escritas (no DMG, menos a duração).
        if from_cpu && !self.powered {
            match addr {
                0xFF11 | 0xFF16 | 0xFF20 => self.write_length(addr, value & 0x3F),
                0xFF1B => self.write_length(addr, value),
                _ => {}
            }
            return;
        }
        self.regs[usize::from(addr - 0xFF10)] = value;
        let extra = self.length_extra_clock();
        match addr {
            0xFF10 => {
                let ch = &mut self.ch1;
                ch.sweep_period = (value >> 4) & 7;
                ch.sweep_negate = value & 0x08 != 0;
                ch.sweep_shift = value & 7;
                // Sair do modo de subtração depois de usá-lo desliga o canal.
                if !ch.sweep_negate && ch.sweep_negated {
                    ch.enabled = false;
                }
            }
            0xFF11 | 0xFF16 => {
                let ch = if addr == 0xFF11 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                ch.duty = value >> 6;
                ch.length.counter = 64 - u16::from(value & 0x3F);
            }
            0xFF12 | 0xFF17 => {
                let ch = if addr == 0xFF12 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                ch.envelope.write(value);
                if !ch.envelope.dac_on() {
                    ch.enabled = false;
                }
            }
            0xFF13 | 0xFF18 => {
                let ch = if addr == 0xFF13 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                ch.frequency = (ch.frequency & 0x700) | u16::from(value);
            }
            0xFF14 | 0xFF19 => {
                let is_ch1 = addr == 0xFF14;
                let ch = if is_ch1 { &mut self.ch1 } else { &mut self.ch2 };
                ch.frequency = (ch.frequency & 0xFF) | (u16::from(value & 7) << 8);
                Self::length_control(&mut ch.length, &mut ch.enabled, value, extra, 64);
                if value & 0x80 != 0 {
                    ch.enabled = ch.envelope.dac_on();
                    ch.timer = ch.period();
                    ch.envelope.trigger();
                    if is_ch1 {
                        ch.sweep_shadow = ch.frequency;
                        ch.sweep_timer = if ch.sweep_period == 0 {
                            8
                        } else {
                            ch.sweep_period
                        };
                        ch.sweep_enabled = ch.sweep_period != 0 || ch.sweep_shift != 0;
                        ch.sweep_negated = false;
                        if ch.sweep_shift != 0 && ch.sweep_calculate() > 2047 {
                            ch.enabled = false;
                        }
                    }
                }
            }
            0xFF1A => {
                self.ch3.dac_on = value & 0x80 != 0;
                if !self.ch3.dac_on {
                    self.ch3.enabled = false;
                }
            }
            0xFF1B => self.ch3.length.counter = 256 - u16::from(value),
            0xFF1C => self.ch3.volume_code = (value >> 5) & 3,
            0xFF1D => self.ch3.frequency = (self.ch3.frequency & 0x700) | u16::from(value),
            0xFF1E => {
                let ch = &mut self.ch3;
                ch.frequency = (ch.frequency & 0xFF) | (u16::from(value & 7) << 8);
                Self::length_control(&mut ch.length, &mut ch.enabled, value, extra, 256);
                if value & 0x80 != 0 {
                    ch.enabled = ch.dac_on;
                    // A primeira amostra sai depois de um período (mais
                    // 6 ciclos de atraso do hardware).
                    ch.timer = ch.period() + 6;
                    ch.position = 0;
                }
            }
            0xFF20 => self.ch4.length.counter = 64 - u16::from(value & 0x3F),
            0xFF21 => {
                self.ch4.envelope.write(value);
                if !self.ch4.envelope.dac_on() {
                    self.ch4.enabled = false;
                }
            }
            0xFF22 => {
                self.ch4.shift = value >> 4;
                self.ch4.narrow = value & 0x08 != 0;
                self.ch4.divisor = value & 7;
            }
            0xFF23 => {
                let ch = &mut self.ch4;
                Self::length_control(&mut ch.length, &mut ch.enabled, value, extra, 64);
                if value & 0x80 != 0 {
                    ch.enabled = ch.envelope.dac_on();
                    ch.timer = ch.period();
                    ch.lfsr = 0x7FFF;
                    ch.envelope.trigger();
                }
            }
            0xFF24 => self.volume = value,
            0xFF25 => self.panning = value,
            _ => {}
        }
    }

    /// Bits 6 (duração ligada) e 7 (trigger) de NRx4, com as manias do
    /// hardware: ligar a duração num passo sem contagem conta uma vez, e
    /// o trigger com contador zerado recarrega o máximo.
    fn length_control(length: &mut Length, enabled: &mut bool, value: u8, extra: bool, max: u16) {
        let was_enabled = length.enabled;
        length.enabled = value & 0x40 != 0;
        let trigger = value & 0x80 != 0;
        if extra && !was_enabled && length.enabled && length.counter > 0 {
            length.counter -= 1;
            if length.counter == 0 && !trigger {
                *enabled = false;
            }
        }
        if trigger && length.counter == 0 {
            length.counter = if extra && length.enabled {
                max - 1
            } else {
                max
            };
        }
    }

    fn write_length(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF11 => self.ch1.length.counter = 64 - u16::from(value),
            0xFF16 => self.ch2.length.counter = 64 - u16::from(value),
            0xFF20 => self.ch4.length.counter = 64 - u16::from(value),
            _ => self.ch3.length.counter = 256 - u16::from(value),
        }
    }

    /// Desligar a APU zera todos os registradores (a wave RAM e, no DMG,
    /// os contadores de duração ficam).
    fn power_off(&mut self) {
        let lengths = (
            self.ch1.length.counter,
            self.ch2.length.counter,
            self.ch3.length.counter,
            self.ch4.length.counter,
        );
        let ram = self.ch3.ram;
        self.ch1 = Square::default();
        self.ch2 = Square::default();
        self.ch3 = Wave::default();
        self.ch4 = Noise::default();
        self.ch1.length.counter = lengths.0;
        self.ch2.length.counter = lengths.1;
        self.ch3.length.counter = lengths.2;
        self.ch4.length.counter = lengths.3;
        self.ch3.ram = ram;
        self.volume = 0;
        self.panning = 0;
        self.regs = [0; 0x20];
        self.powered = false;
    }

    pub fn save_state(&self, w: &mut Writer) {
        // Os registradores reconstroem quase tudo; o resto é estado interno.
        w.bool(self.powered);
        w.slice(&self.regs);
        w.slice(&self.ch3.ram);
        w.u8(self.sequencer_step);
        w.bool(self.div_bit);
        for ch in [&self.ch1, &self.ch2] {
            w.bool(ch.enabled);
            w.u8(ch.position);
            w.u32(ch.timer);
            w.u16(ch.frequency);
            w.u16(ch.length.counter);
            w.u8(ch.envelope.volume);
            w.u8(ch.envelope.timer);
            w.u8(ch.sweep_timer);
            w.bool(ch.sweep_enabled);
            w.u16(ch.sweep_shadow);
            w.bool(ch.sweep_negated);
        }
        w.bool(self.ch3.enabled);
        w.u32(self.ch3.timer);
        w.u8(self.ch3.position);
        w.u8(self.ch3.sample);
        w.u16(self.ch3.length.counter);
        w.bool(self.ch4.enabled);
        w.u32(self.ch4.timer);
        w.u16(self.ch4.lfsr);
        w.u16(self.ch4.length.counter);
        w.u8(self.ch4.envelope.volume);
        w.u8(self.ch4.envelope.timer);
    }

    pub fn load_state(&mut self, r: &mut Reader) -> Result<(), StateError> {
        let mut next = Self::new(1);
        next.step = self.step;
        let powered = r.bool()?;
        let mut regs = [0; 0x20];
        r.fill(&mut regs)?;
        // Reaplica os registradores sem efeitos de trigger nem de duração.
        next.powered = true;
        for (i, &value) in regs.iter().enumerate().take(0x16) {
            let addr = 0xFF10 + i as u16;
            let value = if matches!(addr, 0xFF14 | 0xFF19 | 0xFF1E | 0xFF23) {
                value & 0x7F
            } else {
                value
            };
            next.write(addr, value, false);
        }
        next.regs = regs;
        next.powered = powered;
        r.fill(&mut next.ch3.ram)?;
        next.sequencer_step = r.u8()? & 7;
        next.div_bit = r.bool()?;
        for ch in [&mut next.ch1, &mut next.ch2] {
            ch.enabled = r.bool()?;
            ch.position = r.u8()? & 7;
            ch.timer = r.u32()?;
            ch.frequency = r.u16()? & 0x7FF;
            ch.length.counter = r.u16()?.min(64);
            ch.envelope.volume = r.u8()? & 0x0F;
            ch.envelope.timer = r.u8()? & 7;
            ch.sweep_timer = r.u8()? & 0x0F;
            ch.sweep_enabled = r.bool()?;
            ch.sweep_shadow = r.u16()? & 0x7FF;
            ch.sweep_negated = r.bool()?;
            if ch.timer > ch.period() {
                return Err(StateError::Invalid("APU"));
            }
        }
        next.ch3.enabled = r.bool()?;
        next.ch3.timer = r.u32()?;
        next.ch3.position = r.u8()? & 31;
        next.ch3.sample = r.u8()? & 0x0F;
        next.ch3.length.counter = r.u16()?.min(256);
        next.ch4.enabled = r.bool()?;
        next.ch4.timer = r.u32()?;
        next.ch4.lfsr = r.u16()? & 0x7FFF;
        next.ch4.length.counter = r.u16()?.min(64);
        next.ch4.envelope.volume = r.u8()? & 0x0F;
        next.ch4.envelope.timer = r.u8()? & 7;
        if next.ch3.timer > next.ch3.period() + 6 || next.ch4.timer > next.ch4.period() {
            return Err(StateError::Invalid("APU"));
        }
        next.samples = std::mem::take(&mut self.samples);
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
