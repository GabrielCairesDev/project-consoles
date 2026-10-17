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

/// Como [`taps`], mas "sharp-bilinear": cada pixel de origem ocupa um bloco
/// do tamanho da escala inteira (vizinho mais próximo) e só a faixa entre
/// blocos é interpolada. Em escala inteira é idêntico ao vizinho mais
/// próximo; fora dela, os pixels ficam do mesmo tamanho sem tremer.
pub fn sharp_taps(len: usize, out: usize, taps: &mut Vec<Tap>) {
    taps.clear();
    if len == 0 || out == 0 {
        return;
    }
    // Calculado uma vez por tamanho de janela, não por pixel: f64 basta.
    let scale = (out / len).max(1) as f64;
    let region = 0.5 - 0.5 / scale;
    let last = len - 1;
    taps.extend((0..out).map(|i| {
        let texel = (2 * i + 1) as f64 * len as f64 / (2 * out) as f64;
        let center = texel.floor();
        let offset = texel - center - 0.5;
        let sharpened = (offset - offset.clamp(-region, region)) * scale + 0.5;
        // Posição a amostrar, em coordenadas de centro de pixel.
        let pos = (center + sharpened - 0.5).max(0.0);
        let first = (pos.floor() as usize).min(last);
        let weight = ((pos - pos.floor()) * 256.0).round() as u32;
        match weight {
            _ if first == last => Tap {
                first,
                second: first,
                weight: 0,
            },
            256 => Tap {
                first: first + 1,
                second: (first + 2).min(last),
                weight: 0,
            },
            _ => Tap {
                first,
                second: first + 1,
                weight,
            },
        }
    }));
}

/// Troca cada cor de `from` pela de mesma posição em `to` (etapa de cor);
/// as outras passam iguais. Copia `frame` de `src` (linhas de `stride`)
/// para `dst`, sem folga entre as linhas.
pub fn recolor(
    src: &[u32],
    stride: usize,
    (w, h): (usize, usize),
    from: &[u32; 4],
    to: &[u32; 4],
    dst: &mut Vec<u32>,
) {
    dst.clear();
    for row in src.chunks(stride).take(h) {
        dst.extend(row[..w].iter().map(|&px| {
            from.iter()
                .position(|&shade| shade == px)
                .map_or(px, |i| to[i])
        }));
    }
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

/// Média de dois quadros do mesmo tamanho, canal a canal (frame blending:
/// o rastro do LCD dos portáteis). Escreve em `current`.
pub fn blend_frames(current: &mut [u32], previous: &[u32]) {
    for (px, &old) in current.iter_mut().zip(previous) {
        *px = lerp(*px, old, 128);
    }
}

/// Sinal composto simplificado: cada pixel mistura 1-2-1 com os vizinhos da
/// linha, como a banda estreita do vídeo composto. Funde o pontilhado de
/// colunas alternadas (transparência dos jogos da época) numa cor só.
/// Copia `frame` de `src` (linhas de `stride`) para `dst`, contíguo.
pub fn composite(src: &[u32], stride: usize, (w, h): (usize, usize), dst: &mut Vec<u32>) {
    dst.clear();
    for row in src.chunks(stride).take(h) {
        let row = &row[..w];
        dst.extend((0..w).map(|x| {
            let left = row[x.saturating_sub(1)];
            let right = row[(x + 1).min(w - 1)];
            // (left + 2·centro + right) / 4 = média de (média dos vizinhos) e centro.
            lerp(row[x], lerp(left, right, 128), 128)
        }));
    }
}

/// Escurece `px` pelos fatores de cada canal (256 = sem mudança).
#[inline]
fn shade_px(px: u32, [r, g, b]: [u32; 3]) -> u32 {
    let red = (((px >> 16) & 0xFF) * r) >> 8;
    let green = (((px >> 8) & 0xFF) * g) >> 8;
    let blue = ((px & 0xFF) * b) >> 8;
    (red << 16) | (green << 8) | blue
}

/// Aplica a etapa de tela na parte visível do quadro já ampliado: cada
/// linha de `dst` dentro de `area` (`x`, `y`, largura, altura) é escurecida
/// pelo fator da linha (`rows`, 256 = sem mudança) e, se houver, pelos
/// fatores por canal da coluna (`columns`).
pub fn shade(
    dst: &mut [u32],
    window_width: usize,
    (x, y, width, height): (usize, usize, usize, usize),
    rows: &[u32],
    columns: Option<&[[u32; 3]]>,
) {
    for (row, &factor) in rows.iter().enumerate().take(height) {
        let start = (y + row) * window_width + x;
        let line = &mut dst[start..start + width];
        match columns {
            None if factor == 256 => {}
            None => {
                for px in line {
                    *px = shade_px(*px, [factor; 3]);
                }
            }
            Some(columns) => {
                for (px, column) in line.iter_mut().zip(columns) {
                    let mixed = column.map(|c| (c * factor) >> 8);
                    *px = shade_px(*px, mixed);
                }
            }
        }
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
    fn sharp_taps_match_nearest_at_integer_scales() {
        let mut out = Vec::new();
        for (len, k) in [(2, 1), (2, 3), (5, 4), (160, 6)] {
            sharp_taps(len, len * k, &mut out);
            for (i, tap) in out.iter().enumerate() {
                assert_eq!((tap.first, tap.weight), (i / k, 0), "{len}×{k}, pixel {i}");
            }
        }
    }

    #[test]
    fn sharp_taps_blend_only_between_blocks() {
        // 2 → 5: escala inteira 2, sobra meio pixel; só a divisa mistura.
        let mut out = Vec::new();
        sharp_taps(2, 5, &mut out);
        let blend: Vec<_> = out.iter().map(|t| (t.first, t.weight)).collect();
        assert_eq!(blend, [(0, 0), (0, 0), (0, 128), (1, 0), (1, 0)]);
        // Abaixo de 2×, vira bilinear comum.
        let mut plain = Vec::new();
        taps(2, 3, &mut plain);
        sharp_taps(2, 3, &mut out);
        assert_eq!(out, plain);
        // Nunca lê fora da linha, nem reduzindo.
        for (len, n) in [(7, 3), (1, 9), (3, 1), (256, 1919)] {
            sharp_taps(len, n, &mut out);
            assert_eq!(out.len(), n);
            assert!(
                out.iter()
                    .all(|t| t.first < len && t.second < len && t.weight < 256)
            );
        }
    }

    #[test]
    fn recolor_swaps_only_the_listed_shades() {
        let from = [10, 20, 30, 40];
        let to = [1, 2, 3, 4];
        // 2×2 com folga de 1 pixel por linha (stride 3); 99 não é um tom.
        let src = [40, 10, 7, 99, 20, 7];
        let mut dst = Vec::new();
        recolor(&src, 3, (2, 2), &from, &to, &mut dst);
        assert_eq!(dst, [4, 1, 99, 2]);
        // Reaproveita o buffer sem sobras do quadro anterior.
        recolor(&[30], 1, (1, 1), &from, &to, &mut dst);
        assert_eq!(dst, [3]);
    }

    #[test]
    fn blend_frames_averages_each_channel() {
        let mut current = [0x00FF_0000, 0x0000_0000];
        blend_frames(&mut current, &[0x0000_00FF, 0x00FF_FFFF]);
        assert_eq!(current, [0x007F_007F, 0x007F_7F7F]);
    }

    #[test]
    fn composite_fuses_column_dithering_and_keeps_flat_areas() {
        // Pontilhado de colunas alternadas vira uma cor só no meio da linha.
        let src = [X, O, X, O, X, O];
        let mut dst = Vec::new();
        composite(&src, 6, (6, 1), &mut dst);
        assert_eq!(dst[1..5], [0x7F_7F7F; 4]);
        // Área lisa não muda; a folga do stride não é lida.
        composite(
            &[0x123456, 0x123456, 7, 0x123456, 0x123456, 7],
            3,
            (2, 2),
            &mut dst,
        );
        assert_eq!(dst, [0x123456; 4]);
    }

    #[test]
    fn shade_darkens_rows_and_columns_inside_the_area() {
        // Janela 4×2; área 2×2 a partir de x = 1.
        let mut dst = [X; 8];
        shade(&mut dst, 4, (1, 0, 2, 2), &[256, 128], None);
        assert_eq!(dst, [X, X, X, X, X, 0x7F_7F7F, 0x7F_7F7F, X]);
        let mut dst = [X; 4];
        let mask = [[256, 128, 128], [128, 256, 128]];
        shade(&mut dst, 2, (0, 0, 2, 2), &[256, 256], Some(&mask));
        assert_eq!(dst, [0xFF_7F7F, 0x7F_FF7F, 0xFF_7F7F, 0x7F_FF7F]);
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
