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
