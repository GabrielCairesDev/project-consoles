//! Comandos expostos à interface. Trabalho de disco e rede roda em
//! `spawn_blocking`, fora da thread da interface.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tauri::State;

use crate::catalog::{self, Console};
use crate::covers::{Cover, CoverStore, LibretroThumbnails};
use crate::library::{self, FingerprintCache, Game};
use crate::settings::Settings;
use crate::systems;

/// Estado compartilhado entre os comandos.
pub struct AppState {
    settings_path: PathBuf,
    fingerprints_path: PathBuf,
    settings: Mutex<Settings>,
    fingerprints: Mutex<FingerprintCache>,
    /// Jogos da última varredura, por `id`, para localizar a ROM de cada capa
    /// sem varrer de novo.
    games: Mutex<HashMap<String, Game>>,
    covers: CoverStore,
    boxarts: LibretroThumbnails,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf, cache_dir: PathBuf) -> Self {
        let settings_path = config_dir.join("settings.json");
        let fingerprints_path = data_dir.join("rom-index.json");
        let settings = Settings::load(&settings_path).unwrap_or_else(|error| {
            // Não sobrescreve o arquivo: só volta ao padrão nesta execução.
            eprintln!(
                "preferências ilegíveis ({}): {error}",
                settings_path.display()
            );
            Settings::default()
        });
        Self {
            fingerprints: Mutex::new(FingerprintCache::load(&fingerprints_path)),
            settings: Mutex::new(settings),
            settings_path,
            fingerprints_path,
            games: Mutex::new(HashMap::new()),
            covers: CoverStore::new(cache_dir.join("covers")),
            boxarts: LibretroThumbnails::default(),
        }
    }
}

type CommandResult<T> = Result<T, String>;

/// Um mutex envenenado só significa que outra thread entrou em pânico; o dado
/// continua coerente para leitura e reescrita completas, então seguimos.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleInfo {
    #[serde(flatten)]
    console: Console,
    /// O app reconhece ROMs deste console.
    supported: bool,
}

/// Lista o catálogo de consoles para a interface.
#[tauri::command]
pub fn list_consoles() -> Vec<ConsoleInfo> {
    catalog::consoles()
        .iter()
        .map(|&console| ConsoleInfo {
            console,
            supported: systems::for_console(console.id).is_some(),
        })
        .collect()
}

#[tauri::command]
pub fn get_library_dir(state: State<'_, Arc<AppState>>) -> Option<PathBuf> {
    lock(&state.settings).library_dir.clone()
}

/// Define a pasta da biblioteca.
#[tauri::command]
pub async fn set_library_dir(
    path: PathBuf,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<PathBuf> {
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        if !path.is_dir() {
            return Err("A pasta escolhida não existe ou não é uma pasta.".to_owned());
        }
        let mut settings = lock(&state.settings);
        settings.library_dir = Some(path.clone());
        settings
            .save(&state.settings_path)
            .map_err(|error| format!("Não foi possível salvar a preferência: {error}"))?;
        lock(&state.games).clear();
        Ok(path)
    })
    .await
    .map_err(|error| format!("Falha interna: {error}"))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryScan {
    folder: PathBuf,
    /// Jogos de todos os consoles; cada um traz o `consoleId`.
    games: Vec<Game>,
}

/// Varre a pasta da biblioteca (com subpastas) e identifica os jogos.
#[tauri::command]
pub async fn scan_library(state: State<'_, Arc<AppState>>) -> CommandResult<LibraryScan> {
    let folder = lock(&state.settings)
        .library_dir
        .clone()
        .ok_or_else(|| "Escolha a pasta da biblioteca primeiro.".to_owned())?;
    let state = Arc::clone(&state);

    tauri::async_runtime::spawn_blocking(move || {
        let games = {
            let mut fingerprints = lock(&state.fingerprints);
            let games = library::scan(&folder, &mut fingerprints, |fp| {
                systems::identify(fp.size, fp.crc32)
                    .map(|(system, name)| (system.console_id, name.to_owned()))
            })
            .map_err(|error| format!("Não foi possível ler {}: {error}", folder.display()))?;
            if let Err(error) = fingerprints.save(&state.fingerprints_path) {
                // Só custa recalcular na próxima vez; não impede a listagem.
                eprintln!("não foi possível salvar o índice de ROMs: {error}");
            }
            games
        };
        *lock(&state.games) = games.iter().map(|g| (g.id.clone(), g.clone())).collect();
        Ok(LibraryScan { folder, games })
    })
    .await
    .map_err(|error| format!("Falha interna: {error}"))?
}

/// Capa de um jogo da última varredura; baixa ou extrai se necessário.
#[tauri::command]
pub async fn game_cover(
    game_id: String,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<Option<Cover>> {
    let Some(game) = lock(&state.games).get(&game_id).cloned() else {
        return Ok(None);
    };
    let Some(system) = systems::for_console(&game.console_id) else {
        return Ok(None);
    };
    let state = Arc::clone(&state);

    tauri::async_runtime::spawn_blocking(move || {
        state
            .covers
            .resolve(system, &game, &state.boxarts)
            .map_err(|error| format!("Não foi possível obter a capa: {error}"))
    })
    .await
    .map_err(|error| format!("Falha interna: {error}"))?
}
