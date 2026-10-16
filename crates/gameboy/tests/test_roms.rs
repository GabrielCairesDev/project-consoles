//! ROMs de teste públicas (Blargg e Mooneye), baixadas fora do repositório.
//!
//! ```text
//! GB_TESTS_DIR=~/gb-tests cargo test -p gameboy --release --test test_roms -- --ignored --nocapture
//! ```
//!
//! `GB_TESTS_DIR` deve conter `blargg/gb-test-roms-master` e
//! `mooneye/mts-*`. As duas suítes informam o resultado pela serial:
//! Blargg escreve "Passed"/"Failed" e Mooneye envia 3, 5, 8, 13, 21, 34
//! quando passa.

use std::path::{Path, PathBuf};

use gameboy::Console;

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
        "boot_hwio-dmgABCmgb.gb",
        "valores iniciais dos registradores de som: vêm com a APU (etapa 3)",
    ),
    (
        "bits/unused_hwio-GS.gb",
        "bits não usados dos registradores de som: vêm com a APU (etapa 3)",
    ),
];

const MOONEYE_PASS: &[u8] = &[3, 5, 8, 13, 21, 34];
const MAX_FRAMES: u32 = 60 * 120;

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Pass,
    Fail(String),
}

fn run(path: &Path, blargg: bool) -> Outcome {
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
        if blargg {
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

    let mut cases: Vec<(PathBuf, bool)> = Vec::new();
    for dir in ["cpu_instrs/individual", "mem_timing/individual"] {
        cases.extend(roms_in(&blargg.join(dir)).into_iter().map(|p| (p, true)));
    }
    cases.push((blargg.join("instr_timing/instr_timing.gb"), true));
    for dir in [
        "acceptance",
        "acceptance/bits",
        "acceptance/instr",
        "acceptance/interrupts",
        "acceptance/oam_dma",
        "acceptance/timer",
        "emulator-only/mbc1",
        "emulator-only/mbc2",
        "emulator-only/mbc5",
    ] {
        cases.extend(
            roms_in(&mooneye.join(dir))
                .into_iter()
                .filter(|p| mooneye_applies(p))
                .map(|p| (p, false)),
        );
    }

    let mut unexpected = Vec::new();
    let mut passed = 0;
    for (path, is_blargg) in &cases {
        let name = path
            .strip_prefix(&base)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let outcome = run(path, *is_blargg);
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
