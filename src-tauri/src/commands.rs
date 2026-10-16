//! Comandos expostos à interface. Trabalho de disco e rede roda em
//! `spawn_blocking`, fora da thread da interface.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use player::controls::{Bindings, SUPPORTED_KEYS};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::catalog::{self, Console};
use crate::covers::{Cover, CoverStore, LibretroThumbnails};
use crate::library::{self, FingerprintCache, Game};
use crate::runtime::{self, Runner};
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
    runner: Runner,
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
            runner: Runner::default(),
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
    /// O app consegue rodar jogos deste console.
    playable: bool,
}

/// Lista o catálogo de consoles para a interface.
#[tauri::command]
pub fn list_consoles() -> Vec<ConsoleInfo> {
    catalog::consoles()
        .iter()
        .map(|&console| ConsoleInfo {
            console,
            supported: systems::for_console(console.id).is_some(),
            playable: runtime::is_playable(console.id),
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

/// Corpo do evento `game-exited`, emitido quando o processo do jogo termina.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameExited {
    game_id: String,
    /// Código de saída; `null` se o processo foi encerrado por sinal.
    code: Option<i32>,
}

/// Inicia um jogo da última varredura em um processo separado.
///
/// Retorna assim que o processo inicia; o fim é avisado pelo evento
/// `game-exited`.
#[tauri::command]
pub async fn launch_game(
    game_id: String,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<()> {
    let game = lock(&state.games)
        .get(&game_id)
        .cloned()
        .ok_or_else(|| "Jogo não encontrado; atualize a biblioteca.".to_owned())?;
    let bindings = lock(&state.settings)
        .controls_for(&game.console_id)
        .unwrap_or_else(|error| {
            eprintln!("controles salvos inválidos ({error}); usando o padrão");
            Bindings::default()
        });
    let state = Arc::clone(&state);

    tauri::async_runtime::spawn_blocking(move || {
        state
            .runner
            .launch(&game, &bindings, move |code| {
                let payload = GameExited { game_id, code };
                if let Err(error) = app.emit("game-exited", payload) {
                    eprintln!("não foi possível avisar o fim do jogo: {error}");
                }
            })
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Falha interna: {error}"))?
}

/// Teclas de um console, para a tela de configuração de controles.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlsInfo {
    /// Ação → tecla (nomes de `KeyboardEvent.code`).
    bindings: BTreeMap<&'static str, &'static str>,
    defaults: BTreeMap<&'static str, &'static str>,
    /// Teclas que podem ser atribuídas.
    supported_keys: Vec<&'static str>,
}

impl ControlsInfo {
    fn new(bindings: &Bindings) -> Self {
        Self {
            bindings: bindings.pairs().collect(),
            defaults: Bindings::default().pairs().collect(),
            supported_keys: SUPPORTED_KEYS.iter().map(|(name, _)| *name).collect(),
        }
    }
}

fn ensure_playable(console_id: &str) -> CommandResult<()> {
    if runtime::is_playable(console_id) {
        Ok(())
    } else {
        Err("Este console ainda não tem controles configuráveis.".to_owned())
    }
}

/// Teclas configuradas para o console (o padrão, se nunca foram alteradas
/// ou se o que foi salvo está inválido).
#[tauri::command]
pub fn get_controls(
    console_id: String,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<ControlsInfo> {
    ensure_playable(&console_id)?;
    let bindings = lock(&state.settings)
        .controls_for(&console_id)
        .unwrap_or_default();
    Ok(ControlsInfo::new(&bindings))
}

/// Salva as teclas do console. Rejeita mapas incompletos, teclas não
/// suportadas e teclas repetidas; nada muda se a gravação falhar.
#[tauri::command]
pub async fn set_controls(
    console_id: String,
    bindings: HashMap<String, String>,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<ControlsInfo> {
    ensure_playable(&console_id)?;
    let bindings = Bindings::from_pairs(bindings.iter().map(|(a, k)| (a.as_str(), k.as_str())))
        .map_err(|error| format!("Mapa de controles inválido: {error}"))?;
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let mut settings = lock(&state.settings);
        let mut updated = settings.clone();
        updated.set_controls(&console_id, &bindings);
        updated
            .save(&state.settings_path)
            .map_err(|error| format!("Não foi possível salvar os controles: {error}"))?;
        *settings = updated;
        Ok(ControlsInfo::new(&bindings))
    })
    .await
    .map_err(|error| format!("Falha interna: {error}"))?
}
