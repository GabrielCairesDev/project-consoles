//! Programas de teste públicos (ps1-tests, de Jakub Czekański), baixados
//! fora do repositório, e o BIOS do usuário.
//!
//! ```text
//! PSX_TESTS_DIR=~/psx-tests cargo test -p psx --release --test test_programs -- --ignored --nocapture
//! ```
//!
//! `PSX_TESTS_DIR` deve conter `bios/SCPH1001.BIN` e o `tests.zip` do
//! ps1-tests extraído em `ps1-tests/`. `PSX_DISC` aponta para o `.cue` de
//! um jogo seu, usado só pelo teste de partida do disco.
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

/// A resolução interna é só apresentação: com 4× a VRAM 1× e o texto de
/// cada programa de GPU são os mesmos que em 1×.
#[test]
#[ignore = "precisa do BIOS e dos programas de teste (PSX_TESTS_DIR)"]
fn gpu_programs_ignore_internal_resolution() {
    let Some(dir) = tests_dir() else {
        panic!("defina PSX_TESTS_DIR");
    };
    for program in GPU_PROGRAMS {
        let path = dir.join("ps1-tests").join(format!("{}.exe", program.name));
        let exe = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let run = |resolution| {
            let mut console = console(&dir);
            console.set_internal_resolution(resolution);
            console.load_exe(&exe).expect("PS-EXE válido");
            for _ in 0..FRAMES {
                console.run_frame();
            }
            (console.vram().to_vec(), tty(&console), console.cycles())
        };
        let native = run(psx::InternalResolution::Native);
        let upscaled = run(psx::InternalResolution::X4);
        assert!(native == upscaled, "{}: 4× mudou a emulação", program.name);
    }
}

/// Vídeo do MDEC: a foto do `mdec/frame` (320×240, 15 bits, DMA). A
/// fórmula exata da IDCT do hardware não é conhecida, então cada canal pode
/// diferir em 1 (de 31); hoje 17 848 pixels diferem assim.
#[test]
#[ignore = "precisa do BIOS e dos programas de teste (PSX_TESTS_DIR)"]
fn mdec_frame_matches_hardware_vram() {
    let Some(dir) = tests_dir() else {
        panic!("defina PSX_TESTS_DIR");
    };
    let folder = dir.join("ps1-tests/mdec/frame");
    let path = folder.join("frame-15bit-dma.exe");
    let exe = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut console = console(&dir);
    console.load_exe(&exe).expect("PS-EXE válido");
    for _ in 0..FRAMES / 2 {
        console.run_frame();
    }
    let expected = reference_vram(&folder.join("vram-15bit.png"));
    let mut different = 0;
    for y in 0..240 {
        for x in 0..320 {
            let i = y * FULL.0 + x;
            let (ours, theirs) = (console.vram()[i], expected[i]);
            let channel = |p: u16, c: u16| i32::from((p >> (5 * c)) & 31);
            let worst = (0..3)
                .map(|c| (channel(ours, c) - channel(theirs, c)).abs())
                .max()
                .unwrap_or(0);
            assert!(
                worst <= 1,
                "pixel ({x}, {y}): {ours:#06x}, hardware {theirs:#06x}"
            );
            different += usize::from(worst > 0);
        }
    }
    println!("mdec/frame: {different} pixels a 1 nível do hardware");
    assert!(different <= 18_000, "{different} pixels diferentes");
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

#[test]
#[ignore = "precisa do BIOS e dos programas de teste (PSX_TESTS_DIR)"]
fn gte_fuzz_matches_hardware_log() {
    let Some(dir) = tests_dir() else {
        panic!("defina PSX_TESTS_DIR");
    };
    let fuzz = dir.join("ps1-tests/gte-fuzz");
    let exe = std::fs::read(fuzz.join("gte-fuzz.exe")).expect("gte-fuzz.exe");
    let reference = std::fs::read(fuzz.join("gte_valid_0xc0ffee_50.log")).expect("log do hardware");
    let reference = String::from_utf8_lossy(&reference).replace('\r', "");
    let mut console = console(&dir);
    console.load_exe(&exe).expect("PS-EXE válido");
    // O programa espera o controle: START roda o fuzz dos comandos válidos.
    let mut out = Vec::new();
    for frame in 0..40_000 {
        console.set_input(psx::Buttons {
            start: (200..230).contains(&frame),
            ..psx::Buttons::default()
        });
        console.run_frame();
        out.extend(console.take_tty_output());
        if out.ends_with(b"==== END ====\n") || out.ends_with(b"==== END ====\r\n") {
            break;
        }
    }
    let out = String::from_utf8_lossy(&out).replace('\r', "");
    let start = out.find("==== VALID CMD FUZZ").expect("o fuzz começou");
    let got = &out[start..];
    let first_diff = got.lines().zip(reference.lines()).position(|(a, b)| a != b);
    assert_eq!(first_diff, None, "primeira linha diferente");
    assert_eq!(got.lines().count(), reference.lines().count());
}

#[test]
#[ignore = "precisa do BIOS (PSX_TESTS_DIR) e de um jogo (PSX_DISC)"]
fn bios_boots_the_game_from_disc() {
    let (Some(dir), Some(disc)) = (tests_dir(), std::env::var_os("PSX_DISC")) else {
        panic!("defina PSX_TESTS_DIR e PSX_DISC");
    };
    let mut console = console(&dir);
    console.insert_disc(psx::Disc::open(Path::new(&disc)).expect("imagem válida"));
    for _ in 0..900 {
        console.run_frame();
    }
    // O BIOS leu o SYSTEM.CNF e carregou o executável do jogo.
    let out = tty(&console);
    assert!(out.contains("BOOT ="), "{out}");
    assert!(out.contains("EXEC:PC0("), "{out}");
}

/// Desempenho da resolução interna num jogo de verdade (release). Mede o
/// tempo por quadro em cada escala, depois da abertura, e confere que a
/// emulação (VRAM 1× e ciclos) é idêntica em todas. Com `PSX_SCREENSHOT_DIR`
/// grava o último quadro de cada escala em PNG.
///
/// ```text
/// PSX_TESTS_DIR=~/psx-tests PSX_DISC=jogo.cue cargo test -p psx --release \
///     --test test_programs internal_resolution_speed -- --ignored --nocapture
/// ```
#[test]
#[ignore = "precisa do BIOS (PSX_TESTS_DIR) e de um jogo (PSX_DISC)"]
fn internal_resolution_speed() {
    let (Some(dir), Some(disc)) = (tests_dir(), std::env::var_os("PSX_DISC")) else {
        panic!("defina PSX_TESTS_DIR e PSX_DISC");
    };
    const WARMUP: u32 = 1500;
    const MEASURED: u32 = 1200;
    let mut reference = None;
    for scale in 1..=4 {
        let resolution = psx::InternalResolution::from_scale(scale).expect("escala de 1 a 4");
        let mut console = console(&dir);
        console.set_internal_resolution(resolution);
        console.insert_disc(psx::Disc::open(Path::new(&disc)).expect("imagem válida"));
        for _ in 0..WARMUP {
            console.run_frame();
        }
        let mut worst = std::time::Duration::ZERO;
        let start = std::time::Instant::now();
        for _ in 0..MEASURED {
            let frame = std::time::Instant::now();
            console.run_frame();
            worst = worst.max(frame.elapsed());
        }
        let elapsed = start.elapsed();
        let fps = f64::from(MEASURED) / elapsed.as_secs_f64();
        let (_, size) = console.framebuffer();
        println!(
            "{scale}×: {fps:.0} quadros/s (pior quadro {:.1} ms), imagem {}×{}",
            worst.as_secs_f64() * 1000.0,
            size.0,
            size.1
        );
        if let Some(out) = std::env::var_os("PSX_SCREENSHOT_DIR") {
            save_png(&Path::new(&out).join(format!("{scale}x.png")), &console);
        }
        let state = (console.vram().to_vec(), console.cycles());
        match &reference {
            None => reference = Some(state),
            Some(native) => assert!(*native == state, "{scale}× mudou a emulação"),
        }
    }
}

fn save_png(path: &Path, console: &psx::Console) {
    let (pixels, (width, height)) = console.framebuffer();
    let file = std::fs::File::create(path).expect("criar o PNG");
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let rgb: Vec<u8> = pixels
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    let mut writer = encoder.write_header().expect("cabeçalho do PNG");
    writer.write_image_data(&rgb).expect("gravar o PNG");
}
