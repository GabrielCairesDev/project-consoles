//! Escala do quadro para a janela, centralizada com barras pretas.
//!
//! Sem proporção definida a escala é inteira (pixels nítidos e iguais);
//! com proporção, o quadro ocupa o maior retângulo dela que cabe. Antes de
//! ampliar, a etapa de cor aplica a [`Palette`](crate::image::Palette); a ampliação segue o
//! [`Scaler`] escolhido, e a etapa de ajuste decide proporção
//! ([`AspectRatio`]) e encaixe ([`Fit`]). [`ImageSettings::original`] é a
//! imagem original, pixel a pixel.

use crate::enhance::{self, Tap};
use crate::image::{AspectRatio, Fit, FrameBlend, ImageSettings, Scaler, ScreenEffect, Signal};
use crate::mmpx;

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

/// Onde o quadro é desenhado. A proporção é `aspect` ou, sem ela, a do
/// próprio quadro (pixels quadrados). [`Fit::Integer`]: altura múltipla
/// inteira da do quadro, a maior que cabe (no mínimo 1, cortando se a
/// janela for menor); [`Fit::Fill`]: o maior retângulo na proporção.
pub fn layout(
    window: (usize, usize),
    frame: (usize, usize),
    aspect: Option<(usize, usize)>,
    fit: Fit,
) -> Layout {
    let (aw, ah) = aspect.unwrap_or(frame);
    let (width, height) = match fit {
        Fit::Integer => {
            let width_for = |k: usize| frame.1 * k * aw / ah;
            let mut k = (window.1 / frame.1).max(1);
            while k > 1 && width_for(k) > window.0 {
                k -= 1;
            }
            (width_for(k), frame.1 * k)
        }
        Fit::Fill => {
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

/// Pinta de preto só o que fica fora da parte visível de `rect` (as
/// barras). Todo filtro escreve cada pixel visível de `rect`, então pintar
/// o quadro inteiro antes seria escrever a maior parte da janela duas vezes.
fn clear_bars(dst: &mut [u32], window: (usize, usize), rect: Layout) {
    let (visible_w, visible_h) = visible(rect, window);
    let left = rect.x.min(window.0);
    let right = left + visible_w;
    let top = rect.y.min(window.1);
    let bottom = top + visible_h;
    dst[..top * window.0].fill(0);
    for row in dst[top * window.0..bottom * window.0].chunks_exact_mut(window.0) {
        row[..left].fill(0);
        row[right..].fill(0);
    }
    dst[bottom * window.0..].fill(0);
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
    aspect: AspectRatio,
    fit: Fit,
    screen: ScreenEffect,
    signal: Signal,
    blend: FrameBlend,
    /// Tons que substituem os do núcleo, se a paleta não for a dele.
    shades: Option<[u32; 4]>,
    colored: Vec<u32>,
    /// Quadro emulado anterior (sem cor aplicada), para o rastro do LCD.
    previous: Vec<u32>,
    previous_size: (usize, usize),
    previous_colored: Vec<u32>,
    filtered: Vec<u32>,
    /// Fatores da etapa de tela por linha e por coluna da área visível.
    screen_rows: Vec<u32>,
    screen_columns: Vec<[u32; 3]>,
    scaled: Vec<u32>,
    pass: Vec<u32>,
    columns: Vec<Tap>,
    rows: Vec<Tap>,
}

impl Renderer {
    pub fn new(image: ImageSettings) -> Self {
        Self {
            scaler: image.scaler,
            aspect: image.aspect,
            fit: image.fit,
            screen: image.screen,
            signal: image.signal,
            blend: image.blend,
            shades: image.palette.shades(),
            colored: Vec::new(),
            previous: Vec::new(),
            previous_size: (0, 0),
            previous_colored: Vec::new(),
            filtered: Vec::new(),
            screen_rows: Vec::new(),
            screen_columns: Vec::new(),
            scaled: Vec::new(),
            pass: Vec::new(),
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// Guarda o quadro que está na tela antes de o núcleo emular o próximo,
    /// para o rastro do LCD. Chamado a cada quadro emulado (não a cada
    /// desenho): redesenhar sem quadro novo não muda o resultado, e quadros
    /// emulados sem desenho entre eles não quebram a sequência.
    pub fn keep_previous(&mut self, src: &[u32], stride: usize, (w, h): (usize, usize)) {
        if self.blend == FrameBlend::Off {
            return;
        }
        self.previous.clear();
        for row in src.chunks(stride).take(h) {
            self.previous.extend_from_slice(&row[..w]);
        }
        self.previous_size = (w, h);
    }

    /// `tv_aspect` é a proporção da TV do console (se houver), usada quando
    /// o usuário escolhe [`AspectRatio::Tv`].
    ///
    /// Copia `src` (`frame.0 × frame.1`, linhas de `stride` pixels)
    /// ampliado para `dst` (`window.0` de largura), com a ampliação
    /// escolhida. O resto de `dst` fica preto.
    pub fn draw(
        &mut self,
        src: &[u32],
        stride: usize,
        frame: (usize, usize),
        tv_aspect: Option<(usize, usize)>,
        dst: &mut [u32],
        window: (usize, usize),
    ) {
        if frame.0 == 0 || frame.1 == 0 {
            dst.fill(0);
            return;
        }
        let aspect = match self.aspect {
            AspectRatio::Square => None,
            AspectRatio::Tv => tv_aspect,
        };
        let rect = layout(window, frame, aspect, self.fit);
        if rect.width == 0 || rect.height == 0 {
            dst.fill(0);
            return;
        }
        clear_bars(dst, window, rect);
        // Etapas antes da ampliação (cor, rastro, sinal), cada uma gerando a
        // origem da seguinte. Os buffers saem de `self` durante o desenho e
        // voltam depois, para serem reaproveitados no próximo quadro.
        let mut colored = std::mem::take(&mut self.colored);
        let mut filtered = std::mem::take(&mut self.filtered);
        // `colored` guarda o quadro quando cor ou rastro precisam mudá-lo;
        // senão, a origem continua sendo o quadro do núcleo.
        let mut in_colored = false;
        if let Some(shades) = &self.shades {
            enhance::recolor(src, stride, frame, &CORE_SHADES, shades, &mut colored);
            in_colored = true;
        }
        if self.blend == FrameBlend::On && self.previous_size == frame {
            if !in_colored {
                colored.clear();
                for row in src.chunks(stride).take(frame.1) {
                    colored.extend_from_slice(&row[..frame.0]);
                }
                in_colored = true;
            }
            let previous = match &self.shades {
                Some(shades) => {
                    enhance::recolor(
                        &self.previous,
                        frame.0,
                        frame,
                        &CORE_SHADES,
                        shades,
                        &mut self.previous_colored,
                    );
                    &self.previous_colored
                }
                None => &self.previous,
            };
            enhance::blend_frames(&mut colored, previous);
        }
        let (mut src, mut stride) = if in_colored {
            (colored.as_slice(), frame.0)
        } else {
            (src, stride)
        };
        if self.signal == Signal::Composite {
            enhance::composite(src, stride, frame, &mut filtered);
            (src, stride) = (filtered.as_slice(), frame.0);
        }
        match self.scaler {
            Scaler::Nearest => nearest(src, stride, frame, rect, dst, window),
            Scaler::Epx => self.pixel_art(src, stride, frame, rect, dst, window),
            Scaler::Mmpx => self.mmpx(src, stride, frame, rect, dst, window),
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
        self.filtered = filtered;
        self.apply_screen(frame, rect, dst, window);
    }

    /// Etapa de tela, depois da ampliação. Só age com espaço para o efeito:
    /// scanlines e CRT a partir de 2× na vertical, grade de LCD a partir de
    /// 3×; abaixo disso, a imagem fica como está.
    fn apply_screen(
        &mut self,
        frame: (usize, usize),
        rect: Layout,
        dst: &mut [u32],
        window: (usize, usize),
    ) {
        // Fração 1/256 de 1: escurecimento das scanlines, da máscara e das
        // divisões do LCD.
        const SCANLINE: u32 = 166;
        const MASK: u32 = 192;
        const GRID: u32 = 192;
        let (visible_w, visible_h) = visible(rect, window);
        let area = (rect.x, rect.y, visible_w, visible_h);
        // Linha (ou coluna) `i` de `out` amostra esta linha de origem, como o
        // vizinho mais próximo.
        let source = |i: usize, len: usize, out: usize| (2 * i + 1) * len / (2 * out);
        match self.screen {
            ScreenEffect::None => {}
            ScreenEffect::Scanlines | ScreenEffect::Crt => {
                if rect.height < 2 * frame.1 {
                    return;
                }
                self.screen_rows.clear();
                self.screen_rows.extend((0..visible_h).map(|row| {
                    // Metade de baixo de cada linha do console fica escura.
                    let within = (2 * row + 1) * frame.1 * 256 / (2 * rect.height) % 256;
                    if within > 128 { SCANLINE } else { 256 }
                }));
                let columns = if self.screen == ScreenEffect::Crt {
                    self.screen_columns.clear();
                    self.screen_columns.extend((0..visible_w).map(|column| {
                        match (rect.x + column) % 3 {
                            0 => [256, MASK, MASK],
                            1 => [MASK, 256, MASK],
                            _ => [MASK, MASK, 256],
                        }
                    }));
                    Some(self.screen_columns.as_slice())
                } else {
                    None
                };
                enhance::shade(dst, window.0, area, &self.screen_rows, columns);
            }
            ScreenEffect::LcdGrid => {
                if rect.width < 3 * frame.0 || rect.height < 3 * frame.1 {
                    return;
                }
                // A última linha e a última coluna de cada pixel do console
                // viram a divisão entre os pixels da tela.
                let gap = |i: usize, len: usize, out: usize| {
                    i + 1 == out || source(i, len, out) != source(i + 1, len, out)
                };
                self.screen_rows.clear();
                self.screen_rows.extend((0..visible_h).map(|row| {
                    if gap(row, frame.1, rect.height) {
                        GRID
                    } else {
                        256
                    }
                }));
                self.screen_columns.clear();
                self.screen_columns.extend((0..visible_w).map(|column| {
                    [if gap(column, frame.0, rect.width) {
                        GRID
                    } else {
                        256
                    }; 3]
                }));
                enhance::shade(
                    dst,
                    window.0,
                    area,
                    &self.screen_rows,
                    Some(&self.screen_columns),
                );
            }
        }
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

    /// MMPX só amplia 2×: escala 2 ou 3 usa uma passada; 4 ou mais, duas
    /// (4×). O que sobra até a janela é por vizinho mais próximo, como no
    /// EPX quando o fator não divide a escala.
    fn mmpx(
        &mut self,
        src: &[u32],
        stride: usize,
        frame: (usize, usize),
        rect: Layout,
        dst: &mut [u32],
        window: (usize, usize),
    ) {
        let k = (rect.width / frame.0).min(rect.height / frame.1);
        if k < 2 {
            return nearest(src, stride, frame, rect, dst, window);
        }
        let double = (2 * frame.0, 2 * frame.1);
        if k < 4 {
            self.scaled.resize(double.0 * double.1, 0);
            mmpx::mmpx2x(src, stride, frame, &mut self.scaled);
            return nearest(&self.scaled, double.0, double, rect, dst, window);
        }
        let quad = (4 * frame.0, 4 * frame.1);
        self.pass.resize(double.0 * double.1, 0);
        self.scaled.resize(quad.0 * quad.1, 0);
        mmpx::mmpx2x(src, stride, frame, &mut self.pass);
        mmpx::mmpx2x(&self.pass, double.0, double, &mut self.scaled);
        nearest(&self.scaled, quad.0, quad, rect, dst, window);
    }

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

    /// Configuração de teste: com `aspect`, preenche nessa proporção; sem,
    /// pixels quadrados em escala inteira (o comportamento de cada console
    /// antes da etapa de ajuste).
    fn legacy(scaler: Scaler, aspect: Option<(usize, usize)>) -> ImageSettings {
        ImageSettings {
            scaler,
            aspect: AspectRatio::Tv,
            fit: if aspect.is_some() {
                Fit::Fill
            } else {
                Fit::Integer
            },
            ..ImageSettings::original(crate::controls::System::MasterSystem)
        }
    }

    /// Imagem original, sem melhoria.
    fn blit(
        src: &[u32],
        stride: usize,
        frame: (usize, usize),
        aspect: Option<(usize, usize)>,
        dst: &mut [u32],
        window: (usize, usize),
    ) {
        Renderer::new(legacy(Scaler::Nearest, aspect))
            .draw(src, stride, frame, aspect, dst, window);
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
        Renderer::new(legacy(scaler, aspect)).draw(src, stride, frame, aspect, &mut dst, window);
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
            ..legacy(Scaler::Nearest, None)
        });
        let expected_row: Vec<u32> = [shades[0], shades[1], shades[2], shades[3], 0x123456]
            .iter()
            .flat_map(|&c| [c, c])
            .collect();
        assert_eq!(green[..10], expected_row[..]);
        assert_eq!(green[10..], expected_row[..]);

        let mut nearest_only = vec![9; 20];
        blit(&src, 5, (5, 1), None, &mut nearest_only, (10, 2));
        assert_eq!(draw_with(legacy(Scaler::Nearest, None)), nearest_only);
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
    fn tv_aspect_with_integer_fit_keeps_whole_lines() {
        // SMS 256×192 em 4:3 numa janela 1920×1080: altura 5 × 192 = 960,
        // largura 960 × 4/3 = 1280.
        let rect = layout((1920, 1080), (256, 192), Some((4, 3)), Fit::Integer);
        assert_eq!((rect.width, rect.height), (1280, 960));
        assert_eq!((rect.x, rect.y), (320, 60));
        // Janela estreita: a largura manda e a escala desce até caber.
        let narrow = layout((700, 1080), (256, 192), Some((4, 3)), Fit::Integer);
        assert_eq!((narrow.width, narrow.height), (512, 384));
        // Pixels quadrados preenchendo: maior retângulo na proporção do quadro.
        let fill = layout((1000, 1000), (160, 144), None, Fit::Fill);
        assert_eq!((fill.width, fill.height), (1000, 900));
    }

    #[test]
    fn renderer_applies_the_chosen_aspect_and_fit() {
        let src = [0x112233; 4];
        let render = |aspect, fit| {
            let mut dst = vec![9; 12 * 6];
            Renderer::new(ImageSettings {
                aspect,
                fit,
                ..ImageSettings::original(crate::controls::System::MasterSystem)
            })
            .draw(&src, 2, (2, 2), Some((2, 1)), &mut dst, (12, 6));
            dst.iter().filter(|&&px| px == 0x112233).count()
        };
        // Quadrados inteiros: 2×2 → 6×6 (escala 3). TV 2:1 inteira: 12×6.
        assert_eq!(render(AspectRatio::Square, Fit::Integer), 36);
        assert_eq!(render(AspectRatio::Tv, Fit::Integer), 72);
        assert_eq!(render(AspectRatio::Square, Fit::Fill), 36);
        assert_eq!(render(AspectRatio::Tv, Fit::Fill), 72);
        // Sem proporção de TV no console, "TV" vale pixels quadrados.
        let mut dst = vec![9; 12 * 6];
        Renderer::new(ImageSettings {
            aspect: AspectRatio::Tv,
            ..ImageSettings::original(crate::controls::System::MasterSystem)
        })
        .draw(&src, 2, (2, 2), None, &mut dst, (12, 6));
        assert_eq!(dst.iter().filter(|&&px| px == 0x112233).count(), 36);
    }

    /// Renderer com uma configuração de 2D em escala inteira, só com as
    /// mudanças de `change`.
    fn with(change: impl FnOnce(&mut ImageSettings)) -> Renderer {
        let mut image = ImageSettings::original(crate::controls::System::MasterSystem);
        change(&mut image);
        Renderer::new(image)
    }

    #[test]
    fn scanlines_darken_the_lower_half_of_each_line_from_2x() {
        let src = [0xFFFFFF; 4];
        let mut dst = vec![0; 4 * 8];
        with(|i| i.screen = ScreenEffect::Scanlines).draw(&src, 2, (2, 2), None, &mut dst, (4, 8));
        // 2×2 em 4×8: escala 2 (4×4 centralizado na vertical, linhas 2..6).
        let rows: Vec<u32> = dst.chunks(4).map(|row| row[0]).collect();
        assert_eq!(rows, [0, 0, 0xFFFFFF, 0xA5A5A5, 0xFFFFFF, 0xA5A5A5, 0, 0]);
        // Em 1× não há espaço: imagem intacta.
        let mut small = vec![0; 4];
        with(|i| i.screen = ScreenEffect::Scanlines).draw(
            &src,
            2,
            (2, 2),
            None,
            &mut small,
            (2, 2),
        );
        assert_eq!(small, src);
    }

    #[test]
    fn crt_adds_an_rgb_aperture_mask() {
        let src = [0xFFFFFF; 3];
        let mut dst = vec![0; 6 * 2];
        with(|i| i.screen = ScreenEffect::Crt).draw(&src, 3, (3, 1), None, &mut dst, (6, 2));
        assert_eq!(dst[..3], [0xFFBFBF, 0xBFFFBF, 0xBFBFFF]);
        // Linha de scanline: máscara × scanline (166 no canal da coluna,
        // 192 × 166 / 256 = 124 nos outros).
        assert_eq!(dst[6..9], [0xA57B7B, 0x7BA57B, 0x7B7BA5]);
    }

    #[test]
    fn lcd_grid_marks_the_edges_of_each_pixel_from_3x() {
        let src = [0xFFFFFF; 4];
        let mut dst = vec![0; 6 * 6];
        let mut renderer = with(|i| i.screen = ScreenEffect::LcdGrid);
        renderer.draw(&src, 2, (2, 2), None, &mut dst, (6, 6));
        let grid = 0xBFBFBF;
        let corner = 0x8F8F8F;
        assert_eq!(
            dst[..6],
            [0xFFFFFF, 0xFFFFFF, grid, 0xFFFFFF, 0xFFFFFF, grid]
        );
        assert_eq!(dst[12..18], [grid, grid, corner, grid, grid, corner]);
        // Em 2× a grade comeria metade da imagem: não é aplicada.
        let mut dst = vec![0; 16];
        renderer.draw(&src, 2, (2, 2), None, &mut dst, (4, 4));
        assert_eq!(dst, [0xFFFFFF; 16]);
    }

    #[test]
    fn composite_fuses_dithering_before_scaling() {
        let src = [0xFFFFFF, 0, 0xFFFFFF, 0];
        let mut dst = vec![0; 4];
        with(|i| i.signal = Signal::Composite).draw(&src, 4, (4, 1), None, &mut dst, (4, 1));
        assert_eq!(dst[1..3], [0x7F7F7F, 0x7F7F7F]);
    }

    #[test]
    fn frame_blend_mixes_with_the_frame_kept_before_emulating() {
        let mut renderer = with(|i| i.blend = FrameBlend::On);
        let mut dst = vec![0; 1];
        // Sem quadro anterior guardado: o atual, puro.
        renderer.draw(&[0xFF0000], 1, (1, 1), None, &mut dst, (1, 1));
        assert_eq!(dst, [0xFF0000]);
        renderer.keep_previous(&[0xFF0000], 1, (1, 1));
        renderer.draw(&[0x0000FF], 1, (1, 1), None, &mut dst, (1, 1));
        assert_eq!(dst, [0x7F007F]);
        // Redesenhar sem quadro novo dá o mesmo resultado.
        renderer.draw(&[0x0000FF], 1, (1, 1), None, &mut dst, (1, 1));
        assert_eq!(dst, [0x7F007F]);
        // Desligado, nada é guardado nem misturado.
        let mut off = with(|_| {});
        off.keep_previous(&[0xFF0000], 1, (1, 1));
        off.draw(&[0x0000FF], 1, (1, 1), None, &mut dst, (1, 1));
        assert_eq!(dst, [0x0000FF]);
    }

    #[test]
    fn frame_blend_recolors_both_frames_with_the_palette() {
        use crate::image::Palette;
        let mut renderer = with(|i| {
            i.palette = Palette::Dmg;
            i.blend = FrameBlend::On;
        });
        let shades = Palette::Dmg.shades().expect("tons");
        renderer.keep_previous(&[CORE_SHADES[0]], 1, (1, 1));
        let mut dst = vec![0; 1];
        renderer.draw(&[CORE_SHADES[3]], 1, (1, 1), None, &mut dst, (1, 1));
        assert_eq!(dst, [enhance::lerp(shades[3], shades[0], 128)]);
    }

    #[test]
    fn every_pixel_is_written_whatever_was_in_the_window() {
        // Pintar só as barras exige que os filtros escrevam todo o retângulo:
        // o resultado sobre lixo tem que ser igual ao resultado sobre preto.
        let src: Vec<u32> = (0..7 * 5)
            .map(|i| (i * 0x0102_0304) & 0x00FF_FFFF)
            .collect();
        let windows = [(7, 5), (20, 11), (31, 40), (64, 23), (5, 3), (3, 2)];
        let aspects = [None, Some((4, 3)), Some((1, 3))];
        let choices = Scaler::ALL.into_iter().flat_map(|scaler| {
            AspectRatio::ALL.into_iter().flat_map(move |aspect| {
                Fit::ALL.into_iter().flat_map(move |fit| {
                    ScreenEffect::ALL
                        .into_iter()
                        .map(move |screen| ImageSettings {
                            scaler,
                            aspect,
                            fit,
                            screen,
                            signal: if screen == ScreenEffect::Crt {
                                Signal::Composite
                            } else {
                                Signal::Rgb
                            },
                            ..ImageSettings::original(crate::controls::System::MasterSystem)
                        })
                })
            })
        });
        for image in choices {
            let mut renderer = Renderer::new(image);
            for window in windows {
                for aspect in aspects {
                    let mut black = vec![0; window.0 * window.1];
                    renderer.draw(&src, 7, (7, 5), aspect, &mut black, window);
                    let mut garbage = vec![0xAB_CDEF; window.0 * window.1];
                    renderer.draw(&src, 7, (7, 5), aspect, &mut garbage, window);
                    assert_eq!(garbage, black, "{image:?} {window:?} {aspect:?}");
                }
            }
        }
    }

    #[test]
    fn mmpx_runs_once_or_twice_by_scale() {
        // Diagonal com detalhe; 2× e 3× usam uma passada, 4× usa duas.
        let src = [1, 0, 0, 0, 1, 0, 0, 0, 1];
        let mut once = vec![0; 36];
        mmpx::mmpx2x(&src, 3, (3, 3), &mut once);
        assert_eq!(draw(Scaler::Mmpx, &src, 3, (3, 3), None, (6, 6)), once);
        let mut twice = vec![0; 144];
        mmpx::mmpx2x(&once, 6, (6, 6), &mut twice);
        assert_eq!(draw(Scaler::Mmpx, &src, 3, (3, 3), None, (12, 12)), twice);
        // Escala 1: o original.
        assert_eq!(draw(Scaler::Mmpx, &src, 3, (3, 3), None, (3, 3)), src);
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
        for scaler in [Scaler::Epx, Scaler::Mmpx, Scaler::Bilinear] {
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
            layout((800, 600), (256, 192), None, Fit::Integer),
            Layout {
                x: 16,
                y: 12,
                width: 768,
                height: 576
            }
        );
        assert_eq!(
            layout((256, 192), (256, 192), None, Fit::Integer),
            Layout {
                x: 0,
                y: 0,
                width: 256,
                height: 192
            }
        );
        assert_eq!(
            layout((100, 100), (256, 192), None, Fit::Integer),
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
        assert_eq!(
            layout((960, 720), (512, 240), Some((4, 3)), Fit::Fill),
            full
        );
        assert_eq!(
            layout((960, 720), (320, 240), Some((4, 3)), Fit::Fill),
            full
        );
        assert_eq!(
            layout((960, 720), (640, 480), Some((4, 3)), Fit::Fill),
            full
        );
    }

    #[test]
    fn aspect_letterboxes_wide_and_tall_windows() {
        assert_eq!(
            layout((1000, 600), (320, 240), Some((4, 3)), Fit::Fill),
            Layout {
                x: 100,
                y: 0,
                width: 800,
                height: 600
            }
        );
        assert_eq!(
            layout((400, 600), (320, 240), Some((4, 3)), Fit::Fill),
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
                ImageSettings::original(crate::controls::System::MasterSystem),
                None,
            ),
            (
                "SMS mmpx",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings {
                    scaler: Scaler::Mmpx,
                    ..ImageSettings::original(crate::controls::System::MasterSystem)
                },
                None,
            ),
            (
                "SMS epx",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings {
                    scaler: Scaler::Epx,
                    ..ImageSettings::original(crate::controls::System::MasterSystem)
                },
                None,
            ),
            (
                "SMS sharp-bilinear 4:3",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings {
                    scaler: Scaler::SharpBilinear,
                    ..ImageSettings::original(crate::controls::System::MasterSystem)
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
                    ..ImageSettings::original(crate::controls::System::GameBoy)
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
                    ..ImageSettings::original(crate::controls::System::GameBoy)
                },
                Some((10, 9)),
            ),
            (
                "PS1 bilinear 4:3",
                (640, 480),
                frame(640, 480, &rgb),
                ImageSettings {
                    scaler: Scaler::Bilinear,
                    ..ImageSettings::original(crate::controls::System::MasterSystem)
                },
                Some((4, 3)),
            ),
            (
                "PS1 sharp-bilinear 4:3",
                (320, 240),
                frame(320, 240, &rgb),
                ImageSettings {
                    scaler: Scaler::SharpBilinear,
                    ..ImageSettings::original(crate::controls::System::MasterSystem)
                },
                Some((4, 3)),
            ),
            (
                "SMS epx + scanlines",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings {
                    scaler: Scaler::Epx,
                    screen: ScreenEffect::Scanlines,
                    ..ImageSettings::original(crate::controls::System::MasterSystem)
                },
                None,
            ),
            (
                "SMS composto+epx+CRT",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings {
                    scaler: Scaler::Epx,
                    screen: ScreenEffect::Crt,
                    signal: Signal::Composite,
                    ..ImageSettings::original(crate::controls::System::MasterSystem)
                },
                None,
            ),
            (
                "SMS pior: composto+mmpx+CRT",
                (256, 192),
                frame(256, 192, &rgb),
                ImageSettings {
                    scaler: Scaler::Mmpx,
                    screen: ScreenEffect::Crt,
                    signal: Signal::Composite,
                    ..ImageSettings::original(crate::controls::System::MasterSystem)
                },
                None,
            ),
            (
                "GB tudo (paleta+rastro+grade)",
                (160, 144),
                frame(160, 144, &CORE_SHADES),
                ImageSettings {
                    palette: Palette::Dmg,
                    scaler: Scaler::Epx,
                    screen: ScreenEffect::LcdGrid,
                    blend: FrameBlend::On,
                    ..ImageSettings::original(crate::controls::System::GameBoy)
                },
                None,
            ),
            (
                "PS1 composto+bilinear+CRT",
                (640, 480),
                frame(640, 480, &rgb),
                ImageSettings {
                    scaler: Scaler::Bilinear,
                    screen: ScreenEffect::Crt,
                    signal: Signal::Composite,
                    ..ImageSettings::original(crate::controls::System::PlayStation)
                },
                Some((4, 3)),
            ),
        ];
        let mut dst = vec![0; WINDOW.0 * WINDOW.1];
        for (name, size, src, image, aspect) in cases {
            let presentation = legacy(image.scaler, aspect);
            let mut renderer = Renderer::new(ImageSettings {
                aspect: presentation.aspect,
                fit: presentation.fit,
                ..image
            });
            renderer.draw(&src, size.0, size, aspect, &mut dst, WINDOW);
            let start = Instant::now();
            for _ in 0..FRAMES {
                // Como no jogo: o quadro anterior é guardado a cada quadro.
                renderer.keep_previous(&src, size.0, size);
                renderer.draw(black_box(&src), size.0, size, aspect, &mut dst, WINDOW);
                black_box(&dst);
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES);
            println!("{name:<30} {ms:>6.2} ms/quadro");
        }
    }
}
