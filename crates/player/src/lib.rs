//! Janela nativa que executa um jogo de Master System.
//!
//! Roda num processo próprio, iniciado pelo launcher: o event loop do winit
//! precisa da thread principal (no macOS só pode existir um por processo) e
//! um travamento aqui não derruba a biblioteca.
//!
//! Controles: setas = direcional, Z = botão 1, X = botão 2, Enter = Pause,
//! Esc = fechar.

mod audio;
mod ring;
mod video;

use std::fmt;
use std::num::NonZeroU32;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use sms::{Console, Joypad, SCREEN_HEIGHT, SCREEN_WIDTH};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use audio::AudioOut;

/// Taxa usada quando não há áudio: o PSG precisa de alguma taxa de saída.
const FALLBACK_SAMPLE_RATE: u32 = 44_100;
/// Escala inicial da janela.
const INITIAL_SCALE: u32 = 3;
/// Atraso máximo recuperado de uma vez; além disso o relógio é reiniciado
/// (evita a "espiral" de quadros depois de a janela ficar travada).
const MAX_CATCH_UP_FRAMES: u32 = 3;

#[derive(Debug)]
pub enum PlayerError {
    ReadRom(std::io::Error),
    Rom(sms::RomError),
    EventLoop(winit::error::EventLoopError),
    Window(winit::error::OsError),
    Surface(softbuffer::SoftBufferError),
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadRom(e) => write!(f, "não foi possível ler a ROM: {e}"),
            Self::Rom(e) => write!(f, "ROM inválida: {e}"),
            Self::EventLoop(e) => write!(f, "falha no loop de eventos: {e}"),
            Self::Window(e) => write!(f, "não foi possível abrir a janela: {e}"),
            Self::Surface(e) => write!(f, "falha ao desenhar na janela: {e}"),
        }
    }
}

impl std::error::Error for PlayerError {}

/// Abre a janela e executa o jogo até o usuário fechá-la.
pub fn run(rom_path: &Path) -> Result<(), PlayerError> {
    let rom = std::fs::read(rom_path).map_err(PlayerError::ReadRom)?;
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
    let console = Console::new(rom, sample_rate).map_err(PlayerError::Rom)?;
    let title = rom_path.file_stem().map_or_else(
        || "Master System".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );

    let event_loop = EventLoop::new().map_err(PlayerError::EventLoop)?;
    let context = softbuffer::Context::new(event_loop.owned_display_handle())
        .map_err(PlayerError::Surface)?;
    let mut app = App {
        title,
        console,
        audio,
        audio_scratch: Vec::new(),
        context,
        window: None,
        keys: Keys::default(),
        frame_duration: Duration::from_secs_f64(1.0 / sms::FRAME_RATE),
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
    console: Console,
    audio: Option<AudioOut>,
    audio_scratch: Vec<f32>,
    context: softbuffer::Context<OwnedDisplayHandle>,
    window: Option<GameWindow>,
    keys: Keys,
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
        let size = LogicalSize::new(
            SCREEN_WIDTH as u32 * INITIAL_SCALE,
            SCREEN_HEIGHT as u32 * INITIAL_SCALE,
        );
        let attributes = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(size)
            .with_min_inner_size(LogicalSize::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32));
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
        self.console.set_input(self.keys.joypad);
        self.console.run_frame();
        self.audio_scratch.clear();
        self.console.drain_audio(&mut self.audio_scratch);
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
        video::blit(
            self.console.framebuffer(),
            (SCREEN_WIDTH, SCREEN_HEIGHT),
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
            KeyCode::Enter if pressed && !event.repeat => self.console.press_pause(),
            _ => self.keys.set(code, pressed),
        }
    }
}

impl ApplicationHandler for App {
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
            WindowEvent::Focused(false) => self.keys = Keys::default(),
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

/// Teclas do controle atualmente pressionadas.
#[derive(Debug, Default, Clone, Copy)]
struct Keys {
    joypad: Joypad,
}

impl Keys {
    fn set(&mut self, code: KeyCode, pressed: bool) {
        let p = &mut self.joypad;
        let button = match code {
            KeyCode::ArrowUp => &mut p.up,
            KeyCode::ArrowDown => &mut p.down,
            KeyCode::ArrowLeft => &mut p.left,
            KeyCode::ArrowRight => &mut p.right,
            KeyCode::KeyZ => &mut p.button1,
            KeyCode::KeyX => &mut p.button2,
            _ => return,
        };
        *button = pressed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_joypad() {
        let mut keys = Keys::default();
        keys.set(KeyCode::ArrowLeft, true);
        keys.set(KeyCode::KeyZ, true);
        keys.set(KeyCode::KeyQ, true);
        assert_eq!(
            keys.joypad,
            Joypad {
                left: true,
                button1: true,
                ..Joypad::default()
            }
        );
        keys.set(KeyCode::ArrowLeft, false);
        assert!(!keys.joypad.left);
    }
}
