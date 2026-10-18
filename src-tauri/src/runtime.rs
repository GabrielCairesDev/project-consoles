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

use player::controls::{Bindings, System};
use player::image::ImageSettings;

use crate::library::Game;

/// Argumento que faz o executável abrir o jogo em vez do launcher.
pub const PLAY_FLAG: &str = "--play";
/// Argumento com o console da ROM (`master-system`, `mega-drive`).
pub const CONSOLE_FLAG: &str = "--console";
/// Argumento com o mapa de teclas do jogo (ver `player::controls`).
pub const KEYS_FLAG: &str = "--keys";
/// Argumento com o arquivo de estado salvo (continuar de onde parou).
pub const STATE_FLAG: &str = "--state";
/// Argumento com o arquivo do save do próprio jogo (RAM com bateria).
pub const BATTERY_FLAG: &str = "--sav";
/// Argumento com o BIOS do console (o PlayStation precisa dele).
pub const BIOS_FLAG: &str = "--bios";
/// Argumento com a ampliação do quadro ([`player::image::Scaler::id`]);
/// sem ele, vale a recomendada do console.
pub const SCALER_FLAG: &str = "--scaler";
/// Argumento com a paleta ([`player::image::Palette::id`]); sem ele, vale a
/// recomendada do console.
pub const PALETTE_FLAG: &str = "--palette";
/// Argumento com a proporção ([`player::image::AspectRatio::id`]).
pub const ASPECT_FLAG: &str = "--aspect";
/// Argumento com o encaixe na janela ([`player::image::Fit::id`]).
pub const FIT_FLAG: &str = "--fit";
/// Argumento com o efeito de tela ([`player::image::ScreenEffect::id`]).
pub const SCREEN_FLAG: &str = "--screen";
/// Argumento com o sinal de vídeo ([`player::image::Signal::id`]).
pub const SIGNAL_FLAG: &str = "--signal";
/// Argumento com o rastro do LCD ([`player::image::FrameBlend::id`]).
pub const BLEND_FLAG: &str = "--blend";
/// Argumento com a resolução interna ([`player::image::Resolution::id`]).
pub const RESOLUTION_FLAG: &str = "--resolution";
/// Argumento com a cor do desenho ([`player::image::ColorDepth::id`]).
pub const COLOR_FLAG: &str = "--color";
/// Argumento com a geometria ([`player::image::Geometry::id`]).
pub const GEOMETRY_FLAG: &str = "--geometry";
/// Argumento com a filtragem de textura ([`player::image::TextureFilter::id`]).
pub const TEXTURE_FLAG: &str = "--texture";

/// Núcleo que roda jogos deste console, se houver.
pub fn system_for(console_id: &str) -> Option<System> {
    System::from_id(console_id)
}

/// Indica se o app consegue rodar jogos deste console.
pub fn is_playable(console_id: &str) -> bool {
    system_for(console_id).is_some()
}

/// Motivo pelo qual um jogo não pôde ser iniciado.
#[derive(Debug)]
pub enum LaunchError {
    NotPlayable {
        console_id: String,
    },
    RomMissing(PathBuf),
    /// As teclas recebidas são de outro console (erro de quem chamou).
    ControlsMismatch,
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
            Self::ControlsMismatch => f.write_str("Os controles não são deste console."),
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

/// Como o jogo deve rodar.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Teclas do jogo; também definem o console ([`Bindings::system`]).
    pub bindings: Bindings,
    /// Estado para continuar e onde gravar o progresso ao fechar.
    pub state_path: Option<PathBuf>,
    /// Save do próprio jogo: carregado ao abrir e gravado ao fechar.
    pub battery_path: Option<PathBuf>,
    /// BIOS do console, para os que precisam dele.
    pub bios_path: Option<PathBuf>,
    /// Configuração de imagem com que o jogo roda.
    pub image: ImageSettings,
}

type CommandBuilder = Box<dyn Fn(&Path, &LaunchOptions) -> io::Result<Command> + Send + Sync>;

/// Controla o processo do jogo em execução; só um por vez.
pub struct Runner {
    command: CommandBuilder,
    running: Arc<AtomicBool>,
}

impl Default for Runner {
    /// Roda `<executável atual> --play <rom> --console <id> --keys <mapa>
    /// [--state <arquivo>]`.
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
        .arg(CONSOLE_FLAG)
        .arg(options.bindings.system().id())
        .arg(KEYS_FLAG)
        .arg(options.bindings.to_arg())
        .arg(SCALER_FLAG)
        .arg(options.image.scaler.id())
        .arg(PALETTE_FLAG)
        .arg(options.image.palette.id())
        .arg(ASPECT_FLAG)
        .arg(options.image.aspect.id())
        .arg(FIT_FLAG)
        .arg(options.image.fit.id())
        .arg(SCREEN_FLAG)
        .arg(options.image.screen.id())
        .arg(SIGNAL_FLAG)
        .arg(options.image.signal.id())
        .arg(BLEND_FLAG)
        .arg(options.image.blend.id())
        .arg(RESOLUTION_FLAG)
        .arg(options.image.resolution.id())
        .arg(COLOR_FLAG)
        .arg(options.image.color_depth.id())
        .arg(GEOMETRY_FLAG)
        .arg(options.image.geometry.id())
        .arg(TEXTURE_FLAG)
        .arg(options.image.texture.id());
    if let Some(state) = &options.state_path {
        command.arg(STATE_FLAG).arg(state);
    }
    if let Some(battery) = &options.battery_path {
        command.arg(BATTERY_FLAG).arg(battery);
    }
    if let Some(bios) = &options.bios_path {
        command.arg(BIOS_FLAG).arg(bios);
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
        let Some(system) = system_for(&game.console_id) else {
            return Err(LaunchError::NotPlayable {
                console_id: game.console_id.clone(),
            });
        };
        if options.bindings.system() != system {
            return Err(LaunchError::ControlsMismatch);
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
pub(crate) mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn options() -> LaunchOptions {
        LaunchOptions {
            bindings: Bindings::defaults(System::MasterSystem),
            state_path: None,
            battery_path: None,
            bios_path: None,
            image: ImageSettings::recommended(System::MasterSystem),
        }
    }

    fn game(console_id: &str, path: PathBuf) -> Game {
        Game {
            id: "rom.sms".to_owned(),
            console_id: console_id.to_owned(),
            title: "Teste".to_owned(),
            file_name: "rom.sms".to_owned(),
            recognized: false,
            has_save: false,
            has_game_save: false,
            path,
            no_intro_name: None,
        }
    }

    /// Processo que sai com `code` depois de ~`seconds` segundos.
    pub(crate) fn fake_process(code: i32, seconds: u32) -> Runner {
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

        let keys = Bindings::defaults(System::MasterSystem).to_arg();
        assert_eq!(
            args(&options()),
            vec![
                PLAY_FLAG,
                "/Jogos/sonic.sms",
                CONSOLE_FLAG,
                "master-system",
                KEYS_FLAG,
                keys.as_str(),
                SCALER_FLAG,
                "epx",
                PALETTE_FLAG,
                "core",
                ASPECT_FLAG,
                "square",
                FIT_FLAG,
                "integer",
                SCREEN_FLAG,
                "none",
                SIGNAL_FLAG,
                "rgb",
                BLEND_FLAG,
                "off",
                RESOLUTION_FLAG,
                "1x",
                COLOR_FLAG,
                "15-bit",
                GEOMETRY_FLAG,
                "native",
                TEXTURE_FLAG,
                "nearest"
            ]
        );
        let with_state = LaunchOptions {
            state_path: Some(PathBuf::from("/dados/sonic.state")),
            battery_path: Some(PathBuf::from("/dados/sonic.sav")),
            ..options()
        };
        assert_eq!(
            args(&with_state),
            vec![
                PLAY_FLAG,
                "/Jogos/sonic.sms",
                CONSOLE_FLAG,
                "master-system",
                KEYS_FLAG,
                keys.as_str(),
                SCALER_FLAG,
                "epx",
                PALETTE_FLAG,
                "core",
                ASPECT_FLAG,
                "square",
                FIT_FLAG,
                "integer",
                SCREEN_FLAG,
                "none",
                SIGNAL_FLAG,
                "rgb",
                BLEND_FLAG,
                "off",
                RESOLUTION_FLAG,
                "1x",
                COLOR_FLAG,
                "15-bit",
                GEOMETRY_FLAG,
                "native",
                TEXTURE_FLAG,
                "nearest",
                STATE_FLAG,
                "/dados/sonic.state",
                BATTERY_FLAG,
                "/dados/sonic.sav"
            ]
        );
        let with_bios = LaunchOptions {
            bindings: Bindings::defaults(System::PlayStation),
            bios_path: Some(PathBuf::from("/dados/bios/playstation.bin")),
            ..options()
        };
        let bios_args = args(&with_bios);
        assert_eq!(
            bios_args[bios_args.len() - 2..],
            [BIOS_FLAG, "/dados/bios/playstation.bin"]
        );

        // A ampliação vai sempre explícita, logo depois das teclas.
        let plain = LaunchOptions {
            image: ImageSettings::original(System::MasterSystem),
            ..options()
        };
        assert_eq!(
            args(&plain)[6..28],
            [
                SCALER_FLAG,
                "nearest",
                PALETTE_FLAG,
                "core",
                ASPECT_FLAG,
                "square",
                FIT_FLAG,
                "integer",
                SCREEN_FLAG,
                "none",
                SIGNAL_FLAG,
                "rgb",
                BLEND_FLAG,
                "off",
                RESOLUTION_FLAG,
                "1x",
                COLOR_FLAG,
                "15-bit",
                GEOMETRY_FLAG,
                "native",
                TEXTURE_FLAG,
                "nearest"
            ]
        );
    }

    #[test]
    fn consoles_with_core_are_playable() {
        assert_eq!(system_for("master-system"), Some(System::MasterSystem));
        assert_eq!(system_for("mega-drive"), Some(System::Genesis));
        assert_eq!(system_for("game-boy"), Some(System::GameBoy));
        assert_eq!(system_for("playstation"), Some(System::PlayStation));
        assert!(!is_playable("game-boy-color"));
        assert!(!is_playable("snes"));
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
                &options(),
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
            .launch(&target, &options(), move |code| {
                let _ = tx.send(code);
            })
            .expect("primeiro jogo inicia");

        assert!(runner.is_running());
        let second = runner.launch(&target, &options(), |_| {});
        assert!(matches!(second, Err(LaunchError::AlreadyRunning)));

        assert_eq!(rx.recv_timeout(TIMEOUT), Ok(Some(0)));
        let (tx, rx) = mpsc::channel();
        runner
            .launch(&target, &options(), move |code| {
                let _ = tx.send(code);
            })
            .expect("vaga liberada após o fim do primeiro");
        assert_eq!(rx.recv_timeout(TIMEOUT), Ok(Some(0)));
    }

    #[test]
    fn rejects_console_without_core() {
        let rom = rom_file();
        let runner = fake_process(0, 0);
        let result = runner.launch(&game("snes", rom.path().to_owned()), &options(), |_| {});
        assert!(matches!(
            result,
            Err(LaunchError::NotPlayable { console_id }) if console_id == "snes"
        ));
        assert!(!runner.is_running());
    }

    #[test]
    fn rejects_controls_of_another_console() {
        let rom = rom_file();
        let runner = fake_process(0, 0);
        let result = runner.launch(
            &game("mega-drive", rom.path().to_owned()),
            &options(),
            |_| {},
        );
        assert!(matches!(result, Err(LaunchError::ControlsMismatch)));
        assert!(!runner.is_running());
    }

    #[test]
    fn rejects_missing_rom() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let missing = dir.path().join("sumiu.sms");
        let runner = fake_process(0, 0);
        let result = runner.launch(&game("master-system", missing.clone()), &options(), |_| {});
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
        let result = runner.launch(&target, &options(), |_| {});
        assert!(matches!(result, Err(LaunchError::Spawn(_))));
        assert!(!runner.is_running());
    }

    #[test]
    fn builder_error_is_reported_as_spawn() {
        let rom = rom_file();
        let runner = Runner::with_command(|_, _| Err(io::Error::other("sem executável")));
        let result = runner.launch(
            &game("master-system", rom.path().to_owned()),
            &options(),
            |_| {},
        );
        assert!(matches!(result, Err(LaunchError::Spawn(_))));
        assert!(!runner.is_running());
    }
}
