//! Uma das 24 vozes: lê blocos SPU-ADPCM da RAM, anda pelo sample na
//! velocidade do pitch e interpola as 4 últimas amostras (gaussiana).

use super::RAM_HALFWORDS;
use super::envelope::{Adsr, Volume};

/// Amostras por bloco ADPCM de 16 bytes.
const BLOCK_SAMPLES: u32 = 28;
/// Contador de pitch: bits 12+ são a amostra; 4–11, o índice da
/// interpolação.
const BLOCK_END: u32 = BLOCK_SAMPLES << 12;

/// Coeficientes dos filtros ADPCM (5 no SPU).
const FILTER_POS: [i32; 5] = [0, 60, 115, 98, 122];
const FILTER_NEG: [i32; 5] = [0, 0, -52, -55, -60];

const FLAG_LOOP_END: u8 = 1;
const FLAG_LOOP_REPEAT: u8 = 2;
const FLAG_LOOP_START: u8 = 4;

/// Tabela da interpolação gaussiana do hardware (psx-spx). Cada grupo de
/// 4 pesos soma 7F7Fh..7F81h.
const GAUSS: [i32; 512] = [
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1,
    1, 2, 2, 2, 3, 3, 3, 4, 4, 5, 5, 6, 7, 7, 8, 9, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21,
    22, 24, 25, 27, 28, 30, 32, 33, 35, 37, 39, 41, 44, 46, 48, 51, 53, 56, 58, 61, 64, 67, 70, 73,
    77, 80, 84, 87, 91, 95, 99, 103, 107, 111, 116, 120, 125, 130, 135, 140, 145, 150, 156, 161,
    167, 173, 179, 186, 192, 199, 205, 212, 219, 227, 234, 242, 250, 257, 266, 274, 283, 291, 300,
    309, 319, 328, 338, 348, 358, 369, 379, 390, 401, 412, 424, 436, 448, 460, 473, 485, 498, 512,
    525, 539, 553, 567, 582, 597, 612, 627, 643, 659, 675, 692, 708, 726, 743, 761, 779, 797, 816,
    835, 854, 874, 894, 914, 935, 956, 977, 999, 1020, 1043, 1066, 1089, 1112, 1136, 1160, 1184,
    1209, 1234, 1260, 1286, 1312, 1339, 1366, 1394, 1422, 1450, 1479, 1508, 1537, 1567, 1598, 1628,
    1660, 1691, 1723, 1756, 1789, 1822, 1856, 1890, 1924, 1959, 1995, 2031, 2067, 2104, 2141, 2179,
    2217, 2256, 2295, 2334, 2374, 2415, 2456, 2497, 2539, 2582, 2624, 2668, 2712, 2756, 2801, 2846,
    2892, 2938, 2985, 3032, 3079, 3128, 3176, 3225, 3275, 3325, 3376, 3427, 3479, 3531, 3584, 3637,
    3691, 3745, 3799, 3855, 3910, 3967, 4023, 4081, 4138, 4197, 4255, 4315, 4374, 4435, 4495, 4557,
    4619, 4681, 4744, 4807, 4871, 4935, 5000, 5065, 5131, 5197, 5264, 5332, 5399, 5468, 5536, 5606,
    5676, 5746, 5817, 5888, 5959, 6032, 6104, 6177, 6251, 6325, 6400, 6475, 6550, 6626, 6702, 6779,
    6856, 6934, 7012, 7091, 7170, 7249, 7329, 7409, 7490, 7571, 7653, 7735, 7817, 7900, 7983, 8066,
    8150, 8234, 8319, 8404, 8489, 8575, 8661, 8748, 8834, 8922, 9009, 9097, 9185, 9273, 9362, 9451,
    9541, 9630, 9720, 9811, 9901, 9992, 10083, 10174, 10266, 10358, 10450, 10542, 10635, 10727,
    10820, 10913, 11007, 11100, 11194, 11288, 11382, 11476, 11571, 11665, 11760, 11855, 11950,
    12045, 12140, 12236, 12331, 12427, 12522, 12618, 12714, 12809, 12905, 13001, 13097, 13193,
    13289, 13385, 13481, 13577, 13673, 13769, 13865, 13961, 14056, 14152, 14248, 14343, 14439,
    14534, 14630, 14725, 14820, 14915, 15010, 15104, 15199, 15293, 15387, 15481, 15575, 15669,
    15762, 15855, 15948, 16041, 16133, 16226, 16317, 16409, 16500, 16592, 16682, 16773, 16863,
    16953, 17042, 17131, 17220, 17308, 17396, 17484, 17571, 17658, 17744, 17830, 17916, 18001,
    18086, 18170, 18254, 18337, 18420, 18502, 18584, 18665, 18746, 18826, 18905, 18985, 19063,
    19141, 19219, 19295, 19372, 19447, 19522, 19597, 19671, 19744, 19816, 19888, 19959, 20030,
    20100, 20169, 20238, 20306, 20373, 20439, 20505, 20570, 20634, 20698, 20760, 20822, 20884,
    20944, 21004, 21063, 21121, 21178, 21235, 21290, 21345, 21399, 21452, 21505, 21556, 21607,
    21657, 21706, 21754, 21801, 21848, 21893, 21938, 21982, 22025, 22066, 22107, 22148, 22187,
    22225, 22262, 22299, 22334, 22369, 22402, 22435, 22467, 22498, 22527, 22556, 22584, 22611,
    22637, 22662, 22686, 22709, 22731, 22752, 22772, 22791, 22809, 22826, 22842, 22857, 22872,
    22885, 22897, 22908, 22918, 22927, 22935, 22942, 22948, 22953, 22957, 22960, 22962, 22963,
];

#[derive(Clone)]
pub(super) struct Voice {
    pub(super) volume: [Volume; 2],
    pub(super) pitch: u16,
    /// Começo do sample (SSA) e endereço de repetição (LSAX), em unidades
    /// de 8 bytes.
    pub(super) start: u16,
    pub(super) repeat: u16,
    pub(super) adsr: Adsr,
    /// O LSAX escrito pela CPU depois do Key On vale mais que a marca de
    /// começo de loop do sample.
    ignore_loop_start: bool,
    /// Bloco atual, em unidades de 8 bytes.
    pub(super) address: u16,
    counter: u32,
    flags: u8,
    /// As 3 últimas amostras do bloco anterior e as 28 do atual.
    samples: [i16; 3 + BLOCK_SAMPLES as usize],
    /// Duas últimas amostras decodificadas, para o filtro.
    previous: [i32; 2],
    /// Última saída depois do envelope (para a modulação e a captura).
    pub(super) output: i16,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            volume: [Volume::default(); 2],
            pitch: 0,
            start: 0,
            repeat: 0,
            adsr: Adsr::default(),
            ignore_loop_start: false,
            address: 0,
            counter: 0,
            flags: 0,
            samples: [0; 3 + BLOCK_SAMPLES as usize],
            previous: [0; 2],
            output: 0,
        }
    }
}

impl Voice {
    pub(super) fn key_on(&mut self, ram: &[u16; RAM_HALFWORDS]) {
        self.address = self.start & !1;
        self.counter = 0;
        self.ignore_loop_start = false;
        self.samples = [0; 3 + BLOCK_SAMPLES as usize];
        self.previous = [0; 2];
        self.adsr.key_on();
        self.decode(ram);
    }

    pub(super) fn set_repeat(&mut self, value: u16) {
        self.repeat = value;
        self.ignore_loop_start = true;
    }

    /// Decodifica o bloco de `address` depois das 3 últimas amostras.
    fn decode(&mut self, ram: &[u16; RAM_HALFWORDS]) {
        let base = usize::from(self.address) * 4;
        let header = ram[base];
        let shift = match header & 0xF {
            // Valores reservados agem como 9.
            13.. => 9,
            shift => shift,
        };
        let filter = usize::from((header >> 4) & 7).min(4);
        self.flags = (header >> 8) as u8;
        if self.flags & FLAG_LOOP_START != 0 && !self.ignore_loop_start {
            self.repeat = self.address;
        }
        let (f0, f1) = (FILTER_POS[filter], FILTER_NEG[filter]);
        self.samples
            .copy_within(BLOCK_SAMPLES as usize..BLOCK_SAMPLES as usize + 3, 0);
        let [mut old, mut older] = self.previous;
        for i in 0..BLOCK_SAMPLES as usize {
            let word = ram[base + 1 + i / 4];
            let nibble = (word >> (i % 4 * 4)) & 0xF;
            let raw = i32::from(((nibble << 12) as i16) >> shift);
            let sample = (raw + ((old * f0 + older * f1 + 32) >> 6)).clamp(-0x8000, 0x7FFF);
            self.samples[3 + i] = sample as i16;
            older = old;
            old = sample;
        }
        self.previous = [old, older];
    }

    /// Amostra atual interpolada.
    fn interpolate(&self) -> i32 {
        let i = ((self.counter >> 4) & 0xFF) as usize;
        let s = (self.counter >> 12) as usize;
        let [oldest, older, old, new] = std::array::from_fn(|k| i32::from(self.samples[s + k]));
        ((GAUSS[0xFF - i] * oldest) >> 15)
            + ((GAUSS[0x1FF - i] * older) >> 15)
            + ((GAUSS[0x100 + i] * old) >> 15)
            + ((GAUSS[i] * new) >> 15)
    }

    /// Gera uma amostra e avança. `modulator` é a saída da voz anterior,
    /// quando a modulação de pitch está ligada; `noise` substitui o sample.
    /// Devolve `true` quando passou por um fim de loop (ENDX).
    pub(super) fn tick(
        &mut self,
        ram: &[u16; RAM_HALFWORDS],
        modulator: Option<i16>,
        noise: Option<i16>,
    ) -> bool {
        let sample = noise.map_or_else(|| self.interpolate(), i32::from);
        self.output = ((sample * i32::from(self.adsr.level)) >> 15) as i16;

        let mut step = u32::from(self.pitch);
        if let Some(m) = modulator {
            let factor = i32::from(m) + 0x8000;
            step = ((i32::from(self.pitch as i16) * factor) >> 15) as u32 & 0xFFFF;
        }
        self.counter += step.min(0x4000);
        let mut ended = false;
        if self.counter >= BLOCK_END {
            self.counter -= BLOCK_END;
            if self.flags & FLAG_LOOP_END != 0 {
                ended = true;
                self.address = self.repeat & !1;
                if self.flags & FLAG_LOOP_REPEAT == 0 && noise.is_none() {
                    self.adsr.silence();
                }
            } else {
                self.address = self.address.wrapping_add(2);
            }
            self.decode(ram);
        }
        self.adsr.tick();
        for volume in &mut self.volume {
            volume.tick();
        }
        ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RAM com um bloco em `at` (unidades de 8 bytes).
    fn block(ram: &mut [u16; RAM_HALFWORDS], at: usize, header: u16, nibbles: u16) {
        ram[at * 4] = header;
        ram[at * 4 + 1..at * 4 + 8].fill(nibbles);
    }

    fn ram() -> Box<[u16; RAM_HALFWORDS]> {
        vec![0; RAM_HALFWORDS].try_into().expect("tamanho")
    }

    #[test]
    fn decodes_with_shift_and_filter() {
        let mut ram = ram();
        // Shift 12 (sem ampliar) e filtro 0: as amostras são os nibbles.
        block(&mut ram, 2, 0x000C, 0x7171);
        // Shift 0, filtro 1: 1 << 12 mais 60/64 da anterior.
        block(&mut ram, 4, 0x0010, 0x0001);
        let mut voice = Voice {
            start: 2,
            ..Voice::default()
        };
        voice.key_on(&ram);
        assert_eq!(voice.samples[3..7], [1, 7, 1, 7]);
        voice.address = 4;
        voice.decode(&ram);
        assert_eq!(voice.samples[..3], [7, 1, 7], "histórico do bloco anterior");
        let first = 4096 + ((7 * 60 + 32) >> 6);
        assert_eq!(voice.samples[3], first as i16);
        assert_eq!(
            voice.samples[4],
            ((first * 60 + 32) >> 6) as i16,
            "só o filtro"
        );
    }

    #[test]
    fn interpolation_of_a_constant_is_almost_the_constant() {
        let mut voice = Voice {
            samples: [1000; 31],
            ..Voice::default()
        };
        for counter in [0, 0x10, 0x800, 0xFF0] {
            voice.counter = counter;
            let out = voice.interpolate();
            assert!((990..=1000).contains(&out), "{out}");
        }
    }

    #[test]
    fn loops_and_one_shot_end() {
        let mut ram = ram();
        // Bloco 0: começo de loop; bloco 2: fim com repetição.
        block(&mut ram, 0, 0x0400, 0);
        block(&mut ram, 2, 0x0300, 0);
        let mut voice = Voice {
            pitch: 0x4000,
            ..Voice::default()
        };
        voice.key_on(&ram);
        let ends: Vec<bool> = (0..14).map(|_| voice.tick(&ram, None, None)).collect();
        // 4 amostras por tick: 7 ticks por bloco.
        assert_eq!(voice.address, 0, "voltou ao começo do loop");
        assert_eq!(ends.iter().filter(|&&e| e).count(), 1);
        assert_eq!(voice.repeat, 0);
        assert_ne!(voice.adsr.phase, super::super::envelope::Phase::Release);

        // Fim sem repetição: silencia.
        block(&mut ram, 2, 0x0100, 0);
        voice.key_on(&ram);
        for _ in 0..14 {
            voice.tick(&ram, None, None);
        }
        assert_eq!(voice.adsr.level, 0);
        assert_eq!(voice.adsr.phase, super::super::envelope::Phase::Off);
    }

    #[test]
    fn repeat_written_by_cpu_wins_over_loop_start() {
        let mut ram = ram();
        block(&mut ram, 0, 0x0400, 0);
        block(&mut ram, 2, 0x0700, 0);
        let mut voice = Voice {
            pitch: 0x4000,
            ..Voice::default()
        };
        voice.key_on(&ram);
        voice.set_repeat(0x100);
        for _ in 0..14 {
            voice.tick(&ram, None, None);
        }
        assert_eq!(voice.address, 0x100);
    }

    #[test]
    fn pitch_modulation_scales_the_step() {
        let ram = ram();
        let mut voice = Voice {
            pitch: 0x1000,
            ..Voice::default()
        };
        voice.tick(&ram, Some(0), None);
        assert_eq!(voice.counter, 0x1000, "fator 1,0");
        voice.tick(&ram, Some(0x7FFF), None);
        assert_eq!(voice.counter, 0x1000 + 0x1FFF, "fator ~2,0");
        voice.tick(&ram, Some(-0x8000), None);
        assert_eq!(voice.counter, 0x2FFF, "fator 0");
    }
}
