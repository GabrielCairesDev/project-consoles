//! Saída de áudio pelo dispositivo padrão do sistema (cpal).

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::ring::SampleRing;

/// Folga de áudio entre o jogo e o dispositivo (~100 ms) antes de descartar.
const BUFFER_SECONDS: f32 = 0.1;

/// Stream ativo. Precisa continuar vivo enquanto o jogo roda.
pub struct AudioOut {
    _stream: cpal::Stream,
    ring: Arc<SampleRing>,
    pub sample_rate: u32,
    /// Um filtro por canal (L, R).
    dc: [DcBlocker; 2],
    scratch: Vec<f32>,
}

impl AudioOut {
    /// Abre o dispositivo padrão. `Err` descreve por que não há áudio; o
    /// chamador decide seguir mudo.
    pub fn open() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("nenhum dispositivo de saída de áudio")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let sample_rate = config.sample_rate;
        // Quadros estéreo inteiros: capacidade par, e cada envio também.
        let ring = Arc::new(SampleRing::new(
            (sample_rate as f32 * BUFFER_SECONDS) as usize * 2,
        ));

        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, config, ring.clone()),
            SampleFormat::I16 => build::<i16>(&device, config, ring.clone()),
            SampleFormat::U16 => build::<u16>(&device, config, ring.clone()),
            SampleFormat::I32 => build::<i32>(&device, config, ring.clone()),
            other => return Err(format!("formato de áudio não suportado: {other}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;

        Ok(Self {
            _stream: stream,
            ring,
            sample_rate,
            dc: Default::default(),
            scratch: Vec::new(),
        })
    }

    /// Envia áudio estéreo intercalado (L, R), já sem nível DC. O que não
    /// couber é descartado: atraso acumulado no áudio é pior que um estalo.
    pub fn push(&mut self, samples: &[f32]) {
        self.scratch.clear();
        for frame in samples.as_chunks::<2>().0 {
            self.scratch.push(self.dc[0].filter(frame[0]));
            self.scratch.push(self.dc[1].filter(frame[1]));
        }
        // Só quadros inteiros entram, para L e R não trocarem de lugar.
        let fits = self.ring.free() & !1;
        self.ring
            .push(&self.scratch[..self.scratch.len().min(fits)]);
    }
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    ring: Arc<SampleRing>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    let mut last = [0.0f32; 2];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for frame in data.chunks_mut(channels) {
                    // Sem amostra (atraso do jogo): repete a última, o que
                    // soa melhor que um corte para zero. O produtor só envia
                    // pares, então L e R chegam juntos.
                    if let (Some(l), Some(r)) = (ring.pop(), ring.pop()) {
                        last = [l, r];
                    }
                    match frame {
                        [mono] => *mono = T::from_sample((last[0] + last[1]) * 0.5),
                        [l, r, rest @ ..] => {
                            *l = T::from_sample(last[0]);
                            *r = T::from_sample(last[1]);
                            rest.fill(T::from_sample(0.0));
                        }
                        [] => {}
                    }
                }
            },
            |err| eprintln!("erro no áudio: {err}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// Filtro passa-altas de um polo: centraliza a saída do PSG em zero.
#[derive(Debug, Default)]
struct DcBlocker {
    prev_in: f32,
    prev_out: f32,
}

impl DcBlocker {
    const POLE: f32 = 0.995;

    fn filter(&mut self, x: f32) -> f32 {
        let y = x - self.prev_in + Self::POLE * self.prev_out;
        self.prev_in = x;
        self.prev_out = y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_blocker_removes_constant_offset() {
        let mut dc = DcBlocker::default();
        let last = (0..5_000).map(|_| dc.filter(0.5)).last().unwrap_or(1.0);
        assert!(last.abs() < 1e-3, "{last}");
    }

    #[test]
    fn dc_blocker_keeps_square_wave() {
        let mut dc = DcBlocker::default();
        let out: Vec<f32> = (0..2_000)
            .map(|i| dc.filter(if (i / 50) % 2 == 0 { 0.5 } else { 0.0 }))
            .collect();
        let peak = out[1_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.2, "{peak}");
    }
}
