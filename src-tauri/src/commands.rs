//! Comandos expostos à interface. Trabalho de disco e rede roda em
//! `spawn_blocking`, fora da thread da interface.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use player::controls::{Bindings, SUPPORTED_KEYS, System};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::catalog::{self, Console};
use crate::covers::{Cover, CoverStore, LibretroThumbnails};
use crate::library::{self, FingerprintCache, Game};
use crate::runtime::{self, LaunchOptions, Runner};
use crate::saves::SaveStore;
use crate::settings::{ConsoleSettings, Settings};
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
    saves: SaveStore,
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
            saves: SaveStore::new(data_dir.join("saves")),
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
        let mut games = games;
        for game in &mut games {
            game.has_save = state.saves.exists(game);
        }
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
    /// O jogo deixou progresso salvo para continuar.
    has_save: bool,
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
    let system = runtime::system_for(&game.console_id)
        .ok_or_else(|| "Ainda não é possível jogar títulos deste console.".to_owned())?;
    let (bindings, console_settings) = {
        let settings = lock(&state.settings);
        let bindings = settings.controls_for(system).unwrap_or_else(|error| {
            eprintln!("controles salvos inválidos ({error}); usando o padrão");
            Bindings::defaults(system)
        });
        (bindings, settings.console(&game.console_id))
    };
    let options = LaunchOptions {
        bindings,
        // Desligado: o jogo começa do início e não grava progresso; o que já
        // estava salvo fica intacto para quando a opção voltar.
        state_path: console_settings.resume.then(|| state.saves.path_for(&game)),
    };
    let state = Arc::clone(&state);

    tauri::async_runtime::spawn_blocking(move || {
        let watcher = Arc::clone(&state);
        let exited = game.clone();
        state
            .runner
            .launch(&game, &options, move |code| {
                let has_save = watcher.saves.exists(&exited);
                let payload = GameExited {
                    game_id,
                    code,
                    has_save,
                };
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
    /// Ações do controle deste console, na ordem de exibição.
    actions: Vec<&'static str>,
    /// Ação → tecla (nomes de `KeyboardEvent.code`).
    bindings: BTreeMap<&'static str, &'static str>,
    defaults: BTreeMap<&'static str, &'static str>,
    /// Teclas que podem ser atribuídas.
    supported_keys: Vec<&'static str>,
}

impl ControlsInfo {
    fn new(bindings: &Bindings) -> Self {
        let system = bindings.system();
        Self {
            actions: system.actions().iter().map(|a| a.id()).collect(),
            bindings: bindings.pairs().collect(),
            defaults: Bindings::defaults(system).pairs().collect(),
            supported_keys: SUPPORTED_KEYS.iter().map(|(name, _)| *name).collect(),
        }
    }
}

fn playable_system(console_id: &str) -> CommandResult<System> {
    runtime::system_for(console_id)
        .ok_or_else(|| "Este console ainda não tem controles configuráveis.".to_owned())
}

/// Teclas configuradas para o console (o padrão, se nunca foram alteradas
/// ou se o que foi salvo está inválido).
#[tauri::command]
pub fn get_controls(
    console_id: String,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<ControlsInfo> {
    let system = playable_system(&console_id)?;
    let bindings = lock(&state.settings)
        .controls_for(system)
        .unwrap_or_else(|_| Bindings::defaults(system));
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
    let system = playable_system(&console_id)?;
    let bindings = Bindings::from_pairs(
        system,
        bindings.iter().map(|(a, k)| (a.as_str(), k.as_str())),
    )
    .map_err(|error| format!("Mapa de controles inválido: {error}"))?;
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let mut settings = lock(&state.settings);
        let mut updated = settings.clone();
        updated.set_controls(&bindings);
        updated
            .save(&state.settings_path)
            .map_err(|error| format!("Não foi possível salvar os controles: {error}"))?;
        *settings = updated;
        Ok(ControlsInfo::new(&bindings))
    })
    .await
    .map_err(|error| format!("Falha interna: {error}"))?
}

/// Apaga o progresso salvo de um jogo, para recomeçar do início.
#[tauri::command]
pub async fn delete_save(game_id: String, state: State<'_, Arc<AppState>>) -> CommandResult<()> {
    let game = lock(&state.games)
        .get(&game_id)
        .cloned()
        .ok_or_else(|| "Jogo não encontrado; atualize a biblioteca.".to_owned())?;
    if state.runner.is_running() {
        return Err("Feche o jogo em execução antes de recomeçar.".to_owned());
    }
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        state
            .saves
            .delete(&game)
            .map_err(|error| format!("Não foi possível apagar o progresso: {error}"))
    })
    .await
    .map_err(|error| format!("Falha interna: {error}"))?
}

/// Opções de jogo do console (ex.: continuar de onde parou).
#[tauri::command]
pub fn get_console_settings(
    console_id: String,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<ConsoleSettings> {
    playable_system(&console_id)?;
    Ok(lock(&state.settings).console(&console_id))
}

/// Salva as opções de jogo do console; nada muda se a gravação falhar.
#[tauri::command]
pub async fn set_console_settings(
    console_id: String,
    settings: ConsoleSettings,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<ConsoleSettings> {
    playable_system(&console_id)?;
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let mut current = lock(&state.settings);
        let mut updated = current.clone();
        updated.set_console(&console_id, settings);
        updated
            .save(&state.settings_path)
            .map_err(|error| format!("Não foi possível salvar as configurações: {error}"))?;
        *current = updated;
        Ok(settings)
    })
    .await
    .map_err(|error| format!("Falha interna: {error}"))?
}
