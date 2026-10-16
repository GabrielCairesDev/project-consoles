//! Roda uma ROM de Game Boy sem janela e grava um quadro em PPM.
//!
//! ```text
//! cargo run -p gameboy --release --example gb_frame -- jogo.gb 600 saida.ppm [botões]
//! ```
//!
//! `botões`: sequência opcional `quadro:tecla` (ex.: `300:s,500:a`), teclas
//! `u d l r a b s e` (s = Start, e = Select), cada uma segurada por 10
//! quadros.

use std::io::Write;

use gameboy::{Buttons, Console, SCREEN_HEIGHT, SCREEN_WIDTH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let rom_path = args
        .next()
        .ok_or("uso: gb_frame <rom> <quadros> <saida.ppm> [botões]")?;
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
    let start = std::time::Instant::now();
    for frame in 0..frames {
        let mut buttons = Buttons::default();
        for &(at, key) in &presses {
            if (at..at + 10).contains(&frame) {
                match key {
                    'u' => buttons.up = true,
                    'd' => buttons.down = true,
                    'l' => buttons.left = true,
                    'r' => buttons.right = true,
                    'a' => buttons.a = true,
                    'b' => buttons.b = true,
                    's' => buttons.start = true,
                    'e' => buttons.select = true,
                    _ => {}
                }
            }
        }
        console.set_input(buttons);
        console.run_frame();
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "{frames} quadros em {elapsed:.2}s ({:.0} quadros/s)",
        f64::from(frames) / elapsed
    );

    let mut file = std::io::BufWriter::new(std::fs::File::create(out_path)?);
    write!(file, "P6\n{SCREEN_WIDTH} {SCREEN_HEIGHT}\n255\n")?;
    for &px in console.framebuffer() {
        file.write_all(&[(px >> 16) as u8, (px >> 8) as u8, px as u8])?;
    }
    Ok(())
}
