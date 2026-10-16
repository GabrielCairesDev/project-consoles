//! Roda o BIOS (e, opcionalmente, um PS-EXE ou um disco `.cue`/`.bin`) e
//! salva o último quadro e a VRAM inteira em PNG.
//!
//! ```text
//! cargo run -p psx --release --example psx_frame -- <bios> [programa.exe | jogo.cue] <quadros> <saida>
//! ```
//!
//! Gera `<saida>.png` (o quadro mostrado) e `<saida>-vram.png` (1024×512).

use std::fs::File;
use std::io::BufWriter;
use std::process::ExitCode;

fn save_png(path: &str, width: usize, height: usize, rgb: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{path}: {e}"))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgb).map_err(|e| e.to_string())
}

fn run(args: &[String]) -> Result<(), String> {
    let [bios_path, rest @ ..] = args else {
        return Err("uso: psx_frame <bios> [programa.exe | jogo.cue] <quadros> <saida>".into());
    };
    let bios = std::fs::read(bios_path).map_err(|e| format!("{bios_path}: {e}"))?;
    let mut console = psx::Console::new(&bios).map_err(|e| e.to_string())?;
    let (exe, frames, out) = match rest {
        [exe, frames, out] => (Some(exe), frames, out),
        [frames, out] => (None, frames, out),
        _ => {
            return Err("uso: psx_frame <bios> [programa.exe | jogo.cue] <quadros> <saida>".into());
        }
    };
    if let Some(disc) = exe.filter(|p| p.ends_with(".cue") || p.ends_with(".bin")) {
        let disc =
            psx::Disc::open(std::path::Path::new(disc)).map_err(|e| format!("{disc}: {e}"))?;
        console.insert_disc(disc);
    } else if let Some(exe) = exe {
        let bytes = std::fs::read(exe).map_err(|e| format!("{exe}: {e}"))?;
        console.load_exe(&bytes).map_err(|e| e.to_string())?;
    }
    let frames: u32 = frames.parse().map_err(|_| "quadros inválido")?;
    for _ in 0..frames {
        console.run_frame();
    }

    let (frame, (width, height)) = console.framebuffer();
    let rgb: Vec<u8> = frame
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
        .collect();
    save_png(&format!("{out}.png"), width, height, &rgb)?;

    let expand = |c: u16| ((c & 0x1F) << 3 | (c & 0x1F) >> 2) as u8;
    let vram: Vec<u8> = console
        .vram()
        .iter()
        .flat_map(|&p| [expand(p), expand(p >> 5), expand(p >> 10)])
        .collect();
    save_png(&format!("{out}-vram.png"), 1024, 512, &vram)?;
    println!("{out}.png ({width}×{height}) e {out}-vram.png");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
