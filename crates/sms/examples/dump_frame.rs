//! Roda uma ROM sem janela e grava um quadro em PPM, para depuração.
//!
//! ```text
//! cargo run -p sms --release --example dump_frame -- jogo.sms 600 saida.ppm [botões]
//! ```
//!
//! `botões` é uma sequência opcional de `quadro:tecla` (ex.: `200:1,400:p`),
//! com teclas `u d l r 1 2 p` (p = Pause), cada uma pressionada por 10 quadros.

use std::io::Write;

use sms::{Console, Joypad, SCREEN_HEIGHT, SCREEN_WIDTH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let rom_path = args
        .next()
        .ok_or("uso: dump_frame <rom> <quadros> <saida.ppm> [botões]")?;
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

    let mut console = Console::new(std::fs::read(rom_path)?, 44_100)?;
    let mut audio = Vec::new();
    for frame in 0..frames {
        let mut pad = Joypad::default();
        for &(at, key) in &presses {
            if (at..at + 10).contains(&frame) {
                match key {
                    'u' => pad.up = true,
                    'd' => pad.down = true,
                    'l' => pad.left = true,
                    'r' => pad.right = true,
                    '1' => pad.button1 = true,
                    '2' => pad.button2 = true,
                    'p' if frame == at => console.press_pause(),
                    _ => {}
                }
            }
        }
        console.set_input(pad);
        console.run_frame();
        console.drain_audio(&mut audio);
    }

    let mut file = std::io::BufWriter::new(std::fs::File::create(out_path)?);
    write!(file, "P6\n{SCREEN_WIDTH} {SCREEN_HEIGHT}\n255\n")?;
    for &px in console.framebuffer() {
        file.write_all(&[(px >> 16) as u8, (px >> 8) as u8, px as u8])?;
    }
    let peak = audio.iter().copied().fold(0.0f32, f32::max);
    println!("{} amostras de áudio, pico {peak:.3}", audio.len());
    Ok(())
}
