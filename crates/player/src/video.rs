//! Escala do quadro para a janela, centralizada com barras pretas.
//!
//! Sem proporção definida a escala é inteira (pixels nítidos e iguais);
//! com proporção, o quadro ocupa o maior retângulo dela que cabe. Antes de
//! ampliar, a etapa de cor aplica a [`Palette`](crate::image::Palette); a ampliação segue o
//! [`Scaler`] escolhido. [`ImageSettings::ORIGINAL`] é a imagem original,
//! pixel a pixel.

use crate::enhance::{self, Tap};
use crate::image::{ImageSettings, Scaler};

/// Tons que o núcleo do Game Boy desenha; a [`Palette`](crate::image::Palette) os substitui.
const CORE_SHADES: [u32; 4] = gameboy::SHADES;

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

/// Parte de `rect` que aparece na janela: `(largura, altura)`.
fn visible(rect: Layout, window: (usize, usize)) -> (usize, usize) {
    (
        rect.width.min(window.0.saturating_sub(rect.x)),
        rect.height.min(window.1.saturating_sub(rect.y)),
    )
}

/// Desenha `src` em `rect` por vizinho mais próximo (o fundo já está pronto).
fn nearest(
    src: &[u32],
    stride: usize,
    frame: (usize, usize),
    rect: Layout,
    dst: &mut [u32],
    window: (usize, usize),
) {
    if rect.width == 0 || rect.height == 0 || frame.0 == 0 || frame.1 == 0 {
        return;
    }
    let (visible_w, visible_h) = visible(rect, window);
    // Coluna de origem em ponto fixo 32.32, amostrando o centro de cada
    // pixel de destino: evita uma divisão por pixel e é exata nas escalas
    // inteiras (o erro acumulado fica abaixo de meio passo).
    let step = ((frame.0 as u64) << 32) / rect.width as u64;
    for row in 0..visible_h {
        let src_row = (2 * row + 1) * frame.1 / (2 * rect.height);
        let src_line = &src[src_row * stride..src_row * stride + frame.0];
        let start = (rect.y + row) * window.0 + rect.x;
        let mut acc = step / 2;
        for px in &mut dst[start..start + visible_w] {
            *px = src_line[(acc >> 32) as usize];
            acc += step;
        }
    }
}

/// Fator do Scale2x/3x/4x para uma escala inteira `k`: o que divide `k`
/// (pixels todos do mesmo tamanho), senão o maior.
fn pixel_art_factor(k: usize) -> usize {
    match k {
        0 | 1 => 1,
        2 | 3 => k,
        _ if k.is_multiple_of(4) => 4,
        _ if k.is_multiple_of(3) => 3,
        _ if k.is_multiple_of(2) => 2,
        _ => 4,
    }
}

/// Desenha os quadros com as opções de imagem escolhidas, reaproveitando
/// os buffers intermediários entre quadros.
#[derive(Debug)]
pub struct Renderer {
    scaler: Scaler,
    /// Tons que substituem os do núcleo, se a paleta não for a dele.
    shades: Option<[u32; 4]>,
    colored: Vec<u32>,
    scaled: Vec<u32>,
    pass: Vec<u32>,
    columns: Vec<Tap>,
    rows: Vec<Tap>,
}

impl Renderer {
    pub fn new(image: ImageSettings) -> Self {
        Self {
            scaler: image.scaler,
            shades: image.palette.shades(),
            colored: Vec::new(),
            scaled: Vec::new(),
            pass: Vec::new(),
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// Copia `src` (`frame.0 × frame.1`, linhas de `stride` pixels)
    /// ampliado para `dst` (`window.0` de largura), com a ampliação
    /// escolhida. O resto de `dst` fica preto.
    pub fn draw(
        &mut self,
        src: &[u32],
        stride: usize,
        frame: (usize, usize),
        aspect: Option<(usize, usize)>,
        dst: &mut [u32],
        window: (usize, usize),
    ) {
        dst.fill(0);
        let rect = layout(window, frame, aspect);
        if frame.0 == 0 || frame.1 == 0 || rect.width == 0 || rect.height == 0 {
            return;
        }
        // Etapa de cor: o quadro recolorido vira a origem da ampliação. O
        // buffer sai de `self` durante o desenho e volta depois, para ser
        // reaproveitado no próximo quadro.
        let mut colored = std::mem::take(&mut self.colored);
        let (src, stride) = match &self.shades {
            Some(shades) => {
                enhance::recolor(src, stride, frame, &CORE_SHADES, shades, &mut colored);
                (colored.as_slice(), frame.0)
            }
            None => (src, stride),
        };
        match self.scaler {
            Scaler::Nearest => nearest(src, stride, frame, rect, dst, window),
            Scaler::Epx => self.pixel_art(src, stride, frame, rect, dst, window),
            Scaler::Bilinear => {
                enhance::taps(frame.0, rect.width, &mut self.columns);
                enhance::taps(frame.1, rect.height, &mut self.rows);
                self.interpolate(src, stride, frame, rect, dst, window);
            }
            Scaler::SharpBilinear => {
                enhance::sharp_taps(frame.0, rect.width, &mut self.columns);
                enhance::sharp_taps(frame.1, rect.height, &mut self.rows);
                self.interpolate(src, stride, frame, rect, dst, window);
            }
        }
        self.colored = colored;
    }

    fn pixel_art(
        &mut self,
        src: &[u32],
        stride: usize,
        frame: (usize, usize),
        rect: Layout,
        dst: &mut [u32],
        window: (usize, usize),
    ) {
        let k = (rect.width / frame.0).min(rect.height / frame.1);
        let factor = pixel_art_factor(k);
        if factor == 1 {
            return nearest(src, stride, frame, rect, dst, window);
        }
        let size = (frame.0 * factor, frame.1 * factor);
        self.scaled.resize(size.0 * size.1, 0);
        match factor {
            2 => enhance::scale2x(src, stride, frame, &mut self.scaled),
            3 => enhance::scale3x(src, stride, frame, &mut self.scaled),
            _ => {
                self.pass.resize(4 * frame.0 * frame.1, 0);
                enhance::scale2x(src, stride, frame, &mut self.pass);
                let half = (2 * frame.0, 2 * frame.1);
                enhance::scale2x(&self.pass, half.0, half, &mut self.scaled);
            }
        }
        nearest(&self.scaled, size.0, size, rect, dst, window);
    }

    /// Bilinear separável: cada linha de origem é interpolada uma vez na
    /// horizontal, e cada linha da janela mistura duas delas.
    /// Interpolação separável (linhas, depois colunas) com as amostras já
    /// calculadas em `columns` e `rows`: bilinear comum ou sharp-bilinear.
    fn interpolate(
        &mut self,
        src: &[u32],
        stride: usize,
        frame: (usize, usize),
        rect: Layout,
        dst: &mut [u32],
        window: (usize, usize),
    ) {
        let (visible_w, visible_h) = visible(rect, window);
        let columns = &self.columns[..visible_w];
        self.pass.resize(frame.1 * visible_w, 0);
        for (y, out) in self.pass.chunks_exact_mut(visible_w).enumerate() {
            enhance::resample_row(&src[y * stride..][..frame.0], columns, out);
        }
        for (row, tap) in self.rows[..visible_h].iter().enumerate() {
            let start = (rect.y + row) * window.0 + rect.x;
            let out = &mut dst[start..start + visible_w];
            let top = &self.pass[tap.first * visible_w..][..visible_w];
            if tap.weight == 0 {
                out.copy_from_slice(top);
                continue;
            }
            let bottom = &self.pass[tap.second * visible_w..][..visible_w];
            for ((px, &a), &b) in out.iter_mut().zip(top).zip(bottom) {
                *px = enhance::lerp(a, b, tap.weight);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Imagem original, sem melhoria.
    fn blit(
        src: &[u32],
        stride: usize,
        frame: (usize, usize),
        aspect: Option<(usize, usize)>,
        dst: &mut [u32],
        window: (usize, usize),
    ) {
        Renderer::new(ImageSettings::ORIGINAL).draw(src, stride, frame, aspect, dst, window);
    }

    fn draw(
        scaler: Scaler,
        src: &[u32],
        stride: usize,
        frame: (usize, usize),
        aspect: Option<(usize, usize)>,
        window: (usize, usize),
    ) -> Vec<u32> {
        let mut dst = vec![9; window.0 * window.1];
        Renderer::new(ImageSettings {
            scaler,
            ..ImageSettings::ORIGINAL
        })
        .draw(src, stride, frame, aspect, &mut dst, window);
        dst
    }

    #[test]
    fn palette_recolors_before_scaling_and_original_keeps_the_core_colors() {
        use crate::image::Palette;
        // Os 4 tons do núcleo e uma cor que não é tom, 5×1 em janela 10×2.
        let src = [
            CORE_SHADES[0],
            CORE_SHADES[1],
            CORE_SHADES[2],
            CORE_SHADES[3],
            0x123456,
        ];
        let draw_with = |image: ImageSettings| {
            let mut dst = vec![9; 20];
            Renderer::new(image).draw(&src, 5, (5, 1), None, &mut dst, (10, 2));
            dst
        };
        let shades = Palette::Dmg.shades().expect("tons do DMG");
        let green = draw_with(ImageSettings {
            palette: Palette::Dmg,
            ..ImageSettings::ORIGINAL
        });
        let expected_row: Vec<u32> = [shades[0], shades[1], shades[2], shades[3], 0x123456]
            .iter()
            .flat_map(|&c| [c, c])
            .collect();
        assert_eq!(green[..10], expected_row[..]);
        assert_eq!(green[10..], expected_row[..]);

        let mut nearest_only = vec![9; 20];
        blit(&src, 5, (5, 1), None, &mut nearest_only, (10, 2));
        assert_eq!(draw_with(ImageSettings::ORIGINAL), nearest_only);
    }

    #[test]
    fn sharp_bilinear_is_nearest_at_integer_scale_and_blends_only_seams() {
        let src = [0x000000, 0x0000FF, 0x00FF00, 0xFF0000];
        let mut nearest_only = vec![9; 8 * 6];
        blit(&src, 2, (2, 2), None, &mut nearest_only, (8, 6));
        assert_eq!(
            draw(Scaler::SharpBilinear, &src, 2, (2, 2), None, (8, 6)),
            nearest_only
        );
        // 2×1 esticado para 5×1: só a coluna da divisa mistura.
        let src = [0x000000, 0x0000FF];
        let dst = draw(Scaler::SharpBilinear, &src, 2, (2, 1), Some((5, 1)), (5, 1));
        assert_eq!(dst, [0x00, 0x00, 0x7F, 0xFF, 0xFF]);
    }

    #[test]
    fn pixel_art_factor_prefers_even_pixels() {
        let factors: Vec<_> = (1..=10).map(pixel_art_factor).collect();
        assert_eq!(factors, [1, 2, 3, 4, 4, 3, 4, 4, 3, 2]);
    }

    #[test]
    fn pixel_art_uses_scale2x_and_letterboxes() {
        // Diagonal 2×2 (stride 3) em janela 6×4: escala 2, centralizada.
        let src = [1, 0, 7, 0, 1, 7];
        let dst = draw(Scaler::Epx, &src, 3, (2, 2), None, (6, 4));
        let mut scaled = [0; 16];
        enhance::scale2x(&src, 3, (2, 2), &mut scaled);
        for (row, line) in dst.as_chunks::<6>().0.iter().enumerate() {
            assert_eq!(line[0], 0);
            assert_eq!(line[5], 0);
            assert_eq!(&line[1..5], &scaled[row * 4..row * 4 + 4]);
        }
    }

    #[test]
    fn pixel_art_at_scale_4_applies_scale2x_twice() {
        let src = [1, 0, 0, 1];
        let dst = draw(Scaler::Epx, &src, 2, (2, 2), None, (8, 8));
        let mut twice = [0; 16];
        enhance::scale2x(&src, 2, (2, 2), &mut twice);
        let mut expected = [0; 64];
        enhance::scale2x(&twice, 4, (4, 4), &mut expected);
        assert_eq!(dst, expected);
    }

    #[test]
    fn enhancements_keep_flat_images_and_unscaled_frames() {
        let flat = [0x123456; 12];
        for scaler in [Scaler::Epx, Scaler::Bilinear] {
            let dst = draw(scaler, &flat, 4, (4, 3), None, (12, 9));
            assert_eq!(dst, vec![0x123456; 108], "{scaler:?}");
        }
        // Na escala 1 (ou cortado), ambos mostram o original.
        let src = [1, 2, 3, 4];
        for scaler in [Scaler::Epx, Scaler::Bilinear] {
            assert_eq!(draw(scaler, &src, 2, (2, 2), None, (2, 2)), src);
            assert_eq!(draw(scaler, &src, 2, (2, 2), None, (1, 1)), [1]);
        }
    }

    #[test]
    fn smooth_interpolates_inside_and_keeps_the_edges() {
        // 2×1 esticado para 4×3 (proporção 4:3).
        let src = [0x000000, 0x0000FF];
        let dst = draw(Scaler::Bilinear, &src, 2, (2, 1), Some((4, 3)), (4, 3));
        for line in dst.as_chunks::<4>().0 {
            assert_eq!(*line, [0x00, 0x3F, 0xBF, 0xFF]);
        }
    }

    #[test]
    fn smooth_mixes_rows_and_crops_to_the_window() {
        let src = [0x000000, 0x0000FF];
        let dst = draw(Scaler::Bilinear, &src, 1, (1, 2), Some((1, 4)), (1, 4));
        assert_eq!(dst, [0x00, 0x3F, 0xBF, 0xFF]);
    }

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

    /// Custo de cada técnica por quadro numa janela de 2560×1440, para a
    /// matriz de `docs/melhoria-de-imagem.md`. Rodar em release:
    /// `cargo test --release -p player bench_ -- --ignored --nocapture`.
    #[test]
    #[ignore = "benchmark: rodar em release, sob demanda"]
    fn bench_techniques_at_1440p() {
        use crate::image::Palette;
        use std::hint::black_box;
        use std::time::Instant;

        const WINDOW: (usize, usize) = (2560, 1440);
        const FRAMES: u32 = 120;
        // Quadro com detalhe (sem áreas lisas, que favorecem o EPX).
        let frame = |w: usize, h: usize, colors: &[u32]| -> Vec<u32> {
            (0..w * h)
                .map(|i| colors[(i * 7 + i / w * 3) % colors.len()])
                .collect()
        };
        let rgb = [0x102030, 0xF0E0D0, 0x40A040, 0xA04040, 0x2040A0];
        let cases = [
            (
                "SMS original",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings::ORIGINAL,
                None,
            ),
            (
                "SMS epx",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings {
                    scaler: Scaler::Epx,
                    ..ImageSettings::ORIGINAL
                },
                None,
            ),
            (
                "SMS sharp-bilinear 4:3",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings {
                    scaler: Scaler::SharpBilinear,
                    ..ImageSettings::ORIGINAL
                },
                Some((4, 3)),
            ),
            (
                "GB paleta + epx",
                (160, 144),
                frame(160, 144, &CORE_SHADES),
                ImageSettings {
                    palette: Palette::Dmg,
                    scaler: Scaler::Epx,
                },
                None,
            ),
            (
                "GB paleta + sharp 10:9",
                (160, 144),
                frame(160, 144, &CORE_SHADES),
                ImageSettings {
                    palette: Palette::Dmg,
                    scaler: Scaler::SharpBilinear,
                },
                Some((10, 9)),
            ),
            (
                "PS1 bilinear 4:3",
                (640, 480),
                frame(640, 480, &rgb),
                ImageSettings {
                    scaler: Scaler::Bilinear,
                    ..ImageSettings::ORIGINAL
                },
                Some((4, 3)),
            ),
            (
                "PS1 sharp-bilinear 4:3",
                (320, 240),
                frame(320, 240, &rgb),
                ImageSettings {
                    scaler: Scaler::SharpBilinear,
                    ..ImageSettings::ORIGINAL
                },
                Some((4, 3)),
            ),
        ];
        let mut dst = vec![0; WINDOW.0 * WINDOW.1];
        for (name, size, src, image, aspect) in cases {
            let mut renderer = Renderer::new(image);
            renderer.draw(&src, size.0, size, aspect, &mut dst, WINDOW);
            let start = Instant::now();
            for _ in 0..FRAMES {
                renderer.draw(black_box(&src), size.0, size, aspect, &mut dst, WINDOW);
                black_box(&dst);
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES);
            println!("{name:<26} {ms:>6.2} ms/quadro");
        }
    }
}
