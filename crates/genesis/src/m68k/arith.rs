//! Aritmética com flags, BCD e o tempo de MUL/DIV.

use super::Size;

/// Resultado de uma soma/subtração com os flags que ela determina.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Flags {
    pub result: u32,
    pub carry: bool,
    pub overflow: bool,
}

pub(crate) fn add(size: Size, src: u32, dst: u32, extend: bool) -> Flags {
    let mask = size.mask();
    let (s, d) = (src & mask, dst & mask);
    let wide = u64::from(s) + u64::from(d) + u64::from(extend);
    let result = (wide as u32) & mask;
    Flags {
        result,
        carry: wide > u64::from(mask),
        overflow: (s ^ result) & (d ^ result) & size.msb() != 0,
    }
}

/// `dst - src - extend`.
pub(crate) fn sub(size: Size, src: u32, dst: u32, extend: bool) -> Flags {
    let mask = size.mask();
    let (s, d) = (src & mask, dst & mask);
    let result = d.wrapping_sub(s).wrapping_sub(u32::from(extend)) & mask;
    Flags {
        result,
        carry: u64::from(s) + u64::from(extend) > u64::from(d),
        overflow: (s ^ d) & (result ^ d) & size.msb() != 0,
    }
}

/// Resultado BCD de 8 bits, com carry e os flags N/V não documentados
/// (comportamento medido no hardware por flamewing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bcd {
    pub result: u8,
    pub carry: bool,
    pub overflow: bool,
}

pub(crate) fn abcd(src: u8, dst: u8, extend: bool) -> Bcd {
    let (s, d, x) = (i32::from(src), i32::from(dst), i32::from(extend));
    let unadjusted = d + s + x;
    let mut result = (d & 0x0F) + (s & 0x0F) + x;
    // `>>` aritmético: (9 - r) negativo vira máscara cheia.
    result += (d & 0xF0) + (s & 0xF0) + (((9 - result) >> 4) & 0x06);
    result += ((0x9F - result) >> 4) & 0x60;
    Bcd {
        result: result as u8,
        carry: result & !0xFF != 0,
        overflow: !unadjusted & result & 0x80 != 0,
    }
}

pub(crate) fn sbcd(src: u8, dst: u8, extend: bool) -> Bcd {
    let (s, d, x) = (i32::from(src), i32::from(dst), i32::from(extend));
    let unadjusted = d - s - x;
    let top = (d & 0xF0) - (s & 0xF0) - (0x60 & (unadjusted >> 4));
    let mut result = (d & 0x0F) - (s & 0x0F) - x;
    let low_adjustment = 0x06 & (result >> 4);
    result = result - low_adjustment + top;
    let carry = (unadjusted - low_adjustment) & 0x300 != 0;
    Bcd {
        result: result as u8,
        carry,
        overflow: unadjusted & !result & 0x80 != 0,
    }
}

/// Ciclos internos de MULU: 38 + 2 por bit 1 no multiplicador.
pub(crate) fn mulu_cycles(src: u16) -> u32 {
    38 + 2 * src.count_ones()
}

/// Ciclos de MULS: 38 + 2 por transição 01/10 no multiplicador
/// (com um 0 implícito à direita).
pub(crate) fn muls_cycles(src: u16) -> u32 {
    let pattern = (u32::from(src) << 1) ^ u32::from(src);
    38 + 2 * (pattern & 0xFFFF).count_ones()
}

/// Ciclos totais de DIVU sem o EA (algoritmo de Jorge Cwik).
pub(crate) fn divu_cycles(dividend: u32, divisor: u16) -> u32 {
    let hdivisor = u32::from(divisor) << 16;
    if dividend >= hdivisor {
        return 10;
    }
    let mut mcycles = 38;
    let mut d = dividend;
    for _ in 0..15 {
        let temp = d;
        d <<= 1;
        if temp & 0x8000_0000 != 0 {
            d = d.wrapping_sub(hdivisor);
        } else {
            mcycles += 2;
            if d >= hdivisor {
                d = d.wrapping_sub(hdivisor);
                mcycles -= 1;
            }
        }
    }
    mcycles * 2
}

/// Ciclos totais de DIVS sem o EA (algoritmo de Jorge Cwik).
pub(crate) fn divs_cycles(dividend: i32, divisor: i16) -> u32 {
    let mut mcycles = 6;
    if dividend < 0 {
        mcycles += 1;
    }
    let abs_dividend = dividend.unsigned_abs();
    let abs_divisor = u32::from(divisor.unsigned_abs());
    if (abs_dividend >> 16) >= abs_divisor {
        return (mcycles + 2) * 2;
    }
    let mut aquot = abs_dividend / abs_divisor;
    mcycles += 55;
    if divisor >= 0 {
        if dividend >= 0 {
            mcycles -= 1;
        } else {
            mcycles += 1;
        }
    }
    for _ in 0..15 {
        if (aquot as i16) >= 0 {
            mcycles += 1;
        }
        aquot <<= 1;
    }
    mcycles * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_sub_flags() {
        let r = add(Size::Byte, 0x7F, 0x01, false);
        assert_eq!(
            r,
            Flags {
                result: 0x80,
                carry: false,
                overflow: true
            }
        );
        let r = add(Size::Word, 0xFFFF, 0x0001, false);
        assert_eq!(
            r,
            Flags {
                result: 0,
                carry: true,
                overflow: false
            }
        );
        let r = sub(Size::Long, 1, 0, false);
        assert_eq!(
            r,
            Flags {
                result: 0xFFFF_FFFF,
                carry: true,
                overflow: false
            }
        );
        let r = sub(Size::Byte, 0x01, 0x80, false);
        assert_eq!(
            r,
            Flags {
                result: 0x7F,
                carry: false,
                overflow: true
            }
        );
        let r = sub(Size::Byte, 0x00, 0x00, true);
        assert_eq!(r.result, 0xFF);
        assert!(r.carry);
    }

    #[test]
    fn bcd_add_and_subtract() {
        assert_eq!(abcd(0x15, 0x27, false).result, 0x42);
        let r = abcd(0x50, 0x60, false);
        assert_eq!((r.result, r.carry), (0x10, true));
        assert_eq!(sbcd(0x15, 0x42, false).result, 0x27);
        let r = sbcd(0x01, 0x00, false);
        assert_eq!((r.result, r.carry), (0x99, true));
    }

    #[test]
    fn multiply_timing() {
        assert_eq!(mulu_cycles(0), 38);
        assert_eq!(mulu_cycles(0xFFFF), 70);
        assert_eq!(muls_cycles(0), 38);
        assert_eq!(muls_cycles(0x5555), 70);
    }

    #[test]
    fn divide_timing_bounds() {
        assert_eq!(divu_cycles(0x0001_0000, 1), 10);
        let c = divu_cycles(1234, 7);
        assert!((76..=136).contains(&c), "{c}");
        let c = divs_cycles(-1234, 7);
        assert!((120..=158).contains(&c), "{c}");
    }
}
