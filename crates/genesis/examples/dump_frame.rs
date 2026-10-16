//! Roda uma ROM de Mega Drive sem janela e grava um quadro em PPM.
//!
//! ```text
//! cargo run -p genesis --release --example dump_frame -- jogo.bin 600 saida.ppm [botões]
//! ```
//!
//! `botões`: sequência opcional `quadro:tecla` (ex.: `300:s,500:a`), teclas
//! `u d l r a b c s` (s = Start), cada uma segurada por 10 quadros.

use std::io::Write;

use genesis::{Console, Pad};

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
    let start = std::time::Instant::now();
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
        if std::env::var("TRACE").is_ok() && frame % 30 == 0 {
            let r = console.cpu_registers();
            eprintln!(
                "quadro {frame}: pc={:06X} sr={:04X} halted={} ir={:04X} vdp={:02X?}",
                r.pc,
                r.sr,
                console.cpu_halted(),
                r.prefetch[0],
                &console.vdp_registers()[..13]
            );
        }
        console.drain_audio(&mut audio);
    }
    let elapsed = start.elapsed();

    let (width, height) = console.frame_size();
    let stride = genesis::SCREEN_MAX_WIDTH;
    let mut file = std::io::BufWriter::new(std::fs::File::create(out_path)?);
    write!(file, "P6\n{width} {height}\n255\n")?;
    for row in console.framebuffer().chunks(stride).take(height) {
        for &px in &row[..width] {
            file.write_all(&[(px >> 16) as u8, (px >> 8) as u8, px as u8])?;
        }
    }
    println!(
        "{frames} quadros em {:.2}s ({:.0} fps), {width}x{height}",
        elapsed.as_secs_f64(),
        f64::from(frames) / elapsed.as_secs_f64()
    );
    Ok(())
}
