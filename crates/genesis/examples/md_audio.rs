//! Roda uma ROM de Mega Drive sem janela e grava o áudio em WAV (16 bits, estéreo).
//!
//! ```text
//! cargo run -p genesis --release --example md_audio -- jogo.bin 1800 saida.wav [botões]
//! ```
//!
//! `botões`: como no `dump_frame` (`quadro:tecla`, teclas `u d l r a b c s`).

use std::io::Write;

use genesis::{Console, Pad};

const RATE: u32 = 44_100;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let rom_path = args
        .next()
        .ok_or("uso: md_audio <rom> <quadros> <saida.wav> [botões]")?;
    let frames: u32 = args.next().ok_or("informe o número de quadros")?.parse()?;
    let out_path = args.next().ok_or("informe o arquivo de saída")?;
    let presses: Vec<(u32, char)> = args
        .next()
        .unwrap_or_default()
        .split(',')
        .filter_map(|p| {
            let (frame, key) = p.split_once(':')?;
            Some((frame.parse().ok()?, key.chars().next()?))
        })
        .collect();

    let mut console = Console::new(std::fs::read(rom_path)?, RATE)?;
    let mut audio = Vec::new();
    for frame in 0..frames {
        let mut pad = Pad::default();
        for &(at, key) in &presses {
            if (at..at + 10).contains(&frame) {
                match key {
                    'u' => pad.up = true,
                    'd' => pad.down = true,
                    'l' => pad.left = true,
                    'r' => pad.right = true,
                    'a' => pad.a = true,
                    'b' => pad.b = true,
                    'c' => pad.c = true,
                    's' => pad.start = true,
                    _ => {}
                }
            }
        }
        console.set_input(pad);
        console.run_frame();
        console.drain_audio(&mut audio);
    }

    // Remove o nível DC (média) antes de gravar, como o player faz.
    let mean = audio.iter().sum::<f32>() / audio.len().max(1) as f32;
    let data_len = (audio.len() * 2) as u32;
    let mut file = std::io::BufWriter::new(std::fs::File::create(out_path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&RATE.to_le_bytes())?;
    file.write_all(&(RATE * 4).to_le_bytes())?;
    file.write_all(&4u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    let mut peak = 0.0f32;
    for s in &audio {
        let v = (s - mean).clamp(-1.0, 1.0);
        peak = peak.max(v.abs());
        file.write_all(&((v * 32767.0) as i16).to_le_bytes())?;
    }
    println!(
        "{:.1}s de áudio, pico {peak:.3}, nível DC {mean:.3}",
        audio.len() as f64 / 2.0 / f64::from(RATE)
    );
    Ok(())
}
