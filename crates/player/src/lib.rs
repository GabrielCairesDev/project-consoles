//! Janela nativa que executa um jogo de Master System.
//!
//! Roda num processo próprio, iniciado pelo launcher: o event loop do winit
//! precisa da thread principal (no macOS só pode existir um por processo) e
//! um travamento aqui não derruba a biblioteca.
//!
//! Controles: configuráveis por [`controls::Bindings`] (padrão: setas,
//! Z = botão 1, X = botão 2, Enter = Pause). Esc sempre fecha o jogo.

mod audio;
pub mod controls;
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
use controls::{Action, Bindings};

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
pub fn run(rom_path: &Path, bindings: Bindings) -> Result<(), PlayerError> {
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
        bindings,
        joypad: Joypad::default(),
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
    bindings: Bindings,
    joypad: Joypad,
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
        self.console.set_input(self.joypad);
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
            _ => match self.bindings.action_for(code) {
                Some(Action::Pause) if pressed && !event.repeat => self.console.press_pause(),
                Some(action) => set_button(&mut self.joypad, action, pressed),
                None => {}
            },
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
            WindowEvent::Focused(false) => self.joypad = Joypad::default(),
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

/// Aplica uma ação do controle ao estado dos botões (Pause não é botão do
/// controle: vira NMI em `handle_key`).
fn set_button(joypad: &mut Joypad, action: Action, pressed: bool) {
    let button = match action {
        Action::Up => &mut joypad.up,
        Action::Down => &mut joypad.down,
        Action::Left => &mut joypad.left,
        Action::Right => &mut joypad.right,
        Action::Button1 => &mut joypad.button1,
        Action::Button2 => &mut joypad.button2,
        Action::Pause => return,
    };
    *button = pressed;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_map_to_joypad() {
        let mut joypad = Joypad::default();
        set_button(&mut joypad, Action::Left, true);
        set_button(&mut joypad, Action::Button1, true);
        set_button(&mut joypad, Action::Pause, true);
        assert_eq!(
            joypad,
            Joypad {
                left: true,
                button1: true,
                ..Joypad::default()
            }
        );
        set_button(&mut joypad, Action::Left, false);
        assert!(!joypad.left);
    }
}
