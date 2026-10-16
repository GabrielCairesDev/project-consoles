//! Conversão de taxa do áudio estéreo por interpolação linear, para
//! núcleos que geram numa taxa fixa (o PlayStation, a 44,1 kHz).

/// Reamostrador de áudio estéreo intercalado (L, R). Guarda a posição
/// fracionária e o último quadro entre chamadas, então o som não estala
/// entre um quadro de vídeo e o seguinte.
#[derive(Debug, Clone)]
pub struct Resampler {
    /// Quadros de entrada por quadro de saída.
    step: f64,
    /// Posição da próxima saída, em quadros de entrada, contada a partir de
    /// `previous` (0 = `previous`, 1 = o primeiro quadro novo).
    position: f64,
    previous: [f32; 2],
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            step: f64::from(input_rate) / f64::from(output_rate),
            position: 0.0,
            previous: [0.0; 2],
        }
    }

    /// Converte `input` e acrescenta o resultado a `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let frames = input.as_chunks::<2>().0;
        let Some(&last) = frames.last() else {
            return;
        };
        let frame = |i: usize| if i == 0 { self.previous } else { frames[i - 1] };
        while self.position <= frames.len() as f64 {
            let index = self.position.floor();
            let fraction = (self.position - index) as f32;
            let index = index as usize;
            let a = frame(index);
            let b = if index < frames.len() {
                frames[index]
            } else {
                a
            };
            out.extend([0, 1].map(|c| a[c] + (b[c] - a[c]) * fraction));
            self.position += self.step;
        }
        self.position -= frames.len() as f64;
        self.previous = last;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(resampler: &mut Resampler, frames: usize, value: impl Fn(usize) -> f32) -> Vec<f32> {
        let input: Vec<f32> = (0..frames).flat_map(|i| [value(i), -value(i)]).collect();
        let mut out = Vec::new();
        resampler.process(&input, &mut out);
        out
    }

    #[test]
    fn output_count_follows_the_rate_ratio_across_calls() {
        let mut resampler = Resampler::new(44_100, 48_000);
        let mut total = 0;
        // Um segundo em pedaços de tamanho de um quadro de vídeo.
        for _ in 0..60 {
            total += run(&mut resampler, 735, |_| 0.5).len() / 2;
        }
        assert!((47_999..=48_001).contains(&total), "{total}");
    }

    #[test]
    fn same_rate_passes_samples_through_with_one_frame_delay() {
        let mut resampler = Resampler::new(44_100, 44_100);
        let out = run(&mut resampler, 4, |i| i as f32 + 1.0);
        assert_eq!(out, [0.0, 0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0]);
        let out = run(&mut resampler, 1, |_| 5.0);
        assert_eq!(out, [5.0, -5.0], "continua de onde parou");
    }

    #[test]
    fn interpolates_between_frames_and_keeps_channels_apart() {
        let mut resampler = Resampler::new(1, 2);
        let out = run(&mut resampler, 2, |i| i as f32 * 2.0 + 2.0);
        assert_eq!(out, [0.0, 0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0]);
        let mut empty = Vec::new();
        resampler.process(&[], &mut empty);
        assert!(empty.is_empty());
    }
}
