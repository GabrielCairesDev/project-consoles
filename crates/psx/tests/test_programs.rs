//! Programas de teste públicos (ps1-tests, de Jakub Czekański), baixados
//! fora do repositório, e o BIOS do usuário.
//!
//! ```text
//! PSX_TESTS_DIR=~/psx-tests cargo test -p psx --release --test test_programs -- --ignored --nocapture
//! ```
//!
//! `PSX_TESTS_DIR` deve conter `bios/SCPH1001.BIN` e o `tests.zip` do
//! ps1-tests extraído em `ps1-tests/`.
//!
//! Os programas escrevem o resultado pelo putchar do BIOS: uma linha
//! "pass - …" ou "fail - …" por caso.

use std::path::{Path, PathBuf};

/// Programas com asserções que já devem passar inteiros.
const PROGRAMS: &[&str] = &[
    "cpu/cop/cop",
    "cpu/code-in-io/code-in-io",
    "dma/otc-test/otc-test",
];

const FRAMES: u32 = 600;

/// Programa de GPU comparado com a VRAM de referência (`vram.png`,
/// capturada no hardware).
struct GpuProgram {
    name: &'static str,
    /// Largura e altura comparadas a partir de (0, 0): fora delas a
    /// referência tem o que já estava na VRAM do console.
    area: (usize, usize),
    /// Pixels diferentes aceitos, com o motivo.
    allowed: usize,
    reason: &'static str,
}

const FULL: (usize, usize) = (1024, 512);

const fn exact(name: &'static str) -> GpuProgram {
    GpuProgram {
        name,
        area: FULL,
        allowed: 0,
        reason: "",
    }
}

const GPU_PROGRAMS: &[GpuProgram] = &[
    exact("gpu/triangle/triangle"),
    exact("gpu/quad/quad"),
    exact("gpu/rectangles/rectangles"),
    GpuProgram {
        name: "gpu/lines/lines",
        area: FULL,
        allowed: 64,
        reason: "a cor do último vértice das 2 polilinhas gouraud nunca é \
                 definida pelo programa (setRGB2 repetido): é lixo da pilha",
    },
    GpuProgram {
        name: "gpu/transparency/transparency",
        area: (320, 240),
        allowed: 0,
        reason: "",
    },
    exact("gpu/texture-flip/texture-flip"),
    exact("gpu/texture-overflow/texture-overflow"),
    exact("gpu/uv-interpolation/uv-interpolation"),
    exact("gpu/clipping/clipping"),
    exact("gpu/vram-to-vram-overlap/vram-to-vram-overlap"),
    exact("gpu/clut-cache/clut-cache"),
];

/// VRAM de referência em 15 bits (sem o bit de máscara).
fn reference_vram(path: &Path) -> Vec<u16> {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    ));
    // Paleta e tons de cinza viram RGB/cinza de 8 bits.
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("PNG válido");
    let mut rgb = vec![0; reader.output_buffer_size().expect("tamanho")];
    let info = reader.next_frame(&mut rgb).expect("PNG válido");
    let channels = info.line_size / info.width as usize;
    rgb[..info.buffer_size()]
        .chunks_exact(channels)
        .map(|p| {
            let (r, g, b) = if channels >= 3 {
                (p[0], p[1], p[2])
            } else {
                (p[0], p[0], p[0])
            };
            u16::from(r >> 3) | (u16::from(g >> 3) << 5) | (u16::from(b >> 3) << 10)
        })
        .collect()
}

fn tests_dir() -> Option<PathBuf> {
    std::env::var_os("PSX_TESTS_DIR").map(PathBuf::from)
}

fn console(dir: &Path) -> psx::Console {
    let bios = std::fs::read(dir.join("bios/SCPH1001.BIN")).expect("BIOS em bios/SCPH1001.BIN");
    psx::Console::new(&bios).expect("BIOS válido")
}

fn tty(console: &psx::Console) -> String {
    String::from_utf8_lossy(console.tty_output()).into_owned()
}

#[test]
#[ignore = "precisa do BIOS e dos programas de teste (PSX_TESTS_DIR)"]
fn bios_boots_the_kernel() {
    let Some(dir) = tests_dir() else {
        panic!("defina PSX_TESTS_DIR");
    };
    let mut console = console(&dir);
    for _ in 0..120 {
        console.run_frame();
    }
    let out = tty(&console);
    assert!(out.contains("KERNEL SETUP!"), "{out}");
    assert!(out.contains("System ROM Version 2.2"), "{out}");
}

#[test]
#[ignore = "precisa do BIOS e dos programas de teste (PSX_TESTS_DIR)"]
fn test_programs_pass() {
    let Some(dir) = tests_dir() else {
        panic!("defina PSX_TESTS_DIR");
    };
    let mut failures = Vec::new();
    for name in PROGRAMS {
        let path = dir.join("ps1-tests").join(format!("{name}.exe"));
        let exe = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut console = console(&dir);
        console.load_exe(&exe).expect("PS-EXE válido");
        for _ in 0..FRAMES {
            console.run_frame();
        }
        let out = tty(&console);
        let passed = out.lines().filter(|l| l.starts_with("pass")).count();
        let failed: Vec<&str> = out.lines().filter(|l| l.starts_with("fail")).collect();
        let done = out.contains("Done.");
        println!("{name}: {passed} passaram, {} falharam", failed.len());
        if !failed.is_empty() || !done || passed == 0 {
            failures.push(format!("{name}:\n{}", failed.join("\n")));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "precisa do BIOS e dos programas de teste (PSX_TESTS_DIR)"]
fn gpu_programs_match_hardware_vram() {
    let Some(dir) = tests_dir() else {
        panic!("defina PSX_TESTS_DIR");
    };
    let mut failures = Vec::new();
    for program in GPU_PROGRAMS {
        let GpuProgram {
            name,
            area: (width, height),
            allowed,
            reason,
        } = *program;
        let path = dir.join("ps1-tests").join(format!("{name}.exe"));
        let exe = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut console = console(&dir);
        console.load_exe(&exe).expect("PS-EXE válido");
        for _ in 0..FRAMES {
            console.run_frame();
        }
        let folder = path.parent().expect("pasta do teste");
        let expected = reference_vram(&folder.join("vram.png"));
        let different = console
            .vram()
            .iter()
            .zip(&expected)
            .enumerate()
            .filter(|(i, _)| i % FULL.0 < width && i / FULL.0 < height)
            .filter(|(_, (a, b))| *a & 0x7FFF != **b)
            .count();
        println!("{name}: {different} pixels diferentes (aceito: {allowed}) {reason}");
        if different > allowed {
            failures.push(format!("{name}: {different} pixels diferentes"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "precisa do BIOS e dos programas de teste (PSX_TESTS_DIR)"]
fn gte_test_all_passes() {
    let Some(dir) = tests_dir() else {
        panic!("defina PSX_TESTS_DIR");
    };
    let path = dir.join("ps1-tests/gte/test-all/test-all.exe");
    let exe = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut console = console(&dir);
    console.load_exe(&exe).expect("PS-EXE válido");
    // O programa imprime "Passed tests: N" e "Failed tests: N" no fim.
    for _ in 0..3 * FRAMES {
        if tty(&console).contains("Failed tests:") {
            break;
        }
        console.run_frame();
    }
    let out = tty(&console);
    let count = |label: &str| -> Option<u32> {
        let line = out.lines().find(|l| l.starts_with(label))?;
        line[label.len()..].trim().parse().ok()
    };
    let (passed, failed) = (count("Passed tests:"), count("Failed tests:"));
    println!("gte/test-all: {passed:?} passaram, {failed:?} falharam");
    assert_eq!(failed, Some(0), "{out}");
    assert!(passed.is_some_and(|n| n > 0), "{out}");
}
