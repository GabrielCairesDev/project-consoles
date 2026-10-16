//! YM2612 (OPN2): 6 canais FM de 4 operadores, DAC no canal 6, LFO e timers.
//!
//! Modelo do chip:
//! - fase de 20 bits: `((fnum << block) >> 1) + detune`, vezes o
//!   multiplicador; o índice do seno são os 10 bits altos;
//! - saída por tabelas de log-seno e exponencial (atenuação em passos de
//!   0,09375 dB), 14 bits com sinal;
//! - envelope ADSR com atualização a cada 3 amostras e as tabelas de
//!   incremento do chip; TL e AM somados na saída;
//! - LFO (AM e vibrato), timers A e B com flags de estouro, modo especial
//!   do canal 3 e DAC de 8 bits no canal 6.
//!
//! Limitações conhecidas: SSG-EG e o modo CSM não são emulados; o
//! "ladder effect" do DAC de 9 bits também não.
//!
//! Gera áudio na taxa interna do chip (clock / 144 ≈ 53 kHz) e reamostra
//! para a taxa pedida pela média das amostras de cada período.

use std::f64::consts::PI;
use std::sync::OnceLock;

use chips::state::{Reader, StateError, Writer};

const SIN_LEN: usize = 1024;
const TL_RES_LEN: usize = 256;
const TL_TAB_LEN: usize = 13 * 2 * TL_RES_LEN;
/// Atenuação máxima do envelope (10 bits).
const MAX_ATT: i32 = 1023;
/// Master clocks por amostra interna: 7 (clock do 68000) × 144.
pub(crate) const MCLK_PER_SAMPLE: u32 = 7 * 144;
/// Ganho de cada canal FM na mistura (±8191 → ±0,2).
const CHANNEL_GAIN: f32 = 0.2 / 8192.0;

/// Detune (em unidades do incremento de fase) por DT1 e key code.
const DT_TAB: [[u8; 32]; 4] = [
    [0; 32],
    [
        0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7, 8, 8,
        8, 8,
    ],
    [
        1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 10, 11, 12, 13, 14,
        16, 16, 16, 16,
    ],
    [
        2, 2, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19,
        20, 22, 22, 22, 22,
    ],
];

/// Incrementos do envelope por linha de taxa e ciclo (tabela do chip).
const EG_INC: [[u8; 8]; 19] = [
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 2, 1, 1, 1, 2],
    [1, 2, 1, 2, 1, 2, 1, 2],
    [1, 2, 2, 2, 1, 2, 2, 2],
    [2, 2, 2, 2, 2, 2, 2, 2],
    [2, 2, 2, 4, 2, 2, 2, 4],
    [2, 4, 2, 4, 2, 4, 2, 4],
    [2, 4, 4, 4, 2, 4, 4, 4],
    [4, 4, 4, 4, 4, 4, 4, 4],
    [4, 4, 4, 8, 4, 4, 4, 8],
    [4, 8, 4, 8, 4, 8, 4, 8],
    [4, 8, 8, 8, 4, 8, 8, 8],
    [8, 8, 8, 8, 8, 8, 8, 8],
    [16, 16, 16, 16, 16, 16, 16, 16],
    [0, 0, 0, 0, 0, 0, 0, 0],
];

/// Amostras por passo do LFO, pelos 8 valores de frequência (3,98–72,2 Hz).
const LFO_PERIOD: [u32; 8] = [108, 77, 71, 67, 62, 44, 8, 5];
/// Profundidade do vibrato em cents por FMS.
const PM_CENTS: [f32; 8] = [0.0, 3.4, 6.7, 10.0, 14.0, 20.0, 40.0, 80.0];
/// Deslocamento do AM do LFO por AMS (8 = sem AM).
const AM_SHIFT: [u32; 4] = [8, 3, 1, 0];

struct Tables {
    /// Log-seno (atenuação ×2, bit 0 = sinal negativo).
    sin: [u16; SIN_LEN],
    /// Exponencial: atenuação → amplitude linear com sinal.
    tl: Vec<i32>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        // Passo de envelope: 128 dB / 1024.
        const ENV_STEP: f64 = 128.0 / 1024.0;
        let mut tl = vec![0i32; TL_TAB_LEN];
        for x in 0..TL_RES_LEN {
            let m =
                (f64::from(1 << 16) / 2f64.powf((x as f64 + 1.0) * (ENV_STEP / 4.0) / 8.0)).floor();
            let mut n = (m as i32) >> 4;
            n = if n & 1 != 0 { (n >> 1) + 1 } else { n >> 1 };
            n <<= 2;
            for i in 0..13 {
                tl[x * 2 + i * 2 * TL_RES_LEN] = n >> i;
                tl[x * 2 + 1 + i * 2 * TL_RES_LEN] = -(n >> i);
            }
        }
        let mut sin = [0u16; SIN_LEN];
        for (i, entry) in sin.iter_mut().enumerate() {
            let m = (((i * 2) + 1) as f64 * PI / SIN_LEN as f64).sin();
            let o = 8.0 * (1.0 / m.abs()).ln() / 2f64.ln() / (ENV_STEP / 4.0);
            let n = (2.0 * o) as i32;
            let n = (n >> 1) + (n & 1);
            *entry = (n * 2 + i32::from(m < 0.0)) as u16;
        }
        Tables { sin, tl }
    })
}

/// Saída de um operador: índice de fase (10 bits, já com modulação) e
/// atenuação total (envelope + TL + AM, 10 bits).
fn operator_output(phase_index: u32, attenuation: u32) -> i32 {
    let t = tables();
    let p = (attenuation << 3) as usize + usize::from(t.sin[phase_index as usize & (SIN_LEN - 1)]);
    if p >= TL_TAB_LEN { 0 } else { t.tl[p] }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EgPhase {
    Attack,
    Decay,
    Sustain,
    Release,
}

impl EgPhase {
    fn to_u8(self) -> u8 {
        self as u8
    }

    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Attack,
            1 => Self::Decay,
            2 => Self::Sustain,
            3 => Self::Release,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Operator {
    dt: u8,
    mul: u8,
    tl: u8,
    ks: u8,
    ar: u8,
    am: bool,
    d1r: u8,
    d2r: u8,
    sl: u8,
    rr: u8,
    phase: u32,
    att: i32,
    eg: EgPhase,
    key: bool,
}

impl Default for Operator {
    fn default() -> Self {
        Self {
            dt: 0,
            mul: 0,
            tl: 0,
            ks: 0,
            ar: 0,
            am: false,
            d1r: 0,
            d2r: 0,
            sl: 0,
            rr: 0,
            phase: 0,
            att: MAX_ATT,
            eg: EgPhase::Release,
            key: false,
        }
    }
}

impl Operator {
    fn key_on(&mut self, on: bool, kc: u8) {
        if on && !self.key {
            self.phase = 0;
            self.eg = EgPhase::Attack;
            if self.rate(self.ar, kc) >= 62 {
                self.att = 0;
                self.eg = EgPhase::Decay;
            }
        } else if !on && self.key {
            self.eg = EgPhase::Release;
        }
        self.key = on;
    }

    /// Taxa efetiva 0–63: 2·R + escala de tecla.
    fn rate(&self, r: u8, kc: u8) -> u8 {
        if r == 0 {
            0
        } else {
            (2 * r + (kc >> (3 - self.ks))).min(63)
        }
    }

    fn sustain_level(&self) -> i32 {
        if self.sl == 15 {
            31 << 5
        } else {
            i32::from(self.sl) << 5
        }
    }

    /// Um ciclo do gerador de envelope (`counter` = contador global).
    fn envelope(&mut self, counter: u32, kc: u8) {
        let r = match self.eg {
            EgPhase::Attack => self.rate(self.ar, kc),
            EgPhase::Decay => self.rate(self.d1r, kc),
            EgPhase::Sustain => self.rate(self.d2r, kc),
            EgPhase::Release => self.rate(self.rr * 2 + 1, kc),
        };
        let (shift, row) = rate_params(r);
        if counter & ((1 << shift) - 1) != 0 {
            return;
        }
        let inc = i32::from(EG_INC[row][((counter >> shift) & 7) as usize]);
        match self.eg {
            EgPhase::Attack => {
                if r >= 62 {
                    self.att = 0;
                } else {
                    self.att += (!self.att * inc) >> 4;
                }
                if self.att <= 0 {
                    self.att = 0;
                    self.eg = EgPhase::Decay;
                }
            }
            EgPhase::Decay => {
                self.att = (self.att + inc).min(MAX_ATT);
                if self.att >= self.sustain_level() {
                    self.eg = EgPhase::Sustain;
                }
            }
            EgPhase::Sustain | EgPhase::Release => self.att = (self.att + inc).min(MAX_ATT),
        }
    }

    /// Atenuação total na saída (envelope + TL + AM), limitada a 10 bits.
    fn attenuation(&self, am: u32) -> u32 {
        let am = if self.am { am } else { 0 };
        (self.att as u32 + (u32::from(self.tl) << 3) + am).min(MAX_ATT as u32)
    }

    fn output(&self, modulation: i32, am: u32) -> i32 {
        let index = ((self.phase >> 10) as i32).wrapping_add(modulation) as u32;
        operator_output(index, self.attenuation(am))
    }
}

/// Deslocamento e linha da tabela de incrementos para uma taxa efetiva.
fn rate_params(rate: u8) -> (u32, usize) {
    match rate {
        0 | 1 => (0, 18),
        2..=47 => (11 - u32::from(rate >> 2), usize::from(rate & 3)),
        48..=59 => (0, 4 + usize::from(rate - 48)),
        60 | 61 => (0, 16),
        _ => (0, 17),
    }
}

/// Key code de 5 bits (bloco + 2 bits da nota) para detune e escala.
fn key_code(block: u8, fnum: u16) -> u8 {
    let f11 = (fnum >> 10) & 1;
    let f10 = (fnum >> 9) & 1;
    let f9 = (fnum >> 8) & 1;
    let f8 = (fnum >> 7) & 1;
    let n3 = (f11 & (f10 | f9 | f8)) | ((1 - f11) & f10 & f9 & f8);
    (block << 2) | ((f11 << 1) | n3) as u8
}

#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    /// Operadores na ordem dos registradores: O1, O3, O2, O4.
    ops: [Operator; 4],
    fnum: u16,
    block: u8,
    /// Byte alto de frequência travado até a escrita do byte baixo.
    fnum_latch: u8,
    algorithm: u8,
    feedback: u8,
    left: bool,
    right: bool,
    ams: u8,
    fms: u8,
    /// Duas últimas saídas de O1 (feedback).
    o1_out: [i32; 2],
}

/// Índices na ordem dos registradores.
const O1: usize = 0;
const O3: usize = 1;
const O2: usize = 2;
const O4: usize = 3;

#[derive(Debug, Clone)]
pub struct Ym2612 {
    channels: [Channel; 6],
    address: [u8; 2],
    /// Frequências do modo especial do canal 3 (O1, O2, O3) e travas.
    ch3_fnum: [u16; 3],
    ch3_block: [u8; 3],
    ch3_latch: [u8; 3],
    ch3_mode: u8,
    lfo_enabled: bool,
    lfo_freq: u8,
    lfo_step: u8,
    lfo_counter: u32,
    timer_a: u16,
    timer_b: u8,
    timer_a_count: u16,
    timer_b_count: u16,
    timer_b_sub: u8,
    timer_control: u8,
    status: u8,
    dac_enabled: bool,
    dac: u8,
    eg_counter: u32,
    eg_timer: u8,
    mclk: u32,
    // Reamostragem (não vai para o estado salvo).
    step: u64,
    phase: u64,
    acc: [f32; 2],
    acc_count: u32,
    samples: Vec<f32>,
}

impl Ym2612 {
    pub fn new(sample_rate: u32) -> Self {
        let chip_rate = u64::from(crate::MASTER_CLOCK / MCLK_PER_SAMPLE);
        Self::blank(
            (chip_rate << 32) / u64::from(sample_rate.max(1)),
            Vec::new(),
        )
    }

    /// Reset do chip (linha RESET do Z80): registradores e vozes zerados.
    /// O áudio já gerado e a taxa de saída são mantidos.
    pub fn reset(&mut self) {
        let samples = std::mem::take(&mut self.samples);
        *self = Self::blank(self.step, samples);
    }

    fn blank(step: u64, samples: Vec<f32>) -> Self {
        let mut channels = [Channel::default(); 6];
        for ch in &mut channels {
            ch.left = true;
            ch.right = true;
        }
        Self {
            channels,
            address: [0; 2],
            ch3_fnum: [0; 3],
            ch3_block: [0; 3],
            ch3_latch: [0; 3],
            ch3_mode: 0,
            lfo_enabled: false,
            lfo_freq: 0,
            lfo_step: 0,
            lfo_counter: 0,
            timer_a: 0,
            timer_b: 0,
            timer_a_count: 0,
            timer_b_count: 0,
            timer_b_sub: 0,
            timer_control: 0,
            status: 0,
            dac_enabled: false,
            dac: 0x80,
            eg_counter: 0,
            eg_timer: 0,
            mclk: 0,
            step,
            phase: 0,
            acc: [0.0; 2],
            acc_count: 0,
            samples,
        }
    }

    // ---- Registradores ----------------------------------------------------

    /// `port`: 0/2 = endereço (parte 1/2), 1/3 = dado.
    pub fn write(&mut self, port: u16, value: u8) {
        let part = usize::from(port >> 1) & 1;
        if port & 1 == 0 {
            self.address[part] = value;
            return;
        }
        let reg = self.address[part];
        if part == 0 && reg < 0x30 {
            self.write_global(reg, value);
        } else if reg >= 0x30 {
            self.write_channel(part, reg, value);
        }
    }

    fn write_global(&mut self, reg: u8, value: u8) {
        match reg {
            0x22 => {
                self.lfo_enabled = value & 0x08 != 0;
                self.lfo_freq = value & 7;
                if !self.lfo_enabled {
                    self.lfo_step = 0;
                    self.lfo_counter = 0;
                }
            }
            0x24 => self.timer_a = (self.timer_a & 0x003) | (u16::from(value) << 2),
            0x25 => self.timer_a = (self.timer_a & 0x3FC) | u16::from(value & 3),
            0x26 => self.timer_b = value,
            0x27 => {
                if value & 0x01 != 0 && self.timer_control & 0x01 == 0 {
                    self.timer_a_count = self.timer_a;
                }
                if value & 0x02 != 0 && self.timer_control & 0x02 == 0 {
                    self.timer_b_count = u16::from(self.timer_b);
                    self.timer_b_sub = 0;
                }
                if value & 0x10 != 0 {
                    self.status &= !0x01;
                }
                if value & 0x20 != 0 {
                    self.status &= !0x02;
                }
                self.timer_control = value & 0x0F;
                self.ch3_mode = value >> 6;
            }
            0x28 => {
                let ch = match value & 7 {
                    c @ 0..=2 => usize::from(c),
                    c @ 4..=6 => usize::from(c) - 1,
                    _ => return,
                };
                let kc = self.op_key_code(ch, O1);
                let channel = &mut self.channels[ch];
                // Bits 4–7: O1, O2, O3, O4.
                for (bit, op) in [(0x10, O1), (0x20, O2), (0x40, O3), (0x80, O4)] {
                    channel.ops[op].key_on(value & bit != 0, kc);
                }
            }
            0x2A => self.dac = value,
            0x2B => self.dac_enabled = value & 0x80 != 0,
            _ => {}
        }
    }

    fn write_channel(&mut self, part: usize, reg: u8, value: u8) {
        let index = usize::from(reg & 3);
        if index == 3 {
            return;
        }
        let ch = part * 3 + index;
        if reg < 0xA0 {
            let op = usize::from((reg >> 2) & 3);
            let o = &mut self.channels[ch].ops[op];
            match reg & 0xF0 {
                0x30 => {
                    o.dt = (value >> 4) & 7;
                    o.mul = value & 0x0F;
                }
                0x40 => o.tl = value & 0x7F,
                0x50 => {
                    o.ks = value >> 6;
                    o.ar = value & 0x1F;
                }
                0x60 => {
                    o.am = value & 0x80 != 0;
                    o.d1r = value & 0x1F;
                }
                0x70 => o.d2r = value & 0x1F,
                0x80 => {
                    o.sl = value >> 4;
                    o.rr = value & 0x0F;
                }
                // 0x90: SSG-EG (não emulado).
                _ => {}
            }
            return;
        }
        let c = &mut self.channels[ch];
        match reg & 0xFC {
            0xA0 => {
                c.fnum = u16::from(c.fnum_latch & 7) << 8 | u16::from(value);
                c.block = (c.fnum_latch >> 3) & 7;
            }
            0xA4 => c.fnum_latch = value & 0x3F,
            0xA8 if part == 0 => {
                let latch = self.ch3_latch[index];
                self.ch3_fnum[index] = u16::from(latch & 7) << 8 | u16::from(value);
                self.ch3_block[index] = (latch >> 3) & 7;
            }
            0xAC if part == 0 => self.ch3_latch[index] = value & 0x3F,
            0xB0 => {
                c.feedback = (value >> 3) & 7;
                c.algorithm = value & 7;
            }
            0xB4 => {
                c.left = value & 0x80 != 0;
                c.right = value & 0x40 != 0;
                c.ams = (value >> 4) & 3;
                c.fms = value & 7;
            }
            _ => {}
        }
    }

    /// Status: bits 1–0 = estouro dos timers B e A (ocupado sempre em 0).
    pub fn read_status(&self) -> u8 {
        self.status
    }

    // ---- Síntese ----------------------------------------------------------

    /// Frequência (bloco, fnum) de um operador, considerando o modo
    /// especial do canal 3 (A9 → O1, AA → O2, A8 → O3).
    fn op_frequency(&self, ch: usize, op: usize) -> (u8, u16) {
        if ch == 2 && self.ch3_mode != 0 && op != O4 {
            let slot = match op {
                O1 => 1,
                O2 => 2,
                _ => 0,
            };
            (self.ch3_block[slot], self.ch3_fnum[slot])
        } else {
            let c = &self.channels[ch];
            (c.block, c.fnum)
        }
    }

    fn op_key_code(&self, ch: usize, op: usize) -> u8 {
        let (block, fnum) = self.op_frequency(ch, op);
        key_code(block, fnum)
    }

    fn phase_increment(&self, ch: usize, op: usize, pm_factor: f32) -> u32 {
        let (block, fnum) = self.op_frequency(ch, op);
        let o = &self.channels[ch].ops[op];
        let fnum = if pm_factor != 1.0 {
            ((f32::from(fnum) * pm_factor) as u32).min(0x7FF)
        } else {
            u32::from(fnum)
        };
        let base = (fnum << block) >> 1;
        let kc = key_code(block, fnum as u16);
        let detune = u32::from(DT_TAB[usize::from(o.dt & 3)][usize::from(kc)]);
        let with_dt = if o.dt & 4 != 0 {
            base.wrapping_sub(detune)
        } else {
            base.wrapping_add(detune)
        } & 0x1_FFFF;
        if o.mul == 0 {
            with_dt >> 1
        } else {
            with_dt * u32::from(o.mul)
        }
    }

    /// Avança `mclk` master clocks.
    pub fn run(&mut self, mclk: u32) {
        self.mclk += mclk;
        while self.mclk >= MCLK_PER_SAMPLE {
            self.mclk -= MCLK_PER_SAMPLE;
            let (l, r) = self.clock_sample();
            self.acc[0] += l;
            self.acc[1] += r;
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
    }

    /// Uma amostra interna: timers, LFO, envelopes e os 6 canais.
    fn clock_sample(&mut self) -> (f32, f32) {
        self.clock_timers();

        let (am, pm) = self.clock_lfo();
        self.eg_timer += 1;
        if self.eg_timer == 3 {
            self.eg_timer = 0;
            self.eg_counter = self.eg_counter.wrapping_add(1);
            for ch in 0..6 {
                for op in 0..4 {
                    let kc = self.op_key_code(ch, op);
                    let counter = self.eg_counter;
                    self.channels[ch].ops[op].envelope(counter, kc);
                }
            }
        }

        let (mut left, mut right) = (0.0f32, 0.0f32);
        for ch in 0..6 {
            let c = &self.channels[ch];
            let am_value = am >> AM_SHIFT[usize::from(c.ams)];
            let pm_factor = if pm == 0.0 || c.fms == 0 {
                1.0
            } else {
                2f32.powf(pm * PM_CENTS[usize::from(c.fms)] / 1200.0)
            };
            let out = if ch == 5 && self.dac_enabled {
                (i32::from(self.dac) - 128) << 6
            } else {
                self.channel_output(ch, am_value)
            };
            for op in 0..4 {
                let inc = self.phase_increment(ch, op, pm_factor);
                let o = &mut self.channels[ch].ops[op];
                o.phase = (o.phase + inc) & 0xF_FFFF;
            }
            let c = &self.channels[ch];
            let v = out as f32 * CHANNEL_GAIN;
            if c.left {
                left += v;
            }
            if c.right {
                right += v;
            }
        }
        (left, right)
    }

    /// Saída de um canal pelo algoritmo, limitada a 14 bits.
    fn channel_output(&mut self, ch: usize, am: u32) -> i32 {
        let c = &self.channels[ch];
        let fb = if c.feedback == 0 {
            0
        } else {
            (c.o1_out[0] + c.o1_out[1]) >> (10 - c.feedback)
        };
        let ops = c.ops;
        let o1 = ops[O1].output(fb, am);
        let m = |v: i32| v >> 1;
        let out = match c.algorithm {
            0 => {
                let o2 = ops[O2].output(m(o1), am);
                let o3 = ops[O3].output(m(o2), am);
                ops[O4].output(m(o3), am)
            }
            1 => {
                let o2 = ops[O2].output(0, am);
                let o3 = ops[O3].output(m(o1 + o2), am);
                ops[O4].output(m(o3), am)
            }
            2 => {
                let o2 = ops[O2].output(0, am);
                let o3 = ops[O3].output(m(o2), am);
                ops[O4].output(m(o1 + o3), am)
            }
            3 => {
                let o2 = ops[O2].output(m(o1), am);
                let o3 = ops[O3].output(0, am);
                ops[O4].output(m(o2 + o3), am)
            }
            4 => {
                let o2 = ops[O2].output(m(o1), am);
                let o3 = ops[O3].output(0, am);
                o2 + ops[O4].output(m(o3), am)
            }
            5 => {
                let mo = m(o1);
                ops[O2].output(mo, am) + ops[O3].output(mo, am) + ops[O4].output(mo, am)
            }
            6 => {
                let o2 = ops[O2].output(m(o1), am);
                o2 + ops[O3].output(0, am) + ops[O4].output(0, am)
            }
            _ => o1 + ops[O2].output(0, am) + ops[O3].output(0, am) + ops[O4].output(0, am),
        };
        let c = &mut self.channels[ch];
        c.o1_out = [c.o1_out[1], o1];
        out.clamp(-8192, 8191)
    }

    fn clock_timers(&mut self) {
        if self.timer_control & 0x01 != 0 {
            self.timer_a_count += 1;
            if self.timer_a_count >= 1024 {
                self.timer_a_count = self.timer_a;
                if self.timer_control & 0x04 != 0 {
                    self.status |= 0x01;
                }
            }
        }
        if self.timer_control & 0x02 != 0 {
            self.timer_b_sub += 1;
            if self.timer_b_sub == 16 {
                self.timer_b_sub = 0;
                self.timer_b_count += 1;
                if self.timer_b_count >= 256 {
                    self.timer_b_count = u16::from(self.timer_b);
                    if self.timer_control & 0x08 != 0 {
                        self.status |= 0x02;
                    }
                }
            }
        }
    }

    /// Valores atuais do LFO: AM (0–126) e PM (-1..1, triangular).
    fn clock_lfo(&mut self) -> (u32, f32) {
        if !self.lfo_enabled {
            return (0, 0.0);
        }
        self.lfo_counter += 1;
        if self.lfo_counter >= LFO_PERIOD[usize::from(self.lfo_freq)] {
            self.lfo_counter = 0;
            self.lfo_step = (self.lfo_step + 1) & 127;
        }
        let s = u32::from(self.lfo_step);
        let am = if s < 64 { s * 2 } else { 126 - (s - 64) * 2 };
        let pm = match s {
            0..=31 => s as f32 / 32.0,
            32..=95 => (64.0 - s as f32) / 32.0,
            _ => (s as f32 - 128.0) / 32.0,
        };
        (am, pm)
    }

    /// Move o áudio gerado (estéreo intercalado L, R) para `out`.
    pub fn drain(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.samples);
    }

    // ---- Estado salvo -----------------------------------------------------

    pub fn save_state(&self, w: &mut Writer) {
        for c in &self.channels {
            for o in &c.ops {
                for v in [
                    o.dt,
                    o.mul,
                    o.tl,
                    o.ks,
                    o.ar,
                    o.d1r,
                    o.d2r,
                    o.sl,
                    o.rr,
                    o.eg.to_u8(),
                ] {
                    w.u8(v);
                }
                w.bool(o.am);
                w.bool(o.key);
                w.u32(o.phase);
                w.u16(o.att as u16);
            }
            w.u16(c.fnum);
            for v in [c.block, c.fnum_latch, c.algorithm, c.feedback, c.ams, c.fms] {
                w.u8(v);
            }
            w.bool(c.left);
            w.bool(c.right);
            w.u32(c.o1_out[0] as u32);
            w.u32(c.o1_out[1] as u32);
        }
        w.slice(&self.address);
        for i in 0..3 {
            w.u16(self.ch3_fnum[i]);
            w.u8(self.ch3_block[i]);
            w.u8(self.ch3_latch[i]);
        }
        w.u8(self.ch3_mode);
        w.bool(self.lfo_enabled);
        w.u8(self.lfo_freq);
        w.u8(self.lfo_step);
        w.u32(self.lfo_counter);
        w.u16(self.timer_a);
        w.u8(self.timer_b);
        w.u16(self.timer_a_count);
        w.u16(self.timer_b_count);
        w.u8(self.timer_b_sub);
        w.u8(self.timer_control);
        w.u8(self.status);
        w.bool(self.dac_enabled);
        w.u8(self.dac);
        w.u32(self.eg_counter);
        w.u8(self.eg_timer);
        w.u32(self.mclk);
    }

    pub fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        let invalid = || StateError::Invalid("YM2612");
        for c in &mut self.channels {
            for o in &mut c.ops {
                o.dt = r.u8()? & 7;
                o.mul = r.u8()? & 0x0F;
                o.tl = r.u8()? & 0x7F;
                o.ks = r.u8()? & 3;
                o.ar = r.u8()? & 0x1F;
                o.d1r = r.u8()? & 0x1F;
                o.d2r = r.u8()? & 0x1F;
                o.sl = r.u8()? & 0x0F;
                o.rr = r.u8()? & 0x0F;
                o.eg = EgPhase::from_u8(r.u8()?).ok_or_else(invalid)?;
                o.am = r.bool()?;
                o.key = r.bool()?;
                o.phase = r.u32()? & 0xF_FFFF;
                o.att = i32::from(r.u16()?).min(MAX_ATT);
            }
            c.fnum = r.u16()? & 0x7FF;
            c.block = r.u8()? & 7;
            c.fnum_latch = r.u8()? & 0x3F;
            c.algorithm = r.u8()? & 7;
            c.feedback = r.u8()? & 7;
            c.ams = r.u8()? & 3;
            c.fms = r.u8()? & 7;
            c.left = r.bool()?;
            c.right = r.bool()?;
            c.o1_out = [r.u32()? as i32, r.u32()? as i32];
        }
        r.fill(&mut self.address)?;
        for i in 0..3 {
            self.ch3_fnum[i] = r.u16()? & 0x7FF;
            self.ch3_block[i] = r.u8()? & 7;
            self.ch3_latch[i] = r.u8()? & 0x3F;
        }
        self.ch3_mode = r.u8()? & 3;
        self.lfo_enabled = r.bool()?;
        self.lfo_freq = r.u8()? & 7;
        self.lfo_step = r.u8()? & 127;
        self.lfo_counter = r.u32()?;
        self.timer_a = r.u16()? & 0x3FF;
        self.timer_b = r.u8()?;
        self.timer_a_count = r.u16()?.min(1023);
        self.timer_b_count = r.u16()?.min(255);
        self.timer_b_sub = r.u8()? & 15;
        self.timer_control = r.u8()? & 0x0F;
        self.status = r.u8()? & 3;
        self.dac_enabled = r.bool()?;
        self.dac = r.u8()?;
        self.eg_counter = r.u32()?;
        self.eg_timer = r.u8()? % 3;
        self.mclk = r.u32()?;
        if self.mclk >= MCLK_PER_SAMPLE {
            return Err(invalid());
        }
        self.phase = 0;
        self.acc = [0.0; 2];
        self.acc_count = 0;
        self.samples.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
