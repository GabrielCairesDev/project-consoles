//! Reverb: roda a 22,05 kHz sobre uma área circular da RAM do SPU (de ESA
//! até o fim), com filtros FIR de 39 pontos na entrada e na saída.

use super::RAM_HALFWORDS;

const FIR: [i32; 39] = [
    -0x0001, 0, 0x0002, 0, -0x000A, 0, 0x0023, 0, -0x0067, 0, 0x010A, 0, -0x0268, 0, 0x0534, 0,
    -0x0B90, 0, 0x2806, 0x4000, 0x2806, 0, -0x0B90, 0, 0x0534, 0, -0x0268, 0, 0x010A, 0, -0x0067,
    0, 0x0023, 0, -0x000A, 0, 0x0002, 0, -0x0001,
];

const MASK: u32 = RAM_HALFWORDS as u32 - 1;

// Registradores (0x1F801DC0 + 2·n).
const D_APF1: usize = 0;
const D_APF2: usize = 1;
const V_IIR: usize = 2;
const V_COMB: usize = 3;
const V_WALL: usize = 7;
const V_APF1: usize = 8;
const V_APF2: usize = 9;
/// Pares esquerdo/direito.
const M_SAME: usize = 10;
const M_COMB1: usize = 12;
const M_COMB2: usize = 14;
const D_SAME: usize = 16;
const M_DIFF: usize = 18;
const M_COMB3: usize = 20;
const M_COMB4: usize = 22;
const D_DIFF: usize = 24;
const M_APF1: usize = 26;
const M_APF2: usize = 28;
const V_IN: usize = 30;

fn saturate(value: i32) -> i32 {
    value.clamp(-0x8000, 0x7FFF)
}

fn mul(a: i32, volume: u16) -> i32 {
    (a * i32::from(volume as i16)) >> 15
}

fn fir(history: &[i32; 39]) -> i32 {
    history.iter().zip(FIR).map(|(s, c)| s * c).sum::<i32>() >> 15
}

/// Empurra `value` no fim do histórico.
fn push(history: &mut [i32; 39], value: i32) {
    history.copy_within(1.., 0);
    history[38] = value;
}

#[derive(Clone)]
pub(super) struct Reverb {
    pub(super) registers: [u16; 32],
    /// Começo da área (ESA) e posição atual, em meias-palavras.
    base: u32,
    current: u32,
    input: [[i32; 39]; 2],
    output: [[i32; 39]; 2],
    /// Ciclo ímpar de 44,1 kHz: o reverb só calcula nos pares.
    odd: bool,
}

impl Default for Reverb {
    fn default() -> Self {
        Self {
            registers: [0; 32],
            base: 0,
            current: 0,
            input: [[0; 39]; 2],
            output: [[0; 39]; 2],
            odd: false,
        }
    }
}

impl Reverb {
    /// ESA (em unidades de 8 bytes): também reinicia a posição.
    pub(super) fn set_base(&mut self, value: u16) {
        self.base = u32::from(value) * 4;
        self.current = self.base;
    }

    fn reg(&self, index: usize) -> u16 {
        self.registers[index]
    }

    /// Endereço na área circular de um registrador de endereço, menos
    /// `back` meias-palavras.
    fn address(&self, index: usize, back: u32) -> usize {
        let offset = (u32::from(self.reg(index)) * 4).wrapping_sub(back) & MASK;
        let mut at = self.current + offset;
        if at > MASK {
            at = (at + self.base) & MASK;
        }
        at as usize
    }

    /// Recebe a soma das vozes com reverb e devolve a saída do reverb
    /// (antes do volume EVOL).
    pub(super) fn tick(
        &mut self,
        ram: &mut [u16; RAM_HALFWORDS],
        input: [i32; 2],
        writes: bool,
    ) -> [i32; 2] {
        for (history, value) in self.input.iter_mut().zip(input) {
            push(history, value);
        }
        self.odd = !self.odd;
        let computed = if self.odd {
            [0, 0]
        } else {
            let input = [fir(&self.input[0]), fir(&self.input[1])];
            let out = [0, 1].map(|side| self.side(ram, side, input, writes));
            self.current = (self.current + 1) & MASK;
            if self.current < self.base {
                self.current = self.base;
            }
            out
        };
        let mut out = [0; 2];
        for side in 0..2 {
            push(&mut self.output[side], computed[side]);
            // Metade das amostras são zeros: o filtro compensa com 2×.
            out[side] = saturate(fir(&self.output[side]) * 2);
        }
        out
    }

    fn side(
        &self,
        ram: &mut [u16; RAM_HALFWORDS],
        side: usize,
        input: [i32; 2],
        writes: bool,
    ) -> i32 {
        let read = |ram: &[u16; RAM_HALFWORDS], at: usize| i32::from(ram[at] as i16);
        let other = 1 - side;
        let r = |reg: usize| self.reg(reg);
        let lin = mul(input[side], r(V_IN + side));

        if writes {
            let iir = |ram: &[u16; RAM_HALFWORDS], target: usize, source: usize| {
                let prev = read(ram, self.address(target, 1));
                let wall = mul(read(ram, self.address(source, 0)), r(V_WALL));
                saturate(mul(lin + wall - prev, r(V_IIR)) + prev)
            };
            let same = iir(ram, M_SAME + side, D_SAME + side);
            let diff = iir(ram, M_DIFF + side, D_DIFF + other);
            ram[self.address(M_SAME + side, 0)] = same as u16;
            ram[self.address(M_DIFF + side, 0)] = diff as u16;
        }

        let combs = [M_COMB1, M_COMB2, M_COMB3, M_COMB4];
        let mut out = combs
            .iter()
            .enumerate()
            .map(|(k, &m)| mul(read(ram, self.address(m + side, 0)), r(V_COMB + k)))
            .sum::<i32>();
        for (m, d, v) in [(M_APF1, D_APF1, V_APF1), (M_APF2, D_APF2, V_APF2)] {
            let delayed_at = self.address(m + side, u32::from(r(d)) * 4);
            let delayed = read(ram, delayed_at);
            out = saturate(out - mul(delayed, r(v)));
            if writes {
                ram[self.address(m + side, 0)] = out as u16;
            }
            out = saturate(mul(out, r(v)) + delayed);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ram() -> Box<[u16; RAM_HALFWORDS]> {
        vec![0; RAM_HALFWORDS].try_into().expect("tamanho")
    }

    #[test]
    fn addresses_wrap_inside_the_work_area() {
        let mut reverb = Reverb::default();
        reverb.set_base(0x7000);
        reverb.registers[M_SAME] = 0x0FFF;
        assert_eq!(reverb.address(M_SAME, 0), 0x1C000 + 0x3FFC);
        reverb.current = MASK;
        assert_eq!(reverb.address(M_SAME, 0), 0x1C000 + 0x3FFB, "volta à base");
    }

    #[test]
    fn buffer_address_advances_at_22khz_and_wraps() {
        let mut reverb = Reverb::default();
        let mut ram = ram();
        reverb.set_base(0xFFFF);
        for _ in 0..8 {
            reverb.tick(&mut ram, [0, 0], false);
        }
        assert_eq!(reverb.current, reverb.base, "4 passos: deu a volta");
        reverb.tick(&mut ram, [0, 0], false);
        reverb.tick(&mut ram, [0, 0], false);
        assert_eq!(reverb.current, reverb.base + 1);
    }

    #[test]
    fn input_is_delayed_into_the_buffer_and_echoes() {
        let mut reverb = Reverb::default();
        let mut ram = ram();
        reverb.set_base(0x4000);
        // Só o caminho "same side" com eco, lido de volta por COMB1.
        reverb.registers[V_IN] = 0x7FFF;
        reverb.registers[V_IN + 1] = 0x7FFF;
        reverb.registers[V_IIR] = 0x7FFF;
        reverb.registers[M_SAME] = 0x10;
        reverb.registers[M_SAME + 1] = 0x20;
        let mut peak = 0;
        for tick in 0..400 {
            let input = if tick < 40 { [0x4000, 0] } else { [0, 0] };
            let out = reverb.tick(&mut ram, input, true);
            peak = peak.max(out[0]);
            assert_eq!(out[1], 0, "o lado direito não recebeu nada");
        }
        assert!(ram[0x10000..].iter().any(|&s| s != 0), "gravou na área");
        assert_eq!(peak, 0, "sem volume nos COMB, nada volta");

        reverb.registers[V_COMB] = 0x7FFF;
        reverb.registers[M_COMB1] = 0x10;
        let mut heard = false;
        for tick in 0..400 {
            let input = if tick < 40 { [0x4000, 0] } else { [0, 0] };
            heard |= reverb.tick(&mut ram, input, true)[0] > 0x1000;
        }
        assert!(heard);
    }
}
