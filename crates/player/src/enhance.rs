//! Filtros de melhoria de imagem, aplicados só na apresentação do quadro.
//!
//! - Scale2x/Scale3x (família EPX, de Andrea Mazzoleni/AdvanceMAME):
//!   amplia pixel art arredondando diagonais sem criar cores novas, então a
//!   paleta e a nitidez do original são preservadas.
//! - Bilinear: suaviza a ampliação de imagens 3D (serrilhado e pontilhado).
//!
//! Pixels em `0x00RRGGBB`. Nada aqui aloca: os buffers vêm de quem chama.

/// Amplia `src` (`w × h`, linhas de `stride` pixels) 2× para `dst`
/// (`2w × 2h`, contíguo). Vizinhos fora da imagem repetem a borda.
pub fn scale2x(src: &[u32], stride: usize, (w, h): (usize, usize), dst: &mut [u32]) {
    let out_w = 2 * w;
    for y in 0..h {
        let up = &src[y.saturating_sub(1) * stride..][..w];
        let row = &src[y * stride..][..w];
        let down = &src[(y + 1).min(h - 1) * stride..][..w];
        let (top, bottom) = dst[2 * y * out_w..][..2 * out_w].split_at_mut(out_w);
        for x in 0..w {
            let e = row[x];
            let b = up[x];
            let h = down[x];
            let d = row[x.saturating_sub(1)];
            let f = row[(x + 1).min(w - 1)];
            let (e0, e1, e2, e3) = if b != h && d != f {
                (
                    if d == b { d } else { e },
                    if b == f { f } else { e },
                    if d == h { d } else { e },
                    if h == f { f } else { e },
                )
            } else {
                (e, e, e, e)
            };
            top[2 * x] = e0;
            top[2 * x + 1] = e1;
            bottom[2 * x] = e2;
            bottom[2 * x + 1] = e3;
        }
    }
}

/// Amplia `src` 3× para `dst` (`3w × 3h`, contíguo), como [`scale2x`].
pub fn scale3x(src: &[u32], stride: usize, (w, h): (usize, usize), dst: &mut [u32]) {
    let out_w = 3 * w;
    for y in 0..h {
        let up = &src[y.saturating_sub(1) * stride..][..w];
        let row = &src[y * stride..][..w];
        let down = &src[(y + 1).min(h - 1) * stride..][..w];
        let lines = &mut dst[3 * y * out_w..][..3 * out_w];
        for x in 0..w {
            let (l, r) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let (a, b, c) = (up[l], up[x], up[r]);
            let (d, e, f) = (row[l], row[x], row[r]);
            let (g, h, i) = (down[l], down[x], down[r]);
            let block = if b != h && d != f {
                [
                    if d == b { d } else { e },
                    if (d == b && e != c) || (b == f && e != a) {
                        b
                    } else {
                        e
                    },
                    if b == f { f } else { e },
                    if (d == b && e != g) || (d == h && e != a) {
                        d
                    } else {
                        e
                    },
                    e,
                    if (b == f && e != i) || (h == f && e != c) {
                        f
                    } else {
                        e
                    },
                    if d == h { d } else { e },
                    if (d == h && e != i) || (h == f && e != g) {
                        h
                    } else {
                        e
                    },
                    if h == f { f } else { e },
                ]
            } else {
                [e; 9]
            };
            for (dy, chunk) in block.as_chunks::<3>().0.iter().enumerate() {
                lines[dy * out_w + 3 * x..][..3].copy_from_slice(chunk);
            }
        }
    }
}

/// Amostra de uma coordenada de destino: dois índices de origem e o peso
/// do segundo, em 1/256.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tap {
    pub first: usize,
    pub second: usize,
    pub weight: u32,
}

/// Amostras de `out` posições de destino sobre `len` de origem, com os
/// centros dos pixels alinhados (sem deslocar a imagem meio pixel).
pub fn taps(len: usize, out: usize, taps: &mut Vec<Tap>) {
    taps.clear();
    if len == 0 || out == 0 {
        return;
    }
    // Ponto fixo 16.8: posição de origem do centro de cada pixel de destino.
    let last = (len - 1) as u64;
    taps.extend((0..out as u64).map(|i| {
        let pos = ((2 * i + 1) * len as u64 * 256 / (2 * out as u64)).saturating_sub(128);
        let first = (pos >> 8).min(last);
        Tap {
            first: first as usize,
            second: (first + 1).min(last) as usize,
            weight: if first == last {
                0
            } else {
                (pos & 0xFF) as u32
            },
        }
    }));
}

/// Mistura `a` e `b` com o peso `w` (0..=256) em `b`, canal a canal.
#[inline]
pub fn lerp(a: u32, b: u32, w: u32) -> u32 {
    let inv = 256 - w;
    let rb = ((a & 0x00FF_00FF) * inv + (b & 0x00FF_00FF) * w) >> 8;
    let g = ((a & 0x0000_FF00) * inv + (b & 0x0000_FF00) * w) >> 8;
    (rb & 0x00FF_00FF) | (g & 0x0000_FF00)
}

/// Interpola uma linha de origem para a largura de `out`.
pub fn resample_row(src: &[u32], columns: &[Tap], out: &mut [u32]) {
    for (px, tap) in out.iter_mut().zip(columns) {
        *px = lerp(src[tap.first], src[tap.second], tap.weight);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: u32 = 0x000000;
    const X: u32 = 0xFFFFFF;

    /// Ampliação por repetição, a referência de "sem melhoria".
    fn nearest(src: &[u32], w: usize, h: usize, k: usize) -> Vec<u32> {
        (0..h * k)
            .flat_map(|y| (0..w * k).map(move |x| src[(y / k) * w + x / k]))
            .collect()
    }

    /// Dois pixels em diagonal num fundo liso.
    #[rustfmt::skip]
    const DIAGONAL: [u32; 16] = [
        O, O, O, O,
        O, X, O, O,
        O, O, X, O,
        O, O, O, O,
    ];

    #[test]
    fn scale2x_rounds_a_diagonal() {
        // Só a junção entre os dois pixels é preenchida.
        let mut expected = nearest(&DIAGONAL, 4, 4, 2);
        expected[3 * 8 + 4] = X;
        expected[4 * 8 + 3] = X;
        let mut dst = [7; 64];
        scale2x(&DIAGONAL, 4, (4, 4), &mut dst);
        assert_eq!(dst.to_vec(), expected);
    }

    #[test]
    fn scale2x_keeps_flat_areas_and_lines() {
        let src = [5; 6];
        let mut dst = [0; 24];
        scale2x(&src, 3, (3, 2), &mut dst);
        assert_eq!(dst, [5; 24]);

        // Linha vertical: nenhum pixel novo (b == h).
        #[rustfmt::skip]
        let src = [
            O, X, O,
            O, X, O,
            O, X, O,
        ];
        let mut dst = [7; 36];
        scale2x(&src, 3, (3, 3), &mut dst);
        for row in dst.as_chunks::<6>().0 {
            assert_eq!(*row, [O, O, X, X, O, O]);
        }
    }

    #[test]
    fn scale2x_uses_only_the_valid_part_of_each_row() {
        let src = [1, 2, 9, 3, 4, 9]; // 2 válidos de 3
        let mut dst = [0; 16];
        scale2x(&src, 3, (2, 2), &mut dst);
        assert!(!dst.contains(&9));
    }

    #[test]
    fn scale3x_rounds_a_diagonal_and_keeps_flat_areas() {
        let mut expected = nearest(&DIAGONAL, 4, 4, 3);
        expected[5 * 12 + 6] = X;
        expected[6 * 12 + 5] = X;
        let mut dst = [7; 144];
        scale3x(&DIAGONAL, 4, (4, 4), &mut dst);
        assert_eq!(dst.to_vec(), expected);

        let src = [5; 4];
        let mut dst = [0; 36];
        scale3x(&src, 2, (2, 2), &mut dst);
        assert_eq!(dst, [5; 36]);
    }

    #[test]
    fn scalers_never_invent_colors() {
        let src: Vec<u32> = (0..64u32).map(|i| (i * 7 + i / 3) % 4).collect();
        let mut two = vec![0; 64 * 4];
        let mut three = vec![0; 64 * 9];
        scale2x(&src, 8, (8, 8), &mut two);
        scale3x(&src, 8, (8, 8), &mut three);
        assert!(two.iter().chain(&three).all(|px| (0..4).contains(px)));
    }

    #[test]
    fn lerp_mixes_each_channel() {
        assert_eq!(lerp(0x102030, 0x907050, 0), 0x102030);
        assert_eq!(lerp(0x102030, 0x907050, 256), 0x907050);
        assert_eq!(lerp(0x000000, 0xFFFFFF, 128), 0x7F7F7F);
        assert_eq!(lerp(0xFF0000, 0x0000FF, 128), 0x7F007F);
    }

    #[test]
    fn taps_align_pixel_centers() {
        let mut out = Vec::new();
        // Mesma largura: cada pixel é ele mesmo.
        taps(3, 3, &mut out);
        assert!(
            out.iter()
                .enumerate()
                .all(|(i, t)| t.first == i && t.weight == 0)
        );
        // 2 → 4: bordas sem mistura, meio entre os dois.
        taps(2, 4, &mut out);
        let weights: Vec<_> = out.iter().map(|t| (t.first, t.second, t.weight)).collect();
        assert_eq!(weights, [(0, 1, 0), (0, 1, 64), (0, 1, 192), (1, 1, 0)]);
    }

    #[test]
    fn resample_row_interpolates() {
        let mut columns = Vec::new();
        taps(2, 4, &mut columns);
        let mut out = [0; 4];
        resample_row(&[0x000000, 0x0000FF], &columns, &mut out);
        assert_eq!(out, [0x00, 0x3F, 0xBF, 0xFF]);
    }
}
