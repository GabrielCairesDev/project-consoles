//! Roda o BIOS (e, opcionalmente, um PS-EXE no lugar do shell) e mostra o
//! texto que ele escreve.
//!
//! ```text
//! cargo run -p psx --release --example psx_tty -- <bios> [programa.exe] [quadros]
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(bios_path) = args.first() else {
        eprintln!("uso: psx_tty <bios> [programa.exe] [quadros]");
        return ExitCode::FAILURE;
    };
    let bios = match std::fs::read(bios_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{bios_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut console = match psx::Console::new(&bios) {
        Ok(console) => console,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut frames = 600;
    for arg in &args[1..] {
        if let Ok(n) = arg.parse() {
            frames = n;
        } else {
            match std::fs::read(arg)
                .map_err(|e| e.to_string())
                .and_then(|exe| console.load_exe(&exe).map_err(|e| e.to_string()))
            {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("{arg}: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    let start = std::time::Instant::now();
    for _ in 0..frames {
        console.run_frame();
    }
    let elapsed = start.elapsed();
    print!("{}", String::from_utf8_lossy(console.tty_output()));
    eprintln!(
        "\n{frames} quadros em {:.2?} ({:.1} quadros/s), PC = {:08X}",
        elapsed,
        f64::from(frames) / elapsed.as_secs_f64(),
        console.pc()
    );
    ExitCode::SUCCESS
}
