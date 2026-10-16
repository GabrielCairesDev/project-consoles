// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::ExitCode;

use player::controls::Bindings;
use project_consoles_lib::runtime::{KEYS_FLAG, PLAY_FLAG};

/// O mesmo binário é o launcher e, com `--play <rom> [--keys <mapa>]`, o
/// processo do jogo (ver `crates/player`): sem sidecar, igual em dev e no
/// pacote final.
fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(PLAY_FLAG)) {
        project_consoles_lib::run();
        return ExitCode::SUCCESS;
    }
    let Some(rom) = args.next() else {
        return usage();
    };
    let bindings = match parse_bindings(args) {
        Ok(bindings) => bindings,
        Err(message) => {
            eprintln!("{message}");
            return usage();
        }
    };
    match player::run(Path::new(&rom), bindings) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// `--keys <mapa>` opcional; sem ele, valem as teclas padrão.
fn parse_bindings(mut args: impl Iterator<Item = OsString>) -> Result<Bindings, String> {
    match args.next() {
        None => Ok(Bindings::default()),
        Some(flag) if flag == KEYS_FLAG => {
            let spec = args.next().ok_or("falta o mapa de teclas")?;
            let spec = spec.to_str().ok_or("mapa de teclas inválido")?;
            Bindings::parse_arg(spec).map_err(|e| e.to_string())
        }
        Some(other) => Err(format!(
            "argumento desconhecido: {}",
            other.to_string_lossy()
        )),
    }
}

fn usage() -> ExitCode {
    eprintln!("uso: {PLAY_FLAG} <caminho da ROM> [{KEYS_FLAG} <ação=tecla,...>]");
    ExitCode::from(2)
}
