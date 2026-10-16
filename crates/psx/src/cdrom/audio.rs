//! Som do disco: o decodificador XA-ADPCM, que reamostra para 44,1 kHz, e
//! a matriz de volumes (ATV0–ATV3) aplicada a tudo que vai para o SPU.

use crate::disc::SECTOR_SIZE;

const POS: [i32; 4] = [0, 60, 115, 98];
const NEG: [i32; 4] = [0, 0, -52, -55];

/// Pedaços de 128 bytes num setor de som (0x900 bytes a partir do byte 24).
const PORTIONS: usize = 18;
const SAMPLES_PER_BLOCK: usize = 28;

/// Coeficientes do filtro "zigue-zague" de 25 pontos (psx-spx): cada linha
/// é um atraso (1..=29) e cada coluna uma das 7 saídas a 44,1 kHz geradas a
/// cada 6 amostras de 37,8 kHz. Cada coluna soma ~0x7400: o hardware
/// atenua o XA em ~0,9.
#[rustfmt::skip]
const ZIGZAG: [[i32; 7]; 29] = [
    [      0,       0,       0,       0, -0x0001,  0x0002, -0x0005],
    [      0,       0,       0, -0x0001,  0x0003, -0x0008,  0x0011],
    [      0,       0, -0x0001,  0x0003, -0x0008,  0x0010, -0x0023],
    [      0, -0x0002,  0x0003, -0x0008,  0x0011, -0x0023,  0x0046],
    [      0,       0, -0x0002,  0x0006, -0x0010,  0x002B, -0x0017],
    [-0x0002,  0x0003, -0x0005,  0x0005,  0x000A,  0x001A, -0x0044],
    [ 0x000A, -0x0013,  0x001F, -0x001B,  0x006B, -0x00EB,  0x015B],
    [-0x0022,  0x003C, -0x004A,  0x00A6, -0x016D,  0x027B, -0x0347],
    [ 0x0041, -0x004B,  0x00B3, -0x01A8,  0x0350, -0x0548,  0x080E],
    [-0x0054,  0x00A2, -0x0192,  0x0372, -0x0623,  0x0AFA, -0x1249],
    [ 0x0034, -0x00E3,  0x02B1, -0x05BF,  0x0BCD, -0x16FA,  0x3C07],
    [ 0x0009,  0x0132, -0x039E,  0x09B8, -0x1780,  0x53E0,  0x53E0],
    [-0x010A, -0x0043,  0x04F8, -0x11B4,  0x6794,  0x3C07, -0x16FA],
    [ 0x0400, -0x0267, -0x05A6,  0x74BB,  0x234C, -0x1249,  0x0AFA],
    [-0x0A78,  0x0C9D,  0x7939,  0x0C9D, -0x0A78,  0x080E, -0x0548],
    [ 0x234C,  0x74BB, -0x05A6, -0x0267,  0x0400, -0x0347,  0x027B],
    [ 0x6794, -0x11B4,  0x04F8, -0x0043, -0x010A,  0x015B, -0x00EB],
    [-0x1780,  0x09B8, -0x039E,  0x0132,  0x0009, -0x0044,  0x001A],
    [ 0x0BCD, -0x05BF,  0x02B1, -0x00E3,  0x0034, -0x0017,  0x002B],
    [-0x0623,  0x0372, -0x0192,  0x00A2, -0x0054,  0x0046, -0x0023],
    [ 0x0350, -0x01A8,  0x00B3, -0x004B,  0x0041, -0x0023,  0x0010],
    [-0x016D,  0x00A6, -0x004A,  0x003C, -0x0022,  0x0011, -0x0008],
    [ 0x006B, -0x001B,  0x001F, -0x0013,  0x000A, -0x0005,  0x0002],
    [ 0x000A,  0x0005, -0x0005,  0x0003, -0x0001,       0,       0],
    [-0x0010,  0x0006, -0x0002,       0,       0,       0,       0],
    [ 0x0011, -0x0008,  0x0003, -0x0002,  0x0001,       0,       0],
    [-0x0008,  0x0003, -0x0001,       0,       0,       0,       0],
    [ 0x0003, -0x0001,       0,       0,       0,       0,       0],
    [-0x0001,       0,       0,       0,       0,       0,       0],
];

fn saturate(value: i32) -> i16 {
    value.clamp(-0x8000, 0x7FFF) as i16
}

/// Codinginfo do subcabeçalho (byte 19 do setor).
#[derive(Clone, Copy)]
struct Coding {
    stereo: bool,
    /// 18 900 Hz em vez de 37 800 Hz.
    half_rate: bool,
    eight_bit: bool,
}

impl Coding {
    fn new(byte: u8) -> Self {
        Self {
            stereo: byte & 3 == 1,
            half_rate: (byte >> 2) & 3 == 1,
            eight_bit: (byte >> 4) & 3 == 1,
        }
    }
}

#[derive(Clone)]
pub(super) struct XaDecoder {
    /// Duas últimas amostras de cada canal, para o filtro.
    history: [[i32; 2]; 2],
    ring: [[i16; 32]; 2],
    position: usize,
    /// Amostras de 37,8 kHz até o próximo grupo de 7 saídas.
    sixstep: u8,
}

impl Default for XaDecoder {
    fn default() -> Self {
        Self {
            history: [[0; 2]; 2],
            ring: [[0; 32]; 2],
            position: 0,
            sixstep: 6,
        }
    }
}

impl XaDecoder {
    /// Decodifica um setor de som e entrega as amostras a 44,1 kHz.
    pub(super) fn decode(&mut self, sector: &[u8; SECTOR_SIZE], emit: &mut impl FnMut([i16; 2])) {
        let coding = Coding::new(sector[19]);
        let units = if coding.eight_bit { 4 } else { 8 };
        let per_side = if coding.stereo { units / 2 } else { units };
        let mut decoded = [[0i16; 8 * SAMPLES_PER_BLOCK]; 2];
        for portion in sector[24..24 + PORTIONS * 128].as_chunks::<128>().0 {
            let mut filled = [0; 2];
            for unit in 0..units {
                let channel = usize::from(coding.stereo && unit % 2 == 1);
                let at = filled[channel];
                let out = &mut decoded[channel][at..at + SAMPLES_PER_BLOCK];
                self.decode_block(portion, unit, coding.eight_bit, channel, out);
                filled[channel] += SAMPLES_PER_BLOCK;
            }
            let count = per_side * SAMPLES_PER_BLOCK;
            for (&left, &right) in decoded[0][..count].iter().zip(&decoded[1][..count]) {
                let right = if coding.stereo { right } else { left };
                let repeat = if coding.half_rate { 2 } else { 1 };
                for _ in 0..repeat {
                    self.resample([left, right], emit);
                }
            }
        }
    }

    /// 28 amostras de um bloco (`unit` 0..8 em 4 bits, 0..4 em 8 bits).
    fn decode_block(
        &mut self,
        portion: &[u8],
        unit: usize,
        eight_bit: bool,
        channel: usize,
        out: &mut [i16],
    ) {
        let header = portion[4 + unit];
        let shift = match header & 0xF {
            s @ 0..=12 => u32::from(s),
            _ => 9,
        };
        let filter = usize::from((header >> 4) & 3);
        let [mut old, mut older] = self.history[channel];
        for (j, sample) in out.iter_mut().enumerate() {
            let raw = if eight_bit {
                i32::from(portion[16 + unit + j * 4] as i8) << 8
            } else {
                let byte = portion[16 + unit / 2 + j * 4];
                let nibble = (byte >> (4 * (unit % 2))) & 0xF;
                i32::from((nibble << 4) as i8) << 8
            };
            let value = (raw >> shift) + ((old * POS[filter] + older * NEG[filter] + 32) >> 6);
            let value = saturate(value);
            *sample = value;
            older = old;
            old = i32::from(value);
        }
        self.history[channel] = [old, older];
    }

    /// Reamostra de 37,8 para 44,1 kHz: 7 saídas a cada 6 entradas.
    fn resample(&mut self, sample: [i16; 2], emit: &mut impl FnMut([i16; 2])) {
        for (ring, value) in self.ring.iter_mut().zip(sample) {
            ring[self.position] = value;
        }
        self.position = (self.position + 1) & 31;
        self.sixstep -= 1;
        if self.sixstep > 0 {
            return;
        }
        self.sixstep = 6;
        for table in 0..7 {
            emit([0, 1].map(|channel| self.interpolate(channel, table)));
        }
    }

    fn interpolate(&self, channel: usize, table: usize) -> i16 {
        let ring = &self.ring[channel];
        // Cabe em i32: a maior soma dos módulos de uma coluna é 56 030.
        let sum: i32 = ZIGZAG
            .iter()
            .enumerate()
            .map(|(i, row)| i32::from(ring[(self.position + 31 - i) & 31]) * row[table])
            .sum();
        (sum >> 15).clamp(-0x8000, 0x7FFF) as i16
    }
}

/// Volumes do som do CD para cada saída (ATV0–ATV3): 0x80 é o normal.
#[derive(Clone, Copy)]
pub(super) struct Volumes {
    /// Esquerda→esquerda, esquerda→direita, direita→direita,
    /// direita→esquerda.
    pub(super) pending: [u8; 4],
    active: [u8; 4],
}

impl Default for Volumes {
    fn default() -> Self {
        Self {
            pending: [0x80, 0, 0x80, 0],
            active: [0x80, 0, 0x80, 0],
        }
    }
}

impl Volumes {
    /// ADPCTL bit 5: os volumes escritos passam a valer.
    pub(super) fn apply(&mut self) {
        self.active = self.pending;
    }

    pub(super) fn mix(&self, [left, right]: [i16; 2]) -> [i16; 2] {
        let [ll, lr, rr, rl] = self.active.map(i32::from);
        let (left, right) = (i32::from(left), i32::from(right));
        [
            saturate((left * ll + right * rl) >> 7),
            saturate((right * rr + left * lr) >> 7),
        ]
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Setor de som XA com o mesmo cabeçalho e o mesmo byte de dados em
    /// todos os blocos.
    pub(crate) fn xa_sector(coding: u8, header: u8, data: u8) -> [u8; SECTOR_SIZE] {
        let mut sector = [0; SECTOR_SIZE];
        sector[15] = 2;
        sector[18] = 0x64;
        sector[19] = coding;
        for portion in sector[24..24 + PORTIONS * 128].as_chunks_mut::<128>().0 {
            portion[..16].fill(header);
            portion[16..].fill(data);
        }
        sector
    }

    fn decode(sector: &[u8; SECTOR_SIZE]) -> Vec<[i16; 2]> {
        let mut decoder = XaDecoder::default();
        let mut out = Vec::new();
        decoder.decode(sector, &mut |s| out.push(s));
        out
    }

    #[test]
    fn sample_counts_follow_the_coding_info() {
        // 4032 amostras mono a 37,8 kHz viram 4704 a 44,1 kHz.
        assert_eq!(decode(&xa_sector(0x00, 0, 0)).len(), 4704);
        assert_eq!(decode(&xa_sector(0x01, 0, 0)).len(), 2352, "estéreo");
        assert_eq!(decode(&xa_sector(0x05, 0, 0)).len(), 4704, "18,9 kHz");
        assert_eq!(decode(&xa_sector(0x10, 0, 0)).len(), 2352, "8 bits");
    }

    #[test]
    fn constant_input_settles_at_the_expected_level() {
        // Shift 0, filtro 0: o nibble 1 vira 0x1000, que sai a ~0,907.
        let out = decode(&xa_sector(0x00, 0x00, 0x11));
        let last = out[out.len() - 1];
        assert!((0x0E70..=0x0E90).contains(&last[0]), "{:#x}", last[0]);
        assert_eq!(last[0], last[1], "mono nos dois lados");
    }

    #[test]
    fn stereo_splits_low_and_high_nibbles() {
        // Nibble baixo (esquerda) 1, alto (direita) -1.
        let out = decode(&xa_sector(0x01, 0x00, 0xF1));
        let last = out[out.len() - 1];
        assert!(last[0] > 0x0E00 && last[1] < -0x0E00, "{last:?}");
    }

    #[test]
    fn filter_uses_the_previous_samples() {
        // Filtro 1 (60/64 da anterior) com entrada constante acumula até
        // 16× o valor da entrada (shift 8: 0x10), ~0xE8 na saída.
        let out = decode(&xa_sector(0x00, 0x18, 0x11));
        let last = out[out.len() - 1][0];
        assert!((0x00E0..=0x00F0).contains(&last), "{last:#x}");
    }

    #[test]
    fn volume_matrix_mixes_and_saturates() {
        let mut volumes = Volumes::default();
        assert_eq!(volumes.mix([1000, -2000]), [1000, -2000]);
        volumes.pending = [0x40, 0x40, 0x40, 0x40];
        assert_eq!(volumes.mix([1000, -2000]), [1000, -2000], "sem aplicar");
        volumes.apply();
        assert_eq!(volumes.mix([1000, -2000]), [-500, -500], "mono");
        volumes.pending = [0xFF, 0xFF, 0xFF, 0xFF];
        volumes.apply();
        assert_eq!(volumes.mix([0x7000, 0x7000]), [0x7FFF, 0x7FFF]);
    }
}
