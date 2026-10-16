//! Conformidade do 68000 com a suíte SingleStepTests/680x0.
//!
//! Os arquivos não são versionados (o repositório não declara licença). Para
//! rodar, baixe `68000/v1/*.json.gz` de github.com/SingleStepTests/680x0 e:
//!
//! ```text
//! M68K_TESTS_DIR=~/m68k-tests cargo test -p genesis --release --test m68k_suite -- --ignored --nocapture
//! ```
//!
//! `M68K_FILTER=ADD` restringe aos arquivos cujo nome começa com o filtro.
//! Confere registradores, SR, PC, fila de prefetch, RAM e ciclos.

use std::collections::HashMap;
use std::io::Read;

use genesis::m68k::{Bus, Cpu, Registers};
use serde_json::Value;

/// Casos da suíte que não seguimos, com o motivo:
/// - ASL.b 1583 e 1761: o esperado altera os 24 bits altos de D2, o que um
///   ASL.b não faz (defeito do gerador).
/// - DIVU 5745: divisão por zero que empilha o endereço da própria
///   instrução, diferente dos outros 8.064 casos do arquivo.
const KNOWN_DISCREPANCIES: &[&str] = &[
    "e502 [ASL.b Q, D2] 1583",
    "e502 [ASL.b Q, D2] 1761",
    "80ef [DIVU (d16, A7), D0] 5745",
];

/// Memória esparsa: só os bytes que o teste define (o resto lê 0).
#[derive(Default)]
struct SparseRam(HashMap<u32, u8>);

impl Bus for SparseRam {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.0.get(&addr).copied().unwrap_or(0)
    }
    fn read_word(&mut self, addr: u32) -> u16 {
        u16::from(self.read_byte(addr)) << 8 | u16::from(self.read_byte(addr | 1))
    }
    fn write_byte(&mut self, addr: u32, value: u8) {
        self.0.insert(addr, value);
    }
    fn write_word(&mut self, addr: u32, value: u16) {
        self.write_byte(addr, (value >> 8) as u8);
        self.write_byte(addr | 1, value as u8);
    }
}

fn u32_of(v: &Value, key: &str) -> u32 {
    v[key].as_u64().unwrap_or_else(|| panic!("campo {key}")) as u32
}

fn registers(state: &Value) -> Registers {
    let mut d = [0; 8];
    for (i, r) in d.iter_mut().enumerate() {
        *r = u32_of(state, &format!("d{i}"));
    }
    let mut a = [0; 7];
    for (i, r) in a.iter_mut().enumerate() {
        *r = u32_of(state, &format!("a{i}"));
    }
    let prefetch = &state["prefetch"];
    Registers {
        d,
        a,
        usp: u32_of(state, "usp"),
        ssp: u32_of(state, "ssp"),
        sr: u32_of(state, "sr") as u16,
        pc: u32_of(state, "pc"),
        prefetch: [
            prefetch[0].as_u64().unwrap_or(0) as u16,
            prefetch[1].as_u64().unwrap_or(0) as u16,
        ],
    }
}

fn ram(state: &Value) -> Vec<(u32, u8)> {
    state["ram"]
        .as_array()
        .map(|cells| {
            cells
                .iter()
                .map(|c| {
                    (
                        c[0].as_u64().unwrap_or(0) as u32,
                        c[1].as_u64().unwrap_or(0) as u8,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Diferenças entre o esperado e o obtido, ou vazio se passou.
fn run_case(case: &Value, check_cycles: bool) -> Vec<String> {
    let mut bus = SparseRam::default();
    for (addr, value) in ram(&case["initial"]) {
        bus.write_byte(addr, value);
    }
    let mut cpu = Cpu::new();
    cpu.set_registers(&registers(&case["initial"]));
    let cycles = cpu.step(&mut bus);

    let expected = registers(&case["final"]);
    let got = cpu.registers();
    let mut diffs = Vec::new();
    for i in 0..8 {
        if got.d[i] != expected.d[i] {
            diffs.push(format!("d{i}: {:08X} != {:08X}", got.d[i], expected.d[i]));
        }
    }
    for i in 0..7 {
        if got.a[i] != expected.a[i] {
            diffs.push(format!("a{i}: {:08X} != {:08X}", got.a[i], expected.a[i]));
        }
    }
    for (name, g, e) in [
        ("usp", got.usp, expected.usp),
        ("ssp", got.ssp, expected.ssp),
        ("pc", got.pc, expected.pc),
        ("sr", u32::from(got.sr), u32::from(expected.sr)),
    ] {
        if g != e {
            diffs.push(format!("{name}: {g:X} != {e:X}"));
        }
    }
    if got.prefetch != expected.prefetch {
        diffs.push(format!(
            "prefetch: {:04X?} != {:04X?}",
            got.prefetch, expected.prefetch
        ));
    }
    for (addr, value) in ram(&case["final"]) {
        let g = bus.read_byte(addr);
        if g != value {
            diffs.push(format!("ram[{addr:06X}]: {g:02X} != {value:02X}"));
        }
    }
    let length = case["length"].as_u64().unwrap_or(0) as u32;
    if check_cycles && cycles != length {
        diffs.push(format!("ciclos: {cycles} != {length}"));
    }
    diffs
}

#[test]
#[ignore = "requer M68K_TESTS_DIR com os arquivos da SingleStepTests/680x0"]
fn single_step_tests() {
    let dir = std::env::var("M68K_TESTS_DIR").expect("defina M68K_TESTS_DIR");
    let filter = std::env::var("M68K_FILTER").unwrap_or_default();
    let check_cycles = std::env::var("M68K_SKIP_CYCLES").is_err();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("pasta de testes")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name.ends_with(".json.gz") && name.starts_with(filter.as_str())
        })
        .collect();
    files.sort();

    let (mut total, mut failed, mut skipped) = (0usize, 0usize, 0usize);
    let mut summary = Vec::new();
    for path in &files {
        let mut text = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(path).expect("arquivo"))
            .read_to_string(&mut text)
            .expect("gzip");
        let cases: Value = serde_json::from_str(&text).expect("json");
        let cases = cases.as_array().expect("lista de testes");
        let mut file_failed = 0;
        let mut shown = 0;
        for case in cases {
            if KNOWN_DISCREPANCIES.contains(&case["name"].as_str().unwrap_or_default()) {
                skipped += 1;
                continue;
            }
            let diffs = run_case(case, check_cycles);
            if !diffs.is_empty() {
                file_failed += 1;
                if shown < 2 {
                    shown += 1;
                    println!("FALHOU {}: {}", case["name"], diffs.join("; "));
                }
            }
        }
        total += cases.len();
        failed += file_failed;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        summary.push(format!(
            "{name}: {}/{}",
            cases.len() - file_failed,
            cases.len()
        ));
    }
    println!("\n{}", summary.join("\n"));
    println!(
        "\nTOTAL: {}/{} passaram, {failed} falhas, {skipped} ignorados (KNOWN_DISCREPANCIES)",
        total - failed - skipped,
        total - skipped
    );
    assert_eq!(failed, 0, "{failed} de {total} testes falharam");
}
