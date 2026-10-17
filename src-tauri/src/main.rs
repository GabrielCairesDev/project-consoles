// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use player::controls::{Bindings, System};
use player::image::{ImageSettings, Scaler};
use project_consoles_lib::runtime::{
    BATTERY_FLAG, BIOS_FLAG, CONSOLE_FLAG, KEYS_FLAG, PLAY_FLAG, SCALER_FLAG, STATE_FLAG,
};

/// O mesmo binário é o launcher e, com
/// `--play <rom> --console <id> [--keys <mapa>] [--state <arquivo>]
/// [--sav <arquivo>] [--bios <arquivo>] [--scaler <id>]`, o processo do jogo
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
    match player::run(
        Path::new(&rom),
        options.bindings,
        options.state,
        options.battery,
        options.bios,
        options.image,
    ) {
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
    battery: Option<PathBuf>,
    bios: Option<PathBuf>,
    image: ImageSettings,
}

/// `--console <id>` (obrigatório), `--keys <mapa>`, `--state <arquivo>`,
/// `--sav <arquivo>`, `--bios <arquivo>` e `--scaler <id>`; sem `--keys`
/// valem as teclas padrão do console, sem `--state` o progresso não é
/// salvo, sem `--sav` o save do jogo também não e sem `--scaler` vale a
/// ampliação recomendada do console. Se o console não tiver a ampliação,
/// o player recusa ao abrir.
fn parse_options(mut args: impl Iterator<Item = OsString>) -> Result<PlayOptions, String> {
    let mut system = None;
    let mut keys = None;
    let mut state = None;
    let mut battery = None;
    let mut bios = None;
    let mut scaler = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("falta o valor de {}", flag.to_string_lossy()))?;
        if flag == CONSOLE_FLAG {
            let id = value.to_str().ok_or("console inválido")?;
            system = Some(System::from_id(id).ok_or_else(|| format!("console sem núcleo: {id}"))?);
        } else if flag == KEYS_FLAG {
            keys = Some(value.into_string().map_err(|_| "mapa de teclas inválido")?);
        } else if flag == STATE_FLAG {
            state = Some(PathBuf::from(value));
        } else if flag == BATTERY_FLAG {
            battery = Some(PathBuf::from(value));
        } else if flag == BIOS_FLAG {
            bios = Some(PathBuf::from(value));
        } else if flag == SCALER_FLAG {
            let id = value.to_str().unwrap_or_default();
            scaler =
                Some(Scaler::from_id(id).ok_or_else(|| format!("ampliação desconhecida: {id}"))?);
        } else {
            return Err(format!(
                "argumento desconhecido: {}",
                flag.to_string_lossy()
            ));
        }
    }
    let system = system.ok_or_else(|| format!("falta {CONSOLE_FLAG}"))?;
    let bindings = match keys {
        Some(spec) => Bindings::parse_arg(system, &spec).map_err(|e| e.to_string())?,
        None => Bindings::defaults(system),
    };
    Ok(PlayOptions {
        bindings,
        state,
        battery,
        bios,
        image: ImageSettings {
            scaler: scaler.unwrap_or(ImageSettings::recommended(system).scaler),
        },
    })
}

fn usage() -> ExitCode {
    eprintln!(
        "uso: {PLAY_FLAG} <caminho da ROM> {CONSOLE_FLAG} <id> [{KEYS_FLAG} <ação=tecla,...>] [{STATE_FLAG} <arquivo>] [{BATTERY_FLAG} <arquivo>] [{BIOS_FLAG} <arquivo>] [{SCALER_FLAG} <id>]"
    );
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<PlayOptions, String> {
        parse_options(args.iter().map(OsString::from))
    }

    #[test]
    fn scaler_defaults_to_recommended_and_rejects_unknown() {
        let base = [CONSOLE_FLAG, "game-boy"];
        assert_eq!(
            parse(&base).expect("válido").image,
            ImageSettings::recommended(System::GameBoy)
        );
        let original = [CONSOLE_FLAG, "game-boy", SCALER_FLAG, "nearest"];
        assert_eq!(
            parse(&original).expect("válido").image,
            ImageSettings::ORIGINAL
        );
        assert!(parse(&[CONSOLE_FLAG, "game-boy", SCALER_FLAG, "talvez"]).is_err());
        assert!(parse(&[CONSOLE_FLAG, "game-boy", SCALER_FLAG]).is_err());
    }
}
