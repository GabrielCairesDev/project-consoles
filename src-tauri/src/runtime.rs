//! Execução de jogos em um processo separado.
//!
//! O próprio executável do launcher, chamado com `--play <rom>`, vira o
//! processo do jogo. Assim uma falha no núcleo de emulação não derruba a
//! interface, e a janela do jogo tem seu próprio laço de eventos.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use player::controls::Bindings;

use crate::library::Game;

/// Argumento que faz o executável abrir o jogo em vez do launcher.
pub const PLAY_FLAG: &str = "--play";
/// Argumento com o mapa de teclas do jogo (ver `player::controls`).
pub const KEYS_FLAG: &str = "--keys";
/// Argumento com o arquivo de estado salvo (continuar de onde parou).
pub const STATE_FLAG: &str = "--state";

/// Consoles com núcleo próprio capaz de rodar jogos.
const PLAYABLE_CONSOLES: &[&str] = &["master-system"];

/// Indica se o app consegue rodar jogos deste console.
pub fn is_playable(console_id: &str) -> bool {
    PLAYABLE_CONSOLES.contains(&console_id)
}

/// Motivo pelo qual um jogo não pôde ser iniciado.
#[derive(Debug)]
pub enum LaunchError {
    NotPlayable { console_id: String },
    RomMissing(PathBuf),
    AlreadyRunning,
    Spawn(io::Error),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPlayable { console_id } => {
                write!(f, "Ainda não é possível jogar títulos de {console_id}.")
            }
            Self::RomMissing(path) => {
                write!(f, "O arquivo do jogo não existe mais: {}", path.display())
            }
            Self::AlreadyRunning => f.write_str("Já há um jogo em execução."),
            Self::Spawn(error) => write!(f, "Não foi possível iniciar o jogo: {error}"),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            _ => None,
        }
    }
}

/// Monta o comando que roda a ROM indicada.
/// Como o jogo deve rodar.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub bindings: Bindings,
    /// Estado para continuar e onde gravar o progresso ao fechar.
    pub state_path: Option<PathBuf>,
}

type CommandBuilder = Box<dyn Fn(&Path, &LaunchOptions) -> io::Result<Command> + Send + Sync>;

/// Controla o processo do jogo em execução; só um por vez.
pub struct Runner {
    command: CommandBuilder,
    running: Arc<AtomicBool>,
}

impl Default for Runner {
    /// Roda `<executável atual> --play <rom> --keys <mapa> [--state <arquivo>]`.
    fn default() -> Self {
        Self::with_command(|rom, options| Ok(play_command(&std::env::current_exe()?, rom, options)))
    }
}

/// Linha de comando do processo do jogo (o próprio executável do launcher).
fn play_command(exe: &Path, rom: &Path, options: &LaunchOptions) -> Command {
    let mut command = Command::new(exe);
    command
        .arg(PLAY_FLAG)
        .arg(rom)
        .arg(KEYS_FLAG)
        .arg(options.bindings.to_arg());
    if let Some(state) = &options.state_path {
        command.arg(STATE_FLAG).arg(state);
    }
    command
}

impl Runner {
    /// Usa `command` para montar o processo de cada jogo (injetável em testes).
    pub fn with_command(
        command: impl Fn(&Path, &LaunchOptions) -> io::Result<Command> + Send + Sync + 'static,
    ) -> Self {
        Self {
            command: Box::new(command),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Indica se há um jogo em execução.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Inicia `game` com `options` e retorna sem esperar o fim do processo.
    ///
    /// Quando o processo termina, a vaga é liberada e `on_exit` recebe o
    /// código de saída (`None` se o processo foi encerrado por sinal ou se a
    /// espera falhou). A vaga é liberada antes do callback para que ele já
    /// possa iniciar outro jogo.
    pub fn launch(
        &self,
        game: &Game,
        options: &LaunchOptions,
        on_exit: impl FnOnce(Option<i32>) + Send + 'static,
    ) -> Result<(), LaunchError> {
        if !is_playable(&game.console_id) {
            return Err(LaunchError::NotPlayable {
                console_id: game.console_id.clone(),
            });
        }
        if !game.path.is_file() {
            return Err(LaunchError::RomMissing(game.path.clone()));
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(LaunchError::AlreadyRunning);
        }

        match self.spawn_and_watch(&game.path, options, on_exit) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.running.store(false, Ordering::Release);
                Err(LaunchError::Spawn(error))
            }
        }
    }

    fn spawn_and_watch(
        &self,
        rom: &Path,
        options: &LaunchOptions,
        on_exit: impl FnOnce(Option<i32>) + Send + 'static,
    ) -> io::Result<()> {
        let mut child = (self.command)(rom, options)?.spawn()?;
        let running = Arc::clone(&self.running);
        // O `Child` vai para a thread; se ela não puder ser criada, o
        // closure (com o `Child`) volta no erro e não temos como esperá-lo,
        // então o processo é encerrado aqui antes de liberar a vaga.
        let pid = child.id();
        let spawned = thread::Builder::new()
            .name("game-watcher".to_owned())
            .spawn(move || {
                let code = match child.wait() {
                    Ok(status) => status.code(),
                    Err(error) => {
                        eprintln!("falha ao esperar o processo do jogo: {error}");
                        None
                    }
                };
                running.store(false, Ordering::Release);
                on_exit(code);
            });
        if let Err(error) = spawned {
            eprintln!("thread de espera não criada; processo {pid} ficará órfão");
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn game(console_id: &str, path: PathBuf) -> Game {
        Game {
            id: "rom.sms".to_owned(),
            console_id: console_id.to_owned(),
            title: "Teste".to_owned(),
            file_name: "rom.sms".to_owned(),
            recognized: false,
            has_save: false,
            path,
            no_intro_name: None,
        }
    }

    /// Processo que sai com `code` depois de ~`seconds` segundos.
    fn fake_process(code: i32, seconds: u32) -> Runner {
        Runner::with_command(move |_rom, _options| {
            #[cfg(windows)]
            let command = {
                let mut command = Command::new("cmd");
                command.args([
                    "/C",
                    &format!("ping -n {} 127.0.0.1 >NUL & exit {code}", seconds + 1),
                ]);
                command
            };
            #[cfg(not(windows))]
            let command = {
                let mut command = Command::new("sh");
                command.args(["-c", &format!("sleep {seconds}; exit {code}")]);
                command
            };
            Ok(command)
        })
    }

    fn rom_file() -> tempfile::NamedTempFile {
        tempfile::NamedTempFile::new().expect("arquivo temporário")
    }

    #[test]
    fn play_command_passes_keys_and_optional_state() {
        let exe = Path::new("/app/project-consoles");
        let rom = Path::new("/Jogos/sonic.sms");
        let args = |options: &LaunchOptions| -> Vec<String> {
            play_command(exe, rom, options)
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };

        let keys = Bindings::default().to_arg();
        assert_eq!(
            args(&LaunchOptions::default()),
            vec![PLAY_FLAG, "/Jogos/sonic.sms", KEYS_FLAG, keys.as_str()]
        );
        let with_state = LaunchOptions {
            state_path: Some(PathBuf::from("/dados/sonic.state")),
            ..LaunchOptions::default()
        };
        assert_eq!(
            args(&with_state),
            vec![
                PLAY_FLAG,
                "/Jogos/sonic.sms",
                KEYS_FLAG,
                keys.as_str(),
                STATE_FLAG,
                "/dados/sonic.state"
            ]
        );
    }

    #[test]
    fn only_master_system_is_playable() {
        assert!(is_playable("master-system"));
        assert!(!is_playable("mega-drive"));
        assert!(!is_playable(""));
    }

    #[test]
    fn exit_callback_receives_code_and_frees_slot() {
        let rom = rom_file();
        let runner = fake_process(3, 0);
        let (tx, rx) = mpsc::channel();
        runner
            .launch(
                &game("master-system", rom.path().to_owned()),
                &LaunchOptions::default(),
                move |code| {
                    let _ = tx.send(code);
                },
            )
            .expect("lança o processo falso");

        assert_eq!(rx.recv_timeout(TIMEOUT), Ok(Some(3)));
        assert!(!runner.is_running());
    }

    #[test]
    fn refuses_second_game_while_one_runs() {
        let rom = rom_file();
        let runner = fake_process(0, 2);
        let target = game("master-system", rom.path().to_owned());
        let (tx, rx) = mpsc::channel();
        runner
            .launch(&target, &LaunchOptions::default(), move |code| {
                let _ = tx.send(code);
            })
            .expect("primeiro jogo inicia");

        assert!(runner.is_running());
        let second = runner.launch(&target, &LaunchOptions::default(), |_| {});
        assert!(matches!(second, Err(LaunchError::AlreadyRunning)));

        assert_eq!(rx.recv_timeout(TIMEOUT), Ok(Some(0)));
        let (tx, rx) = mpsc::channel();
        runner
            .launch(&target, &LaunchOptions::default(), move |code| {
                let _ = tx.send(code);
            })
            .expect("vaga liberada após o fim do primeiro");
        assert_eq!(rx.recv_timeout(TIMEOUT), Ok(Some(0)));
    }

    #[test]
    fn rejects_console_without_core() {
        let rom = rom_file();
        let runner = fake_process(0, 0);
        let result = runner.launch(
            &game("mega-drive", rom.path().to_owned()),
            &LaunchOptions::default(),
            |_| {},
        );
        assert!(matches!(
            result,
            Err(LaunchError::NotPlayable { console_id }) if console_id == "mega-drive"
        ));
        assert!(!runner.is_running());
    }

    #[test]
    fn rejects_missing_rom() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let missing = dir.path().join("sumiu.sms");
        let runner = fake_process(0, 0);
        let result = runner.launch(
            &game("master-system", missing.clone()),
            &LaunchOptions::default(),
            |_| {},
        );
        assert!(matches!(result, Err(LaunchError::RomMissing(path)) if path == missing));
        assert!(!runner.is_running());
    }

    #[test]
    fn spawn_failure_frees_slot() {
        let rom = rom_file();
        let runner = Runner::with_command(|_, _| {
            Ok(Command::new("/caminho/que/nao/existe/project-consoles"))
        });
        let target = game("master-system", rom.path().to_owned());
        let result = runner.launch(&target, &LaunchOptions::default(), |_| {});
        assert!(matches!(result, Err(LaunchError::Spawn(_))));
        assert!(!runner.is_running());
    }

    #[test]
    fn builder_error_is_reported_as_spawn() {
        let rom = rom_file();
        let runner = Runner::with_command(|_, _| Err(io::Error::other("sem executável")));
        let result = runner.launch(
            &game("master-system", rom.path().to_owned()),
            &LaunchOptions::default(),
            |_| {},
        );
        assert!(matches!(result, Err(LaunchError::Spawn(_))));
        assert!(!runner.is_running());
    }
}
