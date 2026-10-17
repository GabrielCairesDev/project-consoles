//! MMPX: ampliação 2× de pixel art que preserva o estilo (curvas, diagonais
//! de 1:1 e 2:1, cantos vivos e detalhes de um pixel), sem criar cores.
//!
//! Porte direto da implementação C++ de referência do artigo "MMPX
//! Style-Preserving Pixel Art Magnification" (Journal of Computer Graphics
//! Techniques, 2021), `code/source/cppPerf.cpp` do material suplementar:
//! <https://casual-effects.com/research/McGuire2021PixelArt/>.
//!
//! Copyright 2020 Morgan McGuire & Mara Gagiu. Provided under the Open
//! Source MIT license <https://opensource.org/licenses/MIT>:
//!
//! Permission is hereby granted, free of charge, to any person obtaining a
//! copy of this software and associated documentation files (the
//! "Software"), to deal in the Software without restriction, including
//! without limitation the rights to use, copy, modify, merge, publish,
//! distribute, sublicense, and/or sell copies of the Software, and to
//! permit persons to whom the Software is furnished to do so, subject to
//! the following conditions:
//!
//! The above copyright notice and this permission notice shall be included
//! in all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
//! OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//! MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
//! IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//! CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
//! TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
//! SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//!
//! Diferenças do original: pixels em `0x00RRGGBB` (o byte alto é sempre 0,
//! então o fator de transparência da luminância é constante e não muda as
//! comparações) e linhas de origem com `stride` próprio. A lógica das
//! regras é a mesma, na mesma ordem.

/// Luminância usada só para comparar cores (soma dos canais, como no
/// original, multiplicada pelo fator de transparência).
#[inline]
fn luma(color: u32) -> u32 {
    let alpha = (color & 0xFF00_0000) >> 24;
    (((color & 0x00FF_0000) >> 16) + ((color & 0x0000_FF00) >> 8) + (color & 0x0000_00FF) + 1)
        * (256 - alpha)
}

#[inline]
fn all_eq2(b: u32, a0: u32, a1: u32) -> bool {
    ((b ^ a0) | (b ^ a1)) == 0
}

#[inline]
fn all_eq3(b: u32, a0: u32, a1: u32, a2: u32) -> bool {
    ((b ^ a0) | (b ^ a1) | (b ^ a2)) == 0
}

#[inline]
fn all_eq4(b: u32, a0: u32, a1: u32, a2: u32, a3: u32) -> bool {
    ((b ^ a0) | (b ^ a1) | (b ^ a2) | (b ^ a3)) == 0
}

#[inline]
fn any_eq3(b: u32, a0: u32, a1: u32, a2: u32) -> bool {
    b == a0 || b == a1 || b == a2
}

#[inline]
fn none_eq2(b: u32, a0: u32, a1: u32) -> bool {
    b != a0 && b != a1
}

#[inline]
fn none_eq4(b: u32, a0: u32, a1: u32, a2: u32, a3: u32) -> bool {
    b != a0 && b != a1 && b != a2 && b != a3
}

/// Amplia `src` (`w × h`, linhas de `stride` pixels) 2× para `dst`
/// (`2w × 2h`, contíguo). Vizinhos fora da imagem repetem a borda.
pub fn mmpx2x(src: &[u32], stride: usize, (w, h): (usize, usize), dst: &mut [u32]) {
    if w == 0 || h == 0 {
        return;
    }
    let (max_x, max_y) = (w as isize - 1, h as isize - 1);
    let at = |x: isize, y: isize| -> u32 {
        let (x, y) = (x.clamp(0, max_x) as usize, y.clamp(0, max_y) as usize);
        src[y * stride + x]
    };
    let out_w = 2 * w;

    for y in 0..h as isize {
        // Vizinhança carregada ao longo da linha.
        let mut a = at(-1, y - 1);
        let mut b = at(0, y - 1);
        let mut c = at(1, y - 1);
        let mut d = at(-1, y);
        let mut e = at(0, y);
        let mut f = at(1, y);
        let mut g = at(-1, y + 1);
        let mut h_ = at(0, y + 1);
        let mut i = at(1, y + 1);
        let mut q = at(-2, y);
        let mut r = at(2, y);

        for x in 0..w as isize {
            let (mut j, mut k, mut l, mut m) = (e, e, e, e);

            if ((a ^ e) | (b ^ e) | (c ^ e) | (d ^ e) | (f ^ e) | (g ^ e) | (h_ ^ e) | (i ^ e)) != 0
            {
                let p = at(x, y - 2);
                let s = at(x, y + 2);
                let (bl, dl, el, fl, hl) = (luma(b), luma(d), luma(e), luma(f), luma(h_));

                // Regras de inclinação 1:1.
                if (d == b && d != h_ && d != f)
                    && (el >= dl || e == a)
                    && any_eq3(e, a, c, g)
                    && (el < dl || a != d || e != p || e != q)
                {
                    j = d;
                }
                if (b == f && b != d && b != h_)
                    && (el >= bl || e == c)
                    && any_eq3(e, a, c, i)
                    && (el < bl || c != b || e != p || e != r)
                {
                    k = b;
                }
                if (h_ == d && h_ != f && h_ != b)
                    && (el >= hl || e == g)
                    && any_eq3(e, a, g, i)
                    && (el < hl || g != h_ || e != s || e != q)
                {
                    l = h_;
                }
                if (f == h_ && f != b && f != d)
                    && (el >= fl || e == i)
                    && any_eq3(e, c, g, i)
                    && (el < fl || i != h_ || e != r || e != s)
                {
                    m = f;
                }

                // Regras de interseção.
                if (e != f && all_eq4(e, c, i, d, q) && all_eq2(f, b, h_)) && f != at(x + 3, y) {
                    k = f;
                    m = f;
                }
                if (e != d && all_eq4(e, a, g, f, r) && all_eq2(d, b, h_)) && d != at(x - 3, y) {
                    j = d;
                    l = d;
                }
                if (e != h_ && all_eq4(e, g, i, b, p) && all_eq2(h_, d, f)) && h_ != at(x, y + 3) {
                    l = h_;
                    m = h_;
                }
                if (e != b && all_eq4(e, a, c, h_, s) && all_eq2(b, d, f)) && b != at(x, y - 3) {
                    j = b;
                    k = b;
                }
                if bl < el && all_eq4(e, g, h_, i, s) && none_eq4(e, a, d, c, f) {
                    j = b;
                    k = b;
                }
                if hl < el && all_eq4(e, a, b, c, p) && none_eq4(e, d, g, i, f) {
                    l = h_;
                    m = h_;
                }
                if fl < el && all_eq4(e, a, d, g, q) && none_eq4(e, b, c, i, h_) {
                    k = f;
                    m = f;
                }
                if dl < el && all_eq4(e, c, f, i, r) && none_eq4(e, b, a, g, h_) {
                    j = d;
                    l = d;
                }

                // Regras de inclinação 2:1.
                if h_ != b {
                    if h_ != a && h_ != e && h_ != c {
                        if all_eq3(h_, g, f, r) && none_eq2(h_, d, at(x + 2, y - 1)) {
                            l = m;
                        }
                        if all_eq3(h_, i, d, q) && none_eq2(h_, f, at(x - 2, y - 1)) {
                            m = l;
                        }
                    }
                    if b != i && b != g && b != e {
                        if all_eq3(b, a, f, r) && none_eq2(b, d, at(x + 2, y + 1)) {
                            j = k;
                        }
                        if all_eq3(b, c, d, q) && none_eq2(b, f, at(x - 2, y + 1)) {
                            k = j;
                        }
                    }
                }
                if f != d {
                    if d != i && d != e && d != c {
                        if all_eq3(d, a, h_, s) && none_eq2(d, b, at(x + 1, y + 2)) {
                            j = l;
                        }
                        if all_eq3(d, g, b, p) && none_eq2(d, h_, at(x + 1, y - 2)) {
                            l = j;
                        }
                    }
                    if f != e && f != a && f != g {
                        if all_eq3(f, c, h_, s) && none_eq2(f, b, at(x - 1, y + 2)) {
                            k = m;
                        }
                        if all_eq3(f, i, b, p) && none_eq2(f, h_, at(x - 1, y - 2)) {
                            m = k;
                        }
                    }
                }
            }

            let top = 2 * y as usize * out_w + 2 * x as usize;
            dst[top] = j;
            dst[top + 1] = k;
            dst[top + out_w] = l;
            dst[top + out_w + 1] = m;

            a = b;
            b = c;
            c = at(x + 2, y - 1);
            q = d;
            d = e;
            e = f;
            f = r;
            r = at(x + 3, y);
            g = h_;
            h_ = i;
            i = at(x + 2, y + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: u32 = 0x000000;
    const X: u32 = 0xFFFFFF;

    fn run(src: &[u32], w: usize, h: usize) -> Vec<u32> {
        let mut dst = vec![0xDEAD; 4 * w * h];
        mmpx2x(src, w, (w, h), &mut dst);
        dst
    }

    fn nearest2x(src: &[u32], w: usize, h: usize) -> Vec<u32> {
        let mut dst = Vec::with_capacity(4 * w * h);
        for y in 0..2 * h {
            dst.extend((0..2 * w).map(|x| src[y / 2 * w + x / 2]));
        }
        dst
    }

    #[test]
    fn flat_areas_and_single_pixels_stay_as_nearest() {
        assert_eq!(run(&[X; 12], 4, 3), vec![X; 48]);
        // Um pixel isolado é um detalhe de um pixel: não é arredondado.
        #[rustfmt::skip]
        let dot = [
            O, O, O,
            O, X, O,
            O, O, O,
        ];
        assert_eq!(run(&dot, 3, 3), nearest2x(&dot, 3, 3));
    }

    /// Desenha uma imagem `#`/`.` como pixels claros/escuros.
    fn image(rows: &[&str]) -> (Vec<u32>, usize, usize) {
        let w = rows[0].len();
        let pixels = rows
            .iter()
            .flat_map(|row| row.chars().map(|c| if c == '#' { X } else { O }))
            .collect();
        (pixels, w, rows.len())
    }

    #[test]
    fn diagonal_line_is_reconstructed_without_new_colors() {
        let (src, w, h) = image(&["#....", ".#...", "..#..", "...#.", "....#"]);
        // Diagonal 1:1 vira uma linha contínua de largura constante; as
        // pontas encostam na borda (que se repete) e mantêm o canto.
        let (expected, _, _) = image(&[
            "##........",
            "#.#.......",
            ".###......",
            "..###.....",
            "...###....",
            "....###...",
            ".....###..",
            "......###.",
            ".......#.#",
            "........##",
        ]);
        assert_eq!(run(&src, w, h), expected);
    }

    #[test]
    fn staircase_2_to_1_is_smoothed() {
        // Degraus de dois pixels: o canto de fora de cada degrau é aparado
        // (regra 1:1 seguida da regra 2:1 "B, C, D e Q iguais"), conferido à
        // mão pelas regras da referência.
        let (src, w, h) = image(&["....##", "..####", "######"]);
        let (expected, _, _) = image(&[
            "........####",
            "........####",
            "......######",
            "....########",
            "############",
            "############",
        ]);
        assert_eq!(run(&src, w, h), expected);
    }

    #[test]
    fn respects_stride_and_borders() {
        // 2×2 dentro de linhas de 3 (a terceira coluna é lixo).
        let src = [X, O, 0x123456, O, X, 0x123456];
        let mut dst = [0; 16];
        mmpx2x(&src, 3, (2, 2), &mut dst);
        assert!(dst.iter().all(|&c| c == O || c == X), "não lê a folga");
        // Imagem de 1 pixel: só repete.
        let mut one = [0; 4];
        mmpx2x(&[X], 1, (1, 1), &mut one);
        assert_eq!(one, [X; 4]);
        // Tamanho zero: nada a fazer.
        mmpx2x(&[], 0, (0, 0), &mut []);
    }
}
