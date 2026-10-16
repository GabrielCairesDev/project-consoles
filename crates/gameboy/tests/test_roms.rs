//! ROMs de teste públicas (Blargg e Mooneye), baixadas fora do repositório.
//!
//! ```text
//! GB_TESTS_DIR=~/gb-tests cargo test -p gameboy --release --test test_roms -- --ignored --nocapture
//! ```
//!
//! `GB_TESTS_DIR` deve conter `blargg/gb-test-roms-master`, `mooneye/mts-*`,
//! `dmg-acid2.gb` e a imagem de referência `dmg-acid2-reference.png`.
//!
//! O resultado vem pela serial (Blargg escreve "Passed"/"Failed"; Mooneye
//! envia 3, 5, 8, 13, 21, 34 quando passa) ou, no `dmg_sound` do Blargg,
//! na RAM do cartucho: assinatura DE B0 61 em A001 e o código em A000
//! (0x80 = rodando, 0 = passou).

use std::path::{Path, PathBuf};

use gameboy::{Console, SCREEN_HEIGHT, SCREEN_WIDTH};

/// Testes que ainda não passam, com o motivo. Qualquer outro que falhe, ou
/// um destes que passe, faz o teste falhar (para a lista não ficar velha).
const KNOWN_FAILURES: &[(&str, &str)] = &[
    (
        "timer/rapid_toggle.gb",
        "a interrupção chega uma volta depois (C = D8, esperado D9): depende da fase \
         dentro do M-ciclo entre escritas no DIV e no TAC, que o modelo por M-ciclo \
         não representa sem quebrar os outros testes do timer",
    ),
    (
        "ppu/lcdon_timing-GS.gb",
        "STAT logo depois de ligar o LCD: a 11ª leitura mostra a coincidência \
         LY=LYC um ponto além do hardware",
    ),
    (
        "ppu/lcdon_write_timing-GS.gb",
        "acesso à OAM/VRAM ponto a ponto logo depois de ligar o LCD",
    ),
    (
        "ppu/stat_lyc_onoff.gb",
        "flag de coincidência LY=LYC ao ligar/desligar o LCD",
    ),
    (
        "dmg_sound/rom_singles/09-wave read while on.gb",
        "leitura da wave RAM com o canal 3 tocando: no DMG só funciona no ciclo exato \
         em que o canal lê a RAM",
    ),
    (
        "dmg_sound/rom_singles/10-wave trigger while on.gb",
        "trigger do canal 3 tocando corrompe a wave RAM no DMG (não emulado)",
    ),
    (
        "dmg_sound/rom_singles/12-wave write while on.gb",
        "escrita na wave RAM com o canal 3 tocando (mesmo caso do 09)",
    ),
];

const MOONEYE_PASS: &[u8] = &[3, 5, 8, 13, 21, 34];
const MAX_FRAMES: u32 = 60 * 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    BlarggSerial,
    BlarggMemory,
    Mooneye,
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Pass,
    Fail(String),
}

fn run(path: &Path, protocol: Protocol) -> Outcome {
    let rom = match std::fs::read(path) {
        Ok(rom) => rom,
        Err(e) => return Outcome::Fail(format!("leitura: {e}")),
    };
    let mut console = match Console::new(rom, 44_100) {
        Ok(console) => console,
        Err(e) => return Outcome::Fail(format!("ROM: {e}")),
    };
    for _ in 0..MAX_FRAMES {
        console.run_frame();
        let out = console.serial_output();
        if protocol == Protocol::BlarggMemory {
            let signature = [
                console.peek(0xA001),
                console.peek(0xA002),
                console.peek(0xA003),
            ];
            let code = console.peek(0xA000);
            if signature == [0xDE, 0xB0, 0x61] && code != 0x80 {
                return if code == 0 {
                    Outcome::Pass
                } else {
                    Outcome::Fail(format!("código {code}"))
                };
            }
        } else if protocol == Protocol::BlarggSerial {
            let text = String::from_utf8_lossy(out);
            if text.contains("Passed") {
                return Outcome::Pass;
            }
            if text.contains("Failed") || text.contains("rror") {
                // Deixa a ROM terminar de escrever o detalhe.
                for _ in 0..30 {
                    console.run_frame();
                }
                return Outcome::Fail(
                    String::from_utf8_lossy(console.serial_output()).into_owned(),
                );
            }
        } else if out.len() >= MOONEYE_PASS.len() {
            return if out[..6] == *MOONEYE_PASS {
                Outcome::Pass
            } else {
                Outcome::Fail(format!("serial {:?}", &out[..6]))
            };
        }
    }
    Outcome::Fail(format!(
        "sem resultado; serial: {:?}",
        String::from_utf8_lossy(console.serial_output())
    ))
}

fn roms_in(dir: &Path) -> Vec<PathBuf> {
    let mut roms: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|ext| ext == "gb"))
                .collect()
        })
        .unwrap_or_default();
    roms.sort();
    roms
}

fn find_dir(base: &Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(base)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(prefix))
        })
}

/// O sufixo do nome diz em quais modelos o teste vale: em maiúsculas, um
/// grupo (G = DMG e MGB, S = SGB, C = CGB, A = AGB); em minúsculas, um
/// modelo. Só os que valem para o DMG ABC (o que emulamos) entram.
fn mooneye_applies(path: &Path) -> bool {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some((_, models)) = name.rsplit_once('-') else {
        return true;
    };
    if models.chars().all(|c| c.is_ascii_uppercase()) {
        models.contains('G')
    } else {
        models.contains("dmgABC")
    }
}

#[test]
#[ignore = "precisa das ROMs de teste em GB_TESTS_DIR"]
fn public_test_roms() {
    let base = PathBuf::from(std::env::var_os("GB_TESTS_DIR").expect("defina GB_TESTS_DIR"));
    let blargg = base.join("blargg/gb-test-roms-master");
    let mooneye = find_dir(&base.join("mooneye"), "mts-").expect("pasta do Mooneye");

    let mut cases: Vec<(PathBuf, Protocol)> = Vec::new();
    for dir in ["cpu_instrs/individual", "mem_timing/individual"] {
        cases.extend(
            roms_in(&blargg.join(dir))
                .into_iter()
                .map(|p| (p, Protocol::BlarggSerial)),
        );
    }
    cases.push((
        blargg.join("instr_timing/instr_timing.gb"),
        Protocol::BlarggSerial,
    ));
    cases.extend(
        roms_in(&blargg.join("dmg_sound/rom_singles"))
            .into_iter()
            .map(|p| (p, Protocol::BlarggMemory)),
    );
    for dir in [
        "acceptance",
        "acceptance/bits",
        "acceptance/instr",
        "acceptance/interrupts",
        "acceptance/oam_dma",
        "acceptance/ppu",
        "acceptance/timer",
        "emulator-only/mbc1",
        "emulator-only/mbc2",
        "emulator-only/mbc5",
    ] {
        cases.extend(
            roms_in(&mooneye.join(dir))
                .into_iter()
                .filter(|p| mooneye_applies(p))
                .map(|p| (p, Protocol::Mooneye)),
        );
    }

    let mut unexpected = Vec::new();
    let mut passed = 0;
    for (path, protocol) in &cases {
        let name = path
            .strip_prefix(&base)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let outcome = run(path, *protocol);
        let known = KNOWN_FAILURES.iter().find(|(n, _)| name.ends_with(n));
        match (&outcome, known) {
            (Outcome::Pass, None) => passed += 1,
            (Outcome::Pass, Some(_)) => unexpected.push(format!("{name}: passou (tirar da lista)")),
            (Outcome::Fail(_), Some(_)) => {}
            (Outcome::Fail(why), None) => unexpected.push(format!("{name}: {why}")),
        }
    }
    println!("{passed}/{} passaram", cases.len());
    assert!(unexpected.is_empty(), "\n{}", unexpected.join("\n"));
}

/// Tom de cinza (0 = branco … 3 = preto) de um pixel `0x00RRGGBB`.
fn shade_index(pixel: u32) -> u8 {
    let level = (pixel & 0xFF) as u8;
    3 - level / 64
}

#[test]
#[ignore = "precisa das ROMs de teste em GB_TESTS_DIR"]
fn dmg_acid2_matches_reference() {
    let base = PathBuf::from(std::env::var_os("GB_TESTS_DIR").expect("defina GB_TESTS_DIR"));
    let rom = std::fs::read(base.join("dmg-acid2.gb")).expect("dmg-acid2.gb");
    let mut console = Console::new(rom, 44_100).expect("ROM válida");
    for _ in 0..60 {
        console.run_frame();
    }

    let file = std::fs::File::open(base.join("dmg-acid2-reference.png")).expect("referência");
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("PNG válido");
    let mut pixels = vec![0; reader.output_buffer_size().expect("tamanho")];
    let info = reader.next_frame(&mut pixels).expect("quadro do PNG");
    assert_eq!(
        (info.width as usize, info.height as usize),
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    );
    let channels = info.line_size / info.width as usize;

    let mut differences = Vec::new();
    for (i, &pixel) in console.framebuffer().iter().enumerate() {
        let expected = 3 - pixels[i * channels] / 64;
        if shade_index(pixel) != expected {
            differences.push((i % SCREEN_WIDTH, i / SCREEN_WIDTH));
        }
    }
    assert!(
        differences.is_empty(),
        "{} pixels diferentes, os primeiros: {:?}",
        differences.len(),
        &differences[..differences.len().min(10)]
    );
}
