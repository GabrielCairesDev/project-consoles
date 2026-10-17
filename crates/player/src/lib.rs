//! Janela nativa que executa um jogo (Master System, Mega Drive, Game Boy
//! ou PlayStation).
//!
//! Roda num processo próprio, iniciado pelo launcher: o event loop do winit
//! precisa da thread principal (no macOS só pode existir um por processo) e
//! um travamento aqui não derruba a biblioteca.
//!
//! Controles: configuráveis por [`controls::Bindings`], com padrões por
//! console. Esc sempre fecha o jogo.

mod audio;
pub mod controls;
mod enhance;
pub mod image;
mod machine;
mod mmpx;
mod resample;
mod ring;
mod rom;
mod video;

use std::fmt;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use audio::AudioOut;
use controls::Bindings;
use image::{ImageError, ImageSettings};
use machine::{Machine, MachineError};
use rom::RomReadError;

/// Taxa usada quando não há áudio: os chips precisam de alguma taxa de saída.
const FALLBACK_SAMPLE_RATE: u32 = 44_100;
/// Escala inicial da janela.
const INITIAL_SCALE: u32 = 3;
/// Atraso máximo recuperado de uma vez; além disso o relógio é reiniciado
/// (evita a "espiral" de quadros depois de a janela ficar travada).
const MAX_CATCH_UP_FRAMES: u32 = 3;

#[derive(Debug)]
pub enum PlayerError {
    ReadRom(RomReadError),
    Rom(MachineError),
    /// O PlayStation precisa do BIOS do usuário.
    MissingBios,
    ReadBios(std::io::Error),
    Disc(psx::DiscError),
    EventLoop(winit::error::EventLoopError),
    Window(winit::error::OsError),
    Surface(softbuffer::SoftBufferError),
    SaveState(std::io::Error),
    /// Configuração de imagem com uma escolha que o console não tem.
    Image(ImageError),
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadRom(e) => write!(f, "não foi possível ler a ROM: {e}"),
            Self::Rom(e) => write!(f, "ROM inválida: {e}"),
            Self::MissingBios => f.write_str("falta o BIOS do PlayStation"),
            Self::ReadBios(e) => write!(f, "não foi possível ler o BIOS: {e}"),
            Self::Disc(e) => write!(f, "não foi possível abrir o disco: {e}"),
            Self::EventLoop(e) => write!(f, "falha no loop de eventos: {e}"),
            Self::Window(e) => write!(f, "não foi possível abrir a janela: {e}"),
            Self::Surface(e) => write!(f, "falha ao desenhar na janela: {e}"),
            Self::SaveState(e) => write!(f, "não foi possível salvar o progresso: {e}"),
            Self::Image(e) => write!(f, "configuração de imagem inválida: {e}"),
        }
    }
}

impl std::error::Error for PlayerError {}

/// Por que um save do jogo não serve para um cartucho.
#[derive(Debug)]
pub enum BatterySaveError {
    ReadRom(RomReadError),
    Rom(MachineError),
    /// O cartucho não tem RAM com bateria.
    NoBattery,
    /// O arquivo não é um memory card do PlayStation.
    MemoryCard(psx::MemoryCardError),
    /// O arquivo não tem o tamanho da RAM do cartucho (`expected`, quando
    /// o cartucho declara um só).
    WrongSize {
        size: usize,
        expected: Option<usize>,
    },
}

impl fmt::Display for BatterySaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadRom(e) => write!(f, "não foi possível ler a ROM: {e}"),
            Self::Rom(e) => write!(f, "ROM inválida: {e}"),
            Self::NoBattery => f.write_str("este jogo não salva no cartucho"),
            Self::MemoryCard(e) => e.fmt(f),
            Self::WrongSize {
                size,
                expected: Some(expected),
            } => write!(
                f,
                "o arquivo tem {size} bytes, mas o save deste jogo tem {expected}"
            ),
            Self::WrongSize { size, .. } => {
                write!(
                    f,
                    "o arquivo tem {size} bytes, tamanho que este jogo não usa"
                )
            }
        }
    }
}

impl std::error::Error for BatterySaveError {}

/// Confere se `bytes` serve como save do jogo da ROM em `rom_path`, com a
/// mesma regra que o jogo usa ao carregar o save. Liga o núcleo só para
/// ler o cartucho; não executa nenhum quadro. No PlayStation o save é o
/// memory card, igual para qualquer disco.
pub fn check_battery_save(
    rom_path: &Path,
    system: controls::System,
    bytes: &[u8],
) -> Result<(), BatterySaveError> {
    if system == controls::System::PlayStation {
        return psx::check_memory_card(bytes)
            .map(|_| ())
            .map_err(BatterySaveError::MemoryCard);
    }
    let rom = rom::read(rom_path, system).map_err(BatterySaveError::ReadRom)?;
    let mut machine =
        Machine::new(system, rom, FALLBACK_SAMPLE_RATE).map_err(BatterySaveError::Rom)?;
    if !machine.has_battery() {
        return Err(BatterySaveError::NoBattery);
    }
    let expected = machine.battery_ram().map(<[u8]>::len);
    if machine.load_battery_ram(bytes) {
        Ok(())
    } else {
        Err(BatterySaveError::WrongSize {
            size: bytes.len(),
            expected,
        })
    }
}

/// BIOS recusado pelo console.
pub use psx::BiosError;

/// Confere se `bytes` é um BIOS que o console aceita (hoje só o
/// PlayStation usa BIOS).
pub fn check_bios(system: controls::System, bytes: &[u8]) -> Result<(), BiosError> {
    match system {
        controls::System::PlayStation => psx::check_bios(bytes),
        _ => Ok(()),
    }
}

/// Abre a janela e executa o jogo até o usuário fechá-la. O console é o
/// de `bindings` ([`Bindings::system`]); a ROM pode estar num `.zip`.
///
/// Com `state_path`, o jogo continua do estado salvo nesse arquivo (se
/// existir e for desta ROM) e o estado é gravado nele ao fechar.
///
/// Com `battery_path`, a RAM com bateria do cartucho (o save do próprio
/// jogo; no PlayStation, o memory card) é carregada desse arquivo e
/// gravada nele ao fechar, mesmo sem `state_path`. O estado salvo, quando
/// existe, vem depois e prevalece.
///
/// O PlayStation lê o disco (`.cue` ou `.bin`) em `rom_path` e precisa do
/// BIOS em `bios_path`; ele ainda não salva estado, então `state_path` é
/// ignorado.
///
/// `image` é a configuração de imagem do usuário; uma escolha que não
/// existe no console é recusada antes de abrir o jogo.
pub fn run(
    rom_path: &Path,
    bindings: Bindings,
    state_path: Option<PathBuf>,
    battery_path: Option<PathBuf>,
    bios_path: Option<PathBuf>,
    image: ImageSettings,
) -> Result<(), PlayerError> {
    let system = bindings.system();
    let image = image.check(system).map_err(PlayerError::Image)?;
    let audio = match AudioOut::open() {
        Ok(audio) => Some(audio),
        Err(reason) => {
            eprintln!("sem áudio: {reason}");
            None
        }
    };
    let sample_rate = audio
        .as_ref()
        .map_or(FALLBACK_SAMPLE_RATE, |a| a.sample_rate);
    let mut machine = load_machine(rom_path, system, bios_path.as_deref(), sample_rate)?;
    let state_path = state_path.filter(|_| machine.supports_state());
    // Sem RAM com bateria não há o que gravar; com um arquivo que não
    // serve (outro tamanho), ele é preservado em vez de sobrescrito.
    let battery_path = battery_path
        .filter(|_| machine.has_battery())
        .filter(|path| load_battery(&mut machine, path));
    if let Some(path) = &state_path {
        resume(&mut machine, path);
    }
    let title = rom_path
        .file_stem()
        .map_or_else(|| "Jogo".to_owned(), |s| s.to_string_lossy().into_owned());
    let frame_duration = Duration::from_secs_f64(1.0 / machine.frame_rate());

    let event_loop = EventLoop::new().map_err(PlayerError::EventLoop)?;
    let context = softbuffer::Context::new(event_loop.owned_display_handle())
        .map_err(PlayerError::Surface)?;
    let mut app = App {
        title,
        machine,
        audio,
        audio_scratch: Vec::new(),
        mono_scratch: Vec::new(),
        context,
        window: None,
        renderer: video::Renderer::new(image),
        bindings,
        state_path,
        battery_path,
        frame_duration,
        next_frame: Instant::now(),
        error: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(PlayerError::EventLoop)?;
    app.error.map_or(Ok(()), Err)
}

/// Monta o núcleo: a ROM do cartucho, ou o BIOS e o disco do PlayStation.
fn load_machine(
    path: &Path,
    system: controls::System,
    bios_path: Option<&Path>,
    sample_rate: u32,
) -> Result<Machine, PlayerError> {
    if system == controls::System::PlayStation {
        let bios_path = bios_path.ok_or(PlayerError::MissingBios)?;
        let bios = std::fs::read(bios_path).map_err(PlayerError::ReadBios)?;
        let disc = psx::Disc::open(path).map_err(PlayerError::Disc)?;
        return Machine::playstation(&bios, disc, sample_rate).map_err(PlayerError::Rom);
    }
    let rom = rom::read(path, system).map_err(PlayerError::ReadRom)?;
    Machine::new(system, rom, sample_rate).map_err(PlayerError::Rom)
}

struct GameWindow {
    window: Rc<Window>,
    surface: softbuffer::Surface<OwnedDisplayHandle, Rc<Window>>,
}

struct App {
    title: String,
    machine: Machine,
    audio: Option<AudioOut>,
    audio_scratch: Vec<f32>,
    mono_scratch: Vec<f32>,
    context: softbuffer::Context<OwnedDisplayHandle>,
    window: Option<GameWindow>,
    renderer: video::Renderer,
    bindings: Bindings,
    state_path: Option<PathBuf>,
    battery_path: Option<PathBuf>,
    frame_duration: Duration,
    next_frame: Instant,
    error: Option<PlayerError>,
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: PlayerError) {
        self.error = Some(error);
        event_loop.exit();
    }

    fn create_window(&self, event_loop: &ActiveEventLoop) -> Result<GameWindow, PlayerError> {
        // Mesmo tamanho para todos os consoles de cartucho: o quadro é
        // centralizado. O PlayStation parte de 320×240.
        let (width, height) = self.machine.base_frame();
        let (width, height) = (width as u32, height as u32);
        let size = LogicalSize::new(width * INITIAL_SCALE, height * INITIAL_SCALE);
        let attributes = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(size)
            .with_min_inner_size(LogicalSize::new(width, height));
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(PlayerError::Window)?,
        );
        let surface = softbuffer::Surface::new(&self.context, window.clone())
            .map_err(PlayerError::Surface)?;
        Ok(GameWindow { window, surface })
    }

    fn run_frame(&mut self) {
        self.machine.run_frame();
        // O PlayStation troca entre NTSC e PAL conforme o jogo.
        self.frame_duration = Duration::from_secs_f64(1.0 / self.machine.frame_rate());
        self.audio_scratch.clear();
        self.machine
            .drain_audio(&mut self.audio_scratch, &mut self.mono_scratch);
        if let Some(audio) = &mut self.audio {
            audio.push(&self.audio_scratch);
        }
    }

    fn redraw(&mut self) -> Result<(), PlayerError> {
        let Some(game) = &mut self.window else {
            return Ok(());
        };
        let size = game.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            // Janela minimizada: nada a desenhar.
            return Ok(());
        };
        game.surface
            .resize(width, height)
            .map_err(PlayerError::Surface)?;
        let mut buffer = game.surface.buffer_mut().map_err(PlayerError::Surface)?;
        let frame = self.machine.frame();
        self.renderer.draw(
            frame.pixels,
            frame.stride,
            frame.size,
            frame.aspect,
            &mut buffer,
            (width.get() as usize, height.get() as usize),
        );
        buffer.present().map_err(PlayerError::Surface)
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        let pressed = event.state == ElementState::Pressed;
        match code {
            KeyCode::Escape if pressed => event_loop.exit(),
            // Repetição do teclado não aperta de novo (o Pause é uma borda).
            _ if event.repeat => {}
            _ => {
                if let Some(action) = self.bindings.action_for(code) {
                    self.machine.set_action(action, pressed);
                }
            }
        }
    }
}

/// Carrega o estado salvo, se houver. Estado inválido (outra versão,
/// corrompido) não impede o jogo: começa do início e será sobrescrito.
fn resume(machine: &mut Machine, path: &Path) {
    match std::fs::read(path) {
        Ok(bytes) => {
            if let Err(e) = machine.load_state(&bytes) {
                eprintln!("estado salvo ignorado ({}): {e}", path.display());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("não foi possível ler o estado salvo: {e}"),
    }
}

/// Carrega o save do jogo. Devolve se o arquivo pode ser sobrescrito ao
/// fechar: sim se não existe ou foi aceito; não se é de outro tamanho ou
/// não pôde ser lido (fica intacto para o usuário).
fn load_battery(machine: &mut Machine, path: &Path) -> bool {
    match std::fs::read(path) {
        Ok(bytes) => {
            let accepted = machine.load_battery_ram(&bytes);
            if !accepted {
                eprintln!(
                    "save do jogo ignorado e preservado ({}): tamanho {} não confere",
                    path.display(),
                    bytes.len()
                );
            }
            accepted
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            eprintln!("não foi possível ler o save do jogo: {e}");
            false
        }
    }
}

/// Grava o save do jogo, se o cartucho tiver o que guardar (um jogo de
/// Master System que nunca usou a RAM do cartucho não gera arquivo).
fn save_battery(machine: &Machine, path: &Path) -> std::io::Result<()> {
    match machine.battery_ram() {
        Some(ram) => write_state(path, ram),
        None => Ok(()),
    }
}

/// Grava de forma atômica (arquivo temporário + rename), para não deixar
/// um arquivo pela metade se o processo morrer durante a escrita.
fn write_state(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

impl ApplicationHandler for App {
    /// Fim do loop (janela fechada ou Esc): salva o progresso e o save do
    /// jogo.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(path) = &self.battery_path
            && let Err(e) = save_battery(&self.machine, path)
        {
            self.error.get_or_insert(PlayerError::SaveState(e));
        }
        if let Some(path) = &self.state_path
            && let Some(state) = self.machine.save_state()
            && let Err(e) = write_state(path, &state)
        {
            self.error.get_or_insert(PlayerError::SaveState(e));
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        match self.create_window(event_loop) {
            Ok(game) => {
                self.window = Some(game);
                self.next_frame = Instant::now();
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
            }
            Err(e) => self.fail(event_loop, e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(event_loop, e);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event_loop, &event),
            // Sem foco, nenhuma tecla fica "presa".
            WindowEvent::Focused(false) => self.machine.release_all(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(game) = &self.window else {
            return;
        };
        let now = Instant::now();
        if now < self.next_frame {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
            return;
        }
        let window = game.window.clone();
        let mut frames = 0;
        while self.next_frame <= now && frames < MAX_CATCH_UP_FRAMES {
            self.run_frame();
            self.next_frame += self.frame_duration;
            frames += 1;
        }
        if self.next_frame <= now {
            self.next_frame = now + self.frame_duration;
        }
        window.request_redraw();
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use controls::System;

    fn sms_machine(rom: Vec<u8>) -> Machine {
        Machine::new(System::MasterSystem, rom, 44_100).expect("ROM válida")
    }

    /// Game Boy com MBC1 + 8 KB de RAM com bateria, preso num laço.
    fn game_boy_machine() -> Machine {
        let mut rom = vec![0; 0x8000];
        rom[0x100..0x102].copy_from_slice(&[0x18, 0xFE]); // JR -2
        rom[0x147] = 0x03;
        rom[0x149] = 0x02;
        Machine::new(System::GameBoy, rom, 44_100).expect("ROM válida")
    }

    #[test]
    fn battery_save_loads_and_wrong_size_is_preserved() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("jogo.sav");

        let mut machine = game_boy_machine();
        assert!(load_battery(&mut machine, &path), "sem arquivo: pode criar");
        std::fs::write(&path, vec![0x77; 0x2000]).expect("escrita");
        let mut machine = game_boy_machine();
        assert!(load_battery(&mut machine, &path));
        assert_eq!(machine.battery_ram(), Some(&[0x77; 0x2000][..]));

        std::fs::write(&path, [1, 2, 3]).expect("escrita");
        let mut machine = game_boy_machine();
        assert!(
            !load_battery(&mut machine, &path),
            "tamanho errado: preserva"
        );
        assert_eq!(std::fs::read(&path).expect("leitura"), [1, 2, 3]);
    }

    /// Mega Drive com SRAM declarada no cabeçalho ("RA" em 0x1B0).
    fn genesis_rom() -> Vec<u8> {
        let mut rom = vec![0; 0x400];
        rom[0x1B0..0x1B2].copy_from_slice(b"RA");
        rom
    }

    /// Master System que liga a RAM do cartucho, grava 0x42 e trava.
    const SMS_WRITES_CART_RAM: &[u8] = &[
        0x3E, 0x08, // LD A,08h
        0x32, 0xFC, 0xFF, // LD (FFFCh),A — liga a RAM do cartucho
        0x3E, 0x42, // LD A,42h
        0x32, 0x00, 0x80, // LD (8000h),A
        0x18, 0xFE, // JR $
    ];

    /// Console, como montar a máquina e o tamanho do `.sav`.
    type BatteryCase = (&'static str, fn() -> Machine, usize);

    /// Cada console com RAM de save.
    fn machines_with_battery() -> Vec<BatteryCase> {
        vec![
            ("game-boy", game_boy_machine, 0x2000),
            (
                "mega-drive",
                || Machine::new(System::Genesis, genesis_rom(), 44_100).expect("ROM"),
                0x1_0000,
            ),
            ("master-system", || sms_machine(vec![0x18, 0xFE]), 0x8000),
        ]
    }

    #[test]
    fn battery_save_round_trips_on_every_console() {
        for (console, machine, size) in machines_with_battery() {
            let dir = tempfile::tempdir().expect("pasta temporária");
            let path = dir.path().join("saves").join(console).join("jogo.sav");
            let saved: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            write_state(&path, &saved).expect("grava");

            let mut running = machine();
            assert!(running.has_battery(), "{console}");
            assert!(load_battery(&mut running, &path), "{console}");
            assert_eq!(running.battery_ram(), Some(&saved[..]), "{console}");
            running.run_frame();
            save_battery(&running, &path).expect("grava ao fechar");
            assert_eq!(std::fs::read(&path).expect("leitura"), saved, "{console}");
        }
    }

    #[test]
    fn battery_save_of_wrong_size_is_preserved_on_every_console() {
        for (console, machine, size) in machines_with_battery() {
            let dir = tempfile::tempdir().expect("pasta temporária");
            let path = dir.path().join("jogo.sav");
            let foreign = vec![0x33; size + 1];
            std::fs::write(&path, &foreign).expect("escrita");

            let mut running = machine();
            assert!(!load_battery(&mut running, &path), "{console}");
            // O player não grava num save que recusou (`battery_path` vira
            // `None`); o arquivo continua o mesmo.
            assert_eq!(std::fs::read(&path).expect("leitura"), foreign, "{console}");
        }
    }

    #[test]
    fn master_system_game_that_never_uses_cartridge_ram_has_no_save() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("jogo.sav");
        let mut machine = sms_machine(vec![0x18, 0xFE]);
        assert!(load_battery(&mut machine, &path));
        for _ in 0..3 {
            machine.run_frame();
        }
        save_battery(&machine, &path).expect("fechar");
        assert!(!path.exists(), "nada a guardar: nenhum .sav");
    }

    #[test]
    fn master_system_game_that_uses_cartridge_ram_gets_a_save() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("jogo.sav");
        let mut machine = sms_machine(SMS_WRITES_CART_RAM.to_vec());
        assert!(load_battery(&mut machine, &path));
        machine.run_frame();
        save_battery(&machine, &path).expect("fechar");
        let saved = std::fs::read(&path).expect("save criado");
        assert_eq!(saved.len(), 0x8000, "32 KB, como a RAM do cartucho");
        assert_eq!(saved[0], 0x42);

        // E volta: outro jogo do mesmo cartucho lê o que foi gravado.
        let mut next = sms_machine(SMS_WRITES_CART_RAM.to_vec());
        assert!(load_battery(&mut next, &path));
        assert_eq!(next.battery_ram().map(|r| r[0]), Some(0x42));
    }

    #[test]
    fn master_system_accepts_saves_of_8_16_and_32_kb() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("jogo.sav");
        for size in [0x2000, 0x4000, 0x8000] {
            std::fs::write(&path, vec![0x5A; size]).expect("escrita");
            let mut machine = sms_machine(vec![0x18, 0xFE]);
            assert!(load_battery(&mut machine, &path), "{size}");
            let ram = machine.battery_ram().expect("carregada");
            assert_eq!(ram.len(), 0x8000, "{size}");
            assert!(ram[..size].iter().all(|&b| b == 0x5A), "{size}");
            assert!(ram[size..].iter().all(|&b| b == 0), "{size}");
        }
        for size in [0, 0x1000, 0x6000, 0x1_0000] {
            std::fs::write(&path, vec![0x5A; size]).expect("escrita");
            let mut machine = sms_machine(vec![0x18, 0xFE]);
            assert!(!load_battery(&mut machine, &path), "{size}");
            assert_eq!(std::fs::read(&path).expect("leitura").len(), size);
        }
    }

    #[test]
    fn check_battery_save_follows_the_cartridge() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let mut rom = vec![0; 0x8000];
        rom[0x147] = 0x03; // MBC1 + RAM + bateria
        rom[0x149] = 0x02; // 8 KB
        let gb = dir.path().join("jogo.gb");
        std::fs::write(&gb, &rom).expect("escrita");

        assert!(check_battery_save(&gb, System::GameBoy, &[0; 0x2000]).is_ok());
        assert!(matches!(
            check_battery_save(&gb, System::GameBoy, &[0; 0x8000]),
            Err(BatterySaveError::WrongSize {
                size: 0x8000,
                expected: Some(0x2000)
            })
        ));

        rom[0x147] = 0x01; // MBC1 sem RAM
        std::fs::write(&gb, &rom).expect("escrita");
        assert!(matches!(
            check_battery_save(&gb, System::GameBoy, &[0; 0x2000]),
            Err(BatterySaveError::NoBattery)
        ));

        let sms = dir.path().join("jogo.sms");
        std::fs::write(&sms, vec![0; 0x4000]).expect("escrita");
        assert!(check_battery_save(&sms, System::MasterSystem, &[0; 0x8000]).is_ok());
        assert!(check_battery_save(&sms, System::MasterSystem, &[0; 100]).is_err());

        let md = dir.path().join("jogo.md");
        std::fs::write(&md, genesis_rom()).expect("escrita");
        assert!(check_battery_save(&md, System::Genesis, &[0; 0x1_0000]).is_ok());
        assert!(matches!(
            check_battery_save(&md, System::Genesis, &[0; 0x8000]),
            Err(BatterySaveError::WrongSize {
                expected: Some(0x1_0000),
                ..
            })
        ));
        std::fs::write(&md, vec![0; 0x400]).expect("escrita");
        assert!(matches!(
            check_battery_save(&md, System::Genesis, &[0; 0x1_0000]),
            Err(BatterySaveError::NoBattery)
        ));

        let missing = dir.path().join("nada.gb");
        assert!(matches!(
            check_battery_save(&missing, System::GameBoy, &[]),
            Err(BatterySaveError::ReadRom(_))
        ));
    }

    #[test]
    fn master_system_saves_only_after_the_game_uses_cartridge_ram() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("jogo.sav");
        let mut machine = sms_machine(vec![0x18, 0xFE]);
        assert!(machine.has_battery());
        assert!(load_battery(&mut machine, &path), "sem arquivo: pode criar");
        assert!(machine.battery_ram().is_none(), "nada a gravar ainda");

        std::fs::write(&path, vec![0x42; 0x8000]).expect("escrita");
        let mut machine = sms_machine(vec![0x18, 0xFE]);
        assert!(load_battery(&mut machine, &path));
        assert_eq!(machine.battery_ram(), Some(&[0x42; 0x8000][..]));
    }

    #[test]
    fn state_file_round_trips_and_resumes() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("saves/jogo.state");
        let rom = vec![0x18, 0xFE]; // JR $
        let mut console = sms_machine(rom.clone());
        console.run_frame();
        write_state(&path, &console.save_state().expect("estado")).expect("grava");
        assert!(!path.with_extension("state.tmp").exists());

        let mut resumed = sms_machine(rom);
        resume(&mut resumed, &path);
        assert_eq!(resumed.save_state(), console.save_state());
    }

    #[test]
    fn invalid_or_missing_state_starts_fresh() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let fresh = sms_machine(vec![0x18, 0xFE]);

        let mut console = sms_machine(vec![0x18, 0xFE]);
        resume(&mut console, &dir.path().join("nao-existe.state"));
        assert_eq!(console.save_state(), fresh.save_state());

        let corrupted = dir.path().join("ruim.state");
        std::fs::write(&corrupted, b"lixo").expect("escrita");
        resume(&mut console, &corrupted);
        assert_eq!(console.save_state(), fresh.save_state());
    }

    /// PlayStation com um BIOS que só fica num laço (`j 0xBFC00000`) e um
    /// disco de um setor; o memory card começa formatado e vazio.
    fn playstation_machine(dir: &Path) -> Machine {
        let mut bios = vec![0; 512 * 1024];
        bios[..4].copy_from_slice(&0x0BF0_0000u32.to_le_bytes());
        let disc = dir.join("jogo.bin");
        std::fs::write(&disc, vec![0; 2352]).expect("escrita");
        let disc = psx::Disc::open(&disc).expect("disco");
        Machine::playstation(&bios, disc, 48_000).expect("BIOS do tamanho certo")
    }

    fn memory_card(machine: &Machine) -> Vec<u8> {
        match machine {
            Machine::PlayStation { console, .. } => console.memory_card().to_vec(),
            _ => unreachable!("só PlayStation"),
        }
    }

    #[test]
    fn playstation_memory_card_is_the_game_save() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("saves/playstation/jogo.mcr");
        let mut machine = playstation_machine(dir.path());
        assert!(machine.has_battery());
        assert!(!machine.supports_state(), "ainda sem estado salvo");
        assert!(load_battery(&mut machine, &path), "sem arquivo: pode criar");
        machine.run_frame();
        save_battery(&machine, &path).expect("fechar");
        assert!(
            !path.exists(),
            "o jogo não gravou no cartão: nada a guardar"
        );

        // Um cartão salvo volta inteiro.
        let mut card = memory_card(&machine);
        card[0x2000..0x2010].fill(0x5A);
        write_state(&path, &card).expect("grava");
        let mut machine = playstation_machine(dir.path());
        assert!(load_battery(&mut machine, &path));
        assert_eq!(memory_card(&machine), card);
        save_battery(&machine, &path).expect("fechar");
        assert_eq!(std::fs::read(&path).expect("leitura"), card, "intacto");
    }

    #[test]
    fn playstation_refuses_foreign_memory_cards_and_keeps_them() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("jogo.mcr");
        let mut machine = playstation_machine(dir.path());
        let card = memory_card(&machine);
        for foreign in [vec![0x33; card.len() + 1], vec![0; card.len()]] {
            std::fs::write(&path, &foreign).expect("escrita");
            assert!(!load_battery(&mut machine, &path));
            assert_eq!(std::fs::read(&path).expect("leitura"), foreign);
        }

        let disc = dir.path().join("jogo.cue");
        let check = |bytes: &[u8]| check_battery_save(&disc, System::PlayStation, bytes);
        assert!(check(&card).is_ok(), "não precisa ler o disco");
        assert!(matches!(
            check(&card[..1000]),
            Err(BatterySaveError::MemoryCard(
                psx::MemoryCardError::WrongSize(1000)
            ))
        ));
        assert!(matches!(
            check(&vec![0; card.len()]),
            Err(BatterySaveError::MemoryCard(
                psx::MemoryCardError::NotFormatted
            ))
        ));
    }

    #[test]
    fn playstation_needs_a_bios_of_the_right_size() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let disc = dir.path().join("jogo.bin");
        std::fs::write(&disc, vec![0; 2352]).expect("escrita");
        assert!(matches!(
            load_machine(&disc, System::PlayStation, None, 48_000),
            Err(PlayerError::MissingBios)
        ));
        let bios = dir.path().join("bios.bin");
        std::fs::write(&bios, [0; 100]).expect("escrita");
        assert!(matches!(
            load_machine(&disc, System::PlayStation, Some(&bios), 48_000),
            Err(PlayerError::Rom(MachineError::Bios(_)))
        ));
        assert!(check_bios(System::PlayStation, &[0; 100]).is_err());
        assert!(check_bios(System::PlayStation, &[0; 512 * 1024]).is_ok());
        assert!(check_bios(System::GameBoy, &[]).is_ok(), "não usa BIOS");
    }
}
