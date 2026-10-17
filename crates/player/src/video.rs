//! Escala do quadro para a janela, centralizada com barras pretas.
//!
//! Sem proporção definida a escala é inteira (pixels nítidos e iguais);
//! com proporção, o quadro ocupa o maior retângulo dela que cabe.

/// Retângulo de destino do quadro dentro da janela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// Onde o quadro é desenhado. Escala inteira: a maior que cabe (no mínimo
/// 1, cortando se a janela for menor que o quadro). Com `aspect`: o maior
/// retângulo nessa proporção dentro da janela.
pub fn layout(
    window: (usize, usize),
    frame: (usize, usize),
    aspect: Option<(usize, usize)>,
) -> Layout {
    let (width, height) = match aspect {
        None => {
            let scale = (window.0 / frame.0).min(window.1 / frame.1).max(1);
            (frame.0 * scale, frame.1 * scale)
        }
        Some((aw, ah)) => {
            if window.0 * ah <= window.1 * aw {
                (window.0, window.0 * ah / aw)
            } else {
                (window.1 * aw / ah, window.1)
            }
        }
    };
    Layout {
        x: window.0.saturating_sub(width) / 2,
        y: window.1.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// Copia `src` (`frame.0 × frame.1`, linhas de `stride` pixels) ampliado
/// para `dst` (`window.0` de largura), por vizinho mais próximo. O resto de
/// `dst` fica preto.
pub fn blit(
    src: &[u32],
    stride: usize,
    frame: (usize, usize),
    aspect: Option<(usize, usize)>,
    dst: &mut [u32],
    window: (usize, usize),
) {
    dst.fill(0);
    let Layout {
        x,
        y,
        width,
        height,
    } = layout(window, frame, aspect);
    if width == 0 || height == 0 || frame.0 == 0 || frame.1 == 0 {
        return;
    }
    let visible_w = width.min(window.0 - x);
    let visible_h = height.min(window.1 - y);
    // Coluna de origem em ponto fixo 32.32, amostrando o centro de cada
    // pixel de destino: evita uma divisão por pixel e é exata nas escalas
    // inteiras (o erro acumulado fica abaixo de meio passo).
    let step = ((frame.0 as u64) << 32) / width as u64;
    for row in 0..visible_h {
        let src_row = (2 * row + 1) * frame.1 / (2 * height);
        let src_line = &src[src_row * stride..src_row * stride + frame.0];
        let start = (y + row) * window.0 + x;
        let mut acc = step / 2;
        for px in &mut dst[start..start + visible_w] {
            *px = src_line[(acc >> 32) as usize];
            acc += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_largest_integer_scale_and_centers() {
        assert_eq!(
            layout((800, 600), (256, 192), None),
            Layout {
                x: 16,
                y: 12,
                width: 768,
                height: 576
            }
        );
        assert_eq!(
            layout((256, 192), (256, 192), None),
            Layout {
                x: 0,
                y: 0,
                width: 256,
                height: 192
            }
        );
        assert_eq!(
            layout((100, 100), (256, 192), None),
            Layout {
                x: 0,
                y: 0,
                width: 256,
                height: 192
            }
        );
    }

    #[test]
    fn blit_scales_pixels_and_letterboxes() {
        let src = [1, 2, 3, 4]; // 2×2
        let mut dst = [9; 6 * 4];
        blit(&src, 2, (2, 2), None, &mut dst, (6, 4));
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
        blit(&src, 2, (2, 2), None, &mut dst, (1, 1));
        assert_eq!(dst, [1]);
    }

    #[test]
    fn blit_uses_only_the_valid_part_of_each_row() {
        // Linhas de 3 pixels, só 2 válidos (como o H32 no buffer de H40).
        let src = [1, 2, 7, 3, 4, 7];
        let mut dst = [9; 4];
        blit(&src, 3, (2, 2), None, &mut dst, (2, 2));
        assert_eq!(dst, [1, 2, 3, 4]);
    }

    #[test]
    fn aspect_fills_the_window_regardless_of_resolution() {
        // Regressão: 512×240 do PS1 ficava em escala 1 numa janela 960×720.
        let full = Layout {
            x: 0,
            y: 0,
            width: 960,
            height: 720,
        };
        assert_eq!(layout((960, 720), (512, 240), Some((4, 3))), full);
        assert_eq!(layout((960, 720), (320, 240), Some((4, 3))), full);
        assert_eq!(layout((960, 720), (640, 480), Some((4, 3))), full);
    }

    #[test]
    fn aspect_letterboxes_wide_and_tall_windows() {
        assert_eq!(
            layout((1000, 600), (320, 240), Some((4, 3))),
            Layout {
                x: 100,
                y: 0,
                width: 800,
                height: 600
            }
        );
        assert_eq!(
            layout((400, 600), (320, 240), Some((4, 3))),
            Layout {
                x: 0,
                y: 150,
                width: 400,
                height: 300
            }
        );
    }

    #[test]
    fn aspect_stretches_non_square_pixels() {
        // 4×1 em uma janela 4:3 de 4×3: cada linha de origem vira 3.
        let src = [1, 2, 3, 4];
        let mut dst = [9; 12];
        blit(&src, 4, (4, 1), Some((4, 3)), &mut dst, (4, 3));
        assert_eq!(dst, [1, 2, 3, 4, 1, 2, 3, 4, 1, 2, 3, 4]);

        // 2×2 em 6×3: colunas triplicadas; o centro da linha do meio
        // (1,5 de 3) cai na 2ª linha de origem.
        let src = [1, 2, 3, 4];
        let mut dst = [9; 18];
        blit(&src, 2, (2, 2), Some((2, 1)), &mut dst, (6, 3));
        #[rustfmt::skip]
        let expected = [
            1, 1, 1, 2, 2, 2,
            3, 3, 3, 4, 4, 4,
            3, 3, 3, 4, 4, 4,
        ];
        assert_eq!(dst, expected);
    }
}
