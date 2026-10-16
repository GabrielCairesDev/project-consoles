// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use player::controls::Bindings;
use project_consoles_lib::runtime::{KEYS_FLAG, PLAY_FLAG, STATE_FLAG};

/// O mesmo binário é o launcher e, com
/// `--play <rom> [--keys <mapa>] [--state <arquivo>]`, o processo do jogo
/// (ver `crates/player`): sem sidecar, igual em dev e no pacote final.
fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(PLAY_FLAG)) {
        project_consoles_lib::run();
        return ExitCode::SUCCESS;
    }
    let Some(rom) = args.next() else {
        return usage();
    };
    let options = match parse_options(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return usage();
        }
    };
    match player::run(Path::new(&rom), options.bindings, options.state) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

struct PlayOptions {
    bindings: Bindings,
    state: Option<PathBuf>,
}

/// `--keys <mapa>` e `--state <arquivo>`, ambos opcionais; sem `--keys`
/// valem as teclas padrão, sem `--state` o progresso não é salvo.
fn parse_options(mut args: impl Iterator<Item = OsString>) -> Result<PlayOptions, String> {
    let mut options = PlayOptions {
        bindings: Bindings::default(),
        state: None,
    };
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("falta o valor de {}", flag.to_string_lossy()))?;
        if flag == KEYS_FLAG {
            let spec = value.to_str().ok_or("mapa de teclas inválido")?;
            options.bindings = Bindings::parse_arg(spec).map_err(|e| e.to_string())?;
        } else if flag == STATE_FLAG {
            options.state = Some(PathBuf::from(value));
        } else {
            return Err(format!(
                "argumento desconhecido: {}",
                flag.to_string_lossy()
            ));
        }
    }
    Ok(options)
}

fn usage() -> ExitCode {
    eprintln!(
        "uso: {PLAY_FLAG} <caminho da ROM> [{KEYS_FLAG} <ação=tecla,...>] [{STATE_FLAG} <arquivo>]"
    );
    ExitCode::from(2)
}
