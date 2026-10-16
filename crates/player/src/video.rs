//! Escala inteira do quadro para a janela, centralizada com barras pretas.

/// Retângulo de destino do quadro dentro da janela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub scale: usize,
    pub x: usize,
    pub y: usize,
}

/// Maior escala inteira que cabe na janela (no mínimo 1, cortando se a
/// janela for menor que o quadro).
pub fn layout(window: (usize, usize), frame: (usize, usize)) -> Layout {
    let scale = (window.0 / frame.0).min(window.1 / frame.1).max(1);
    Layout {
        scale,
        x: window.0.saturating_sub(frame.0 * scale) / 2,
        y: window.1.saturating_sub(frame.1 * scale) / 2,
    }
}

/// Copia `src` (`frame.0 × frame.1`, linhas de `stride` pixels) ampliado
/// para `dst` (`window.0` de largura), por vizinho mais próximo. O resto de
/// `dst` fica preto.
pub fn blit(
    src: &[u32],
    stride: usize,
    frame: (usize, usize),
    dst: &mut [u32],
    window: (usize, usize),
) {
    dst.fill(0);
    let Layout { scale, x, y } = layout(window, frame);
    let visible_w = (frame.0 * scale).min(window.0 - x);
    for (row, src_row) in src.chunks(stride).take(frame.1).enumerate() {
        for rep in 0..scale {
            let dy = y + row * scale + rep;
            if dy >= window.1 {
                return;
            }
            let start = dy * window.0 + x;
            let line = &mut dst[start..start + visible_w];
            for (i, px) in line.iter_mut().enumerate() {
                *px = src_row[i / scale];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_largest_integer_scale_and_centers() {
        assert_eq!(
            layout((800, 600), (256, 192)),
            Layout {
                scale: 3,
                x: 16,
                y: 12
            }
        );
        assert_eq!(
            layout((256, 192), (256, 192)),
            Layout {
                scale: 1,
                x: 0,
                y: 0
            }
        );
        assert_eq!(
            layout((100, 100), (256, 192)),
            Layout {
                scale: 1,
                x: 0,
                y: 0
            }
        );
    }

    #[test]
    fn blit_scales_pixels_and_letterboxes() {
        let src = [1, 2, 3, 4]; // 2×2
        let mut dst = [9; 6 * 4];
        blit(&src, 2, (2, 2), &mut dst, (6, 4));
        #[rustfmt::skip]
        let expected = [
            0, 1, 1, 2, 2, 0,
            0, 1, 1, 2, 2, 0,
            0, 3, 3, 4, 4, 0,
            0, 3, 3, 4, 4, 0,
        ];
        assert_eq!(dst, expected);
    }

    #[test]
    fn blit_crops_when_window_is_smaller() {
        let src = [1, 2, 3, 4];
        let mut dst = [9; 1];
        blit(&src, 2, (2, 2), &mut dst, (1, 1));
        assert_eq!(dst, [1]);
    }

    #[test]
    fn blit_uses_only_the_valid_part_of_each_row() {
        // Linhas de 3 pixels, só 2 válidos (como o H32 no buffer de H40).
        let src = [1, 2, 7, 3, 4, 7];
        let mut dst = [9; 4];
        blit(&src, 3, (2, 2), &mut dst, (2, 2));
        assert_eq!(dst, [1, 2, 3, 4]);
    }
}
