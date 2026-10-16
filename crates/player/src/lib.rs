//! Janela nativa que executa um jogo (Master System ou Mega Drive).
//!
//! Roda num processo próprio, iniciado pelo launcher: o event loop do winit
//! precisa da thread principal (no macOS só pode existir um por processo) e
//! um travamento aqui não derruba a biblioteca.
//!
//! Controles: configuráveis por [`controls::Bindings`], com padrões por
//! console. Esc sempre fecha o jogo.

mod audio;
pub mod controls;
mod machine;
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
use machine::{MAX_FRAME, Machine, MachineError};
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
    EventLoop(winit::error::EventLoopError),
    Window(winit::error::OsError),
    Surface(softbuffer::SoftBufferError),
    SaveState(std::io::Error),
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadRom(e) => write!(f, "não foi possível ler a ROM: {e}"),
            Self::Rom(e) => write!(f, "ROM inválida: {e}"),
            Self::EventLoop(e) => write!(f, "falha no loop de eventos: {e}"),
            Self::Window(e) => write!(f, "não foi possível abrir a janela: {e}"),
            Self::Surface(e) => write!(f, "falha ao desenhar na janela: {e}"),
            Self::SaveState(e) => write!(f, "não foi possível salvar o progresso: {e}"),
        }
    }
}

impl std::error::Error for PlayerError {}

/// Abre a janela e executa o jogo até o usuário fechá-la. O console é o
/// de `bindings` ([`Bindings::system`]); a ROM pode estar num `.zip`.
///
/// Com `state_path`, o jogo continua do estado salvo nesse arquivo (se
/// existir e for desta ROM) e o estado é gravado nele ao fechar.
pub fn run(
    rom_path: &Path,
    bindings: Bindings,
    state_path: Option<PathBuf>,
) -> Result<(), PlayerError> {
    let system = bindings.system();
    let rom = rom::read(rom_path, system).map_err(PlayerError::ReadRom)?;
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
    let mut machine = Machine::new(system, rom, sample_rate).map_err(PlayerError::Rom)?;
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
        bindings,
        state_path,
        frame_duration,
        next_frame: Instant::now(),
        error: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(PlayerError::EventLoop)?;
    app.error.map_or(Ok(()), Err)
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
    bindings: Bindings,
    state_path: Option<PathBuf>,
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
        // Mesmo tamanho para todos os consoles: o quadro é centralizado.
        let (width, height) = (MAX_FRAME.0 as u32, MAX_FRAME.1 as u32);
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
        video::blit(
            frame.pixels,
            frame.stride,
            frame.size,
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

/// Grava o estado de forma atômica (arquivo temporário + rename), para não
/// deixar um estado pela metade se o processo morrer durante a escrita.
fn write_state(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("state.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

impl ApplicationHandler for App {
    /// Fim do loop (janela fechada ou Esc): salva o progresso.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(path) = &self.state_path else {
            return;
        };
        if let Err(e) = write_state(path, &self.machine.save_state()) {
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

    #[test]
    fn state_file_round_trips_and_resumes() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("saves/jogo.state");
        let rom = vec![0x18, 0xFE]; // JR $
        let mut console = sms_machine(rom.clone());
        console.run_frame();
        write_state(&path, &console.save_state()).expect("grava");
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
}
