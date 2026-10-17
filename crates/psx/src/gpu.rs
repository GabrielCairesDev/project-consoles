//! GPU: registradores GP1, GPUSTAT, o tempo de vídeo (linhas, hblank e
//! V-blank, NTSC e PAL) e a imagem mostrada. Os comandos de desenho do GP0
//! ficam em [`render`].

mod render;

use render::{Renderer, VRAM_HEIGHT, VRAM_WIDTH};

/// Clock da GPU = 11/7 do clock da CPU.
const CLOCK_NUM: u64 = 11;
const CLOCK_DEN: u64 = 7;

const NTSC_TICKS_PER_LINE: u32 = 3413;
const NTSC_LINES: u16 = 263;
const PAL_TICKS_PER_LINE: u32 = 3406;
const PAL_LINES: u16 = 314;

/// Acontecimentos de vídeo, na ordem em que ocorrem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoEvent {
    HblankStart,
    HblankEnd,
    /// Entrou (`true`) ou saiu do V-blank.
    Vblank(bool),
}

#[derive(Clone)]
pub(crate) struct Gpu {
    last_sync: u64,
    /// Resto de (ciclos × 11) / 7.
    tick_remainder: u64,
    /// Posição na linha, em tiques da GPU.
    line_tick: u32,
    line: u16,
    in_vblank: bool,
    /// Campo do entrelaçado (GPUSTAT bit 13).
    odd_field: bool,
    pub(crate) renderer: Renderer,
    /// Modo de vídeo (GP1 08h), bits 0–7.
    display_mode: u32,
    display_disabled: bool,
    irq: bool,
    dma_direction: u32,
    /// Início da área de display na VRAM (GP1 05h).
    pub(crate) display_start: (u16, u16),
    /// Faixa horizontal (GP1 06h), em tiques da GPU.
    horizontal_range: (u32, u32),
    /// Faixa vertical (GP1 07h), em linhas.
    vertical_range: (u16, u16),
    /// Chegou ao V-blank desde a última consulta.
    pub(crate) frame_done: bool,
    /// Imagem do último quadro, `0x00RRGGBB`, e o tamanho.
    frame: Vec<u32>,
    frame_size: (usize, usize),
    /// O mesmo quadro na resolução interna, quando ela é maior que 1× e o
    /// modo é de 15 bits (`None` usa `frame`). Só para a apresentação.
    upscaled_frame: Vec<u32>,
    upscaled_size: Option<(usize, usize)>,
}

impl Default for Gpu {
    fn default() -> Self {
        let mut gpu = Self {
            last_sync: 0,
            tick_remainder: 0,
            line_tick: 0,
            line: 0,
            in_vblank: true,
            odd_field: false,
            renderer: Renderer::default(),
            display_mode: 0,
            display_disabled: true,
            irq: false,
            dma_direction: 0,
            display_start: (0, 0),
            horizontal_range: (0, 0),
            vertical_range: (0, 0),
            frame_done: false,
            frame: vec![0; 320 * 240],
            frame_size: (320, 240),
            upscaled_frame: Vec::new(),
            upscaled_size: None,
        };
        gpu.reset();
        gpu
    }
}

impl Gpu {
    fn reset(&mut self) {
        self.renderer.reset();
        self.display_mode = 0;
        self.display_disabled = true;
        self.irq = false;
        self.dma_direction = 0;
        self.display_start = (0, 0);
        self.horizontal_range = (0x200, 0x200 + 256 * 10);
        self.vertical_range = (0x10, 0x10 + 240);
    }

    pub(crate) fn is_pal(&self) -> bool {
        self.display_mode & (1 << 3) != 0
    }

    fn interlaced(&self) -> bool {
        self.display_mode & (1 << 5) != 0
    }

    fn ticks_per_line(&self) -> u32 {
        if self.is_pal() {
            PAL_TICKS_PER_LINE
        } else {
            NTSC_TICKS_PER_LINE
        }
    }

    fn lines(&self) -> u16 {
        if self.is_pal() { PAL_LINES } else { NTSC_LINES }
    }

    /// Quadros por segundo no modo atual (~59,29 NTSC, ~49,76 PAL).
    pub(crate) fn frame_rate(&self) -> f64 {
        let ticks_per_second = f64::from(crate::CPU_CLOCK) * CLOCK_NUM as f64 / CLOCK_DEN as f64;
        ticks_per_second / (f64::from(self.ticks_per_line()) * f64::from(self.lines()))
    }

    /// Divisor do dot clock no modo atual (256, 320, 368, 512 ou 640 pixels).
    pub(crate) fn dot_divider(&self) -> u64 {
        if self.display_mode & (1 << 6) != 0 {
            7
        } else {
            [10, 8, 5, 4][(self.display_mode & 3) as usize]
        }
    }

    /// Tiques da GPU até o próximo acontecimento na linha.
    fn ticks_to_boundary(&self) -> u32 {
        let (start, end) = self.hblank_edges();
        [end, start, self.ticks_per_line()]
            .into_iter()
            .filter(|&t| t > self.line_tick)
            .min()
            .unwrap_or(self.ticks_per_line())
            - self.line_tick
    }

    /// (fim do hblank, começo do hblank) dentro da linha.
    fn hblank_edges(&self) -> (u32, u32) {
        let last = self.ticks_per_line() - 1;
        let (x1, x2) = self.horizontal_range;
        (x1.min(last), x2.clamp(x1.min(last), last))
    }

    fn cycles_for_ticks(&self, ticks: u32) -> u64 {
        (u64::from(ticks) * CLOCK_DEN)
            .saturating_sub(self.tick_remainder)
            .div_ceil(CLOCK_NUM)
            .max(1)
    }

    /// Ciclo da CPU do próximo acontecimento de vídeo.
    pub(crate) fn next_event(&self) -> u64 {
        self.last_sync + self.cycles_for_ticks(self.ticks_to_boundary())
    }

    /// Avança até o ciclo `now`, chamando `on_event` em cada acontecimento
    /// com o ciclo em que ele ocorreu.
    pub(crate) fn sync(&mut self, now: u64, mut on_event: impl FnMut(&mut Self, VideoEvent, u64)) {
        while self.last_sync < now {
            let to_boundary = self.ticks_to_boundary();
            let cycles = self.cycles_for_ticks(to_boundary);
            if self.last_sync + cycles > now {
                self.advance_ticks(now - self.last_sync);
                self.last_sync = now;
                break;
            }
            // A conversão de ciclos pode passar um tique da fronteira; o
            // acontecimento é o da fronteira pedida.
            let boundary = self.line_tick + to_boundary;
            self.advance_ticks(cycles);
            self.last_sync += cycles;
            let at = self.last_sync;
            let (hblank_end, hblank_start) = self.hblank_edges();
            if boundary >= self.ticks_per_line() {
                self.line_tick = self.line_tick.saturating_sub(self.ticks_per_line());
                self.next_line(at, &mut on_event);
            } else if boundary == hblank_start {
                on_event(self, VideoEvent::HblankStart, at);
            } else if boundary == hblank_end {
                on_event(self, VideoEvent::HblankEnd, at);
            }
        }
    }

    fn advance_ticks(&mut self, cycles: u64) {
        let total = cycles * CLOCK_NUM + self.tick_remainder;
        self.tick_remainder = total % CLOCK_DEN;
        // Nunca passa do fim da linha: `sync` para em cada fronteira.
        let ticks = (total / CLOCK_DEN).min(u64::from(self.ticks_per_line()));
        self.line_tick += ticks as u32;
    }

    fn next_line(&mut self, at: u64, on_event: &mut impl FnMut(&mut Self, VideoEvent, u64)) {
        self.line += 1;
        if self.line >= self.lines() {
            self.line = 0;
        }
        let (y1, y2) = self.vertical_range;
        let vblank = self.line < y1 || self.line >= y2;
        if vblank != self.in_vblank {
            self.in_vblank = vblank;
            if vblank {
                if self.interlaced() {
                    self.odd_field = !self.odd_field;
                } else {
                    self.odd_field = false;
                }
                self.frame_done = true;
                self.capture_frame();
            }
            on_event(self, VideoEvent::Vblank(vblank), at);
        }
    }

    /// Largura e altura visíveis no modo atual.
    fn display_size(&self) -> (usize, usize) {
        let width = if self.display_mode & (1 << 6) != 0 {
            368
        } else {
            [256, 320, 512, 640][(self.display_mode & 3) as usize]
        };
        let (y1, y2) = self.vertical_range;
        let lines =
            usize::from(y2.saturating_sub(y1)).clamp(1, if self.is_pal() { 288 } else { 240 });
        let double = self.interlaced() && self.display_mode & (1 << 2) != 0;
        (width, if double { lines * 2 } else { lines })
    }

    /// Copia a área de display da VRAM para a imagem do quadro.
    fn capture_frame(&mut self) {
        let (width, height) = self.display_size();
        self.frame_size = (width, height);
        self.frame.resize(width * height, 0);
        if self.display_disabled {
            self.frame.fill(0);
            return;
        }
        let (sx, sy) = (
            usize::from(self.display_start.0),
            usize::from(self.display_start.1),
        );
        let vram = &self.renderer;
        let rgb24 = self.display_mode & (1 << 4) != 0;
        for (y, row) in self.frame.chunks_exact_mut(width).enumerate() {
            let vy = (sy + y) % VRAM_HEIGHT;
            if rgb24 {
                // 3 bytes por pixel, a partir do halfword sx da linha.
                let byte = |i: usize| {
                    let half = vram.pixel((sx + i / 2) % VRAM_WIDTH, vy);
                    if i.is_multiple_of(2) {
                        half & 0xFF
                    } else {
                        half >> 8
                    }
                };
                for (x, out) in row.iter_mut().enumerate() {
                    let i = x * 3;
                    let (r, g, b) = (byte(i), byte(i + 1), byte(i + 2));
                    *out = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
                }
            } else {
                for (x, out) in row.iter_mut().enumerate() {
                    *out = rgb555_to_rgb888(vram.pixel((sx + x) % VRAM_WIDTH, vy));
                }
            }
        }
        self.upscaled_size = None;
        // 24 bits (vídeos do MDEC) são bytes, não pixels: ficam em 1×.
        if !rgb24 && let Some(up) = self.renderer.upscaled() {
            let s = up.scale();
            let (width, height) = (width * s, height * s);
            self.upscaled_frame.resize(width * height, 0);
            let (sx, sy) = (sx * s, sy * s);
            for (y, row) in self.upscaled_frame.chunks_exact_mut(width).enumerate() {
                let line = up.row((sy + y) % up.height());
                for (x, out) in row.iter_mut().enumerate() {
                    // sx < largura da VRAM e x < 640·s: no máximo uma volta.
                    let vx = sx + x;
                    let vx = if vx >= line.len() {
                        vx - line.len()
                    } else {
                        vx
                    };
                    *out = rgb555_to_rgb888(line[vx]);
                }
            }
            self.upscaled_size = Some((width, height));
        }
    }

    /// Imagem do último quadro mostrado e o tamanho (largura, altura), na
    /// resolução interna quando ela está ligada.
    pub(crate) fn frame(&self) -> (&[u32], (usize, usize)) {
        match self.upscaled_size {
            Some(size) => (&self.upscaled_frame, size),
            None => (&self.frame, self.frame_size),
        }
    }

    /// Escala da resolução interna (1 = nativa). A imagem ampliada aparece
    /// a partir do próximo quadro.
    pub(crate) fn set_internal_scale(&mut self, scale: usize) {
        self.renderer.set_scale(scale);
        self.upscaled_size = None;
    }

    pub(crate) fn status(&self) -> u32 {
        let mut stat = self.renderer.status_bits();
        // Sem entrelaçado, o bit do campo fica em 1.
        stat |= u32::from(self.odd_field || !self.interlaced()) << 13;
        // Modo de vídeo: bits 0–5 → 17–22, bit 6 → 16, bit 7 → 14.
        stat |= (self.display_mode & 0x3F) << 17;
        stat |= ((self.display_mode >> 6) & 1) << 16;
        stat |= ((self.display_mode >> 7) & 1) << 14;
        stat |= u32::from(self.display_disabled) << 23;
        stat |= u32::from(self.irq) << 24;
        // Sempre pronta para comandos e DMA (o desenho é instantâneo).
        stat |= 0b101 << 26;
        stat |= u32::from(self.renderer.has_read_data()) << 27;
        stat |= self.dma_direction << 29;
        let request = match self.dma_direction {
            0 => 0,
            1 => 1,
            2 => (stat >> 28) & 1,
            _ => (stat >> 27) & 1,
        };
        stat |= request << 25;
        let odd_line = if self.in_vblank {
            false
        } else if self.interlaced() {
            self.odd_field
        } else {
            self.line & 1 != 0
        };
        stat |= u32::from(odd_line) << 31;
        stat
    }

    pub(crate) fn read(&mut self) -> u32 {
        self.renderer.read()
    }

    pub(crate) fn gp0(&mut self, word: u32) {
        self.renderer.gp0(word);
    }

    /// O GP0 pediu interrupção (comando 1Fh) desde a última consulta.
    pub(crate) fn take_irq(&mut self) -> bool {
        if self.renderer.irq_requested && !self.irq {
            self.irq = true;
            return true;
        }
        false
    }

    pub(crate) fn gp1(&mut self, word: u32) {
        let value = word & 0x00FF_FFFF;
        match word >> 24 {
            0x00 => self.reset(),
            0x01 => self.renderer.reset_command_buffer(),
            0x02 => {
                self.irq = false;
                self.renderer.irq_requested = false;
            }
            0x03 => self.display_disabled = value & 1 != 0,
            0x04 => self.dma_direction = value & 3,
            0x05 => {
                self.display_start = ((value & 0x3FE) as u16, ((value >> 10) & 0x1FF) as u16);
            }
            0x06 => self.horizontal_range = (value & 0xFFF, (value >> 12) & 0xFFF),
            0x07 => {
                self.vertical_range = ((value & 0x3FF) as u16, ((value >> 10) & 0x3FF) as u16);
            }
            0x08 => self.display_mode = value & 0xFF,
            0x10..=0x1F => self.renderer.info(value),
            _ => {}
        }
    }
}

fn rgb555_to_rgb888(pixel: u16) -> u32 {
    let expand = |c: u16| {
        let c = u32::from(c & 0x1F);
        (c << 3) | (c >> 2)
    };
    (expand(pixel) << 16) | (expand(pixel >> 5) << 8) | expand(pixel >> 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(gpu: &mut Gpu, cycles: u64) -> Vec<VideoEvent> {
        let mut events = Vec::new();
        let target = gpu.last_sync + cycles;
        gpu.sync(target, |_, e, _| events.push(e));
        events
    }

    #[test]
    fn frame_rate_follows_the_video_standard() {
        let mut gpu = Gpu::default();
        // 33 868 800 / 571 212 ciclos por quadro.
        assert!(
            (gpu.frame_rate() - 59.293).abs() < 0.001,
            "{}",
            gpu.frame_rate()
        );
        gpu.gp1(0x0800_0008);
        assert!(
            (gpu.frame_rate() - 49.765).abs() < 0.001,
            "{}",
            gpu.frame_rate()
        );
    }

    #[test]
    fn ntsc_frame_has_263_lines_of_3413_ticks() {
        let mut gpu = Gpu::default();
        // 263 × 3413 tiques da GPU = 571212 ciclos da CPU.
        let events = run(&mut gpu, 571_212);
        let hblanks = events
            .iter()
            .filter(|e| **e == VideoEvent::HblankStart)
            .count();
        assert_eq!(hblanks, 263);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, VideoEvent::Vblank(_)))
                .count(),
            2,
            "entra e sai do V-blank uma vez"
        );
        assert!(gpu.frame_done);
    }

    #[test]
    fn next_event_is_reached_exactly() {
        let mut gpu = Gpu::default();
        for _ in 0..1000 {
            let next = gpu.next_event();
            let mut seen = 0;
            gpu.sync(next, |_, _, at| {
                assert_eq!(at, next);
                seen += 1;
            });
            assert!(seen <= 1);
        }
    }

    #[test]
    fn odd_line_bit_toggles_in_progressive_mode() {
        let mut gpu = Gpu::default();
        gpu.gp1(0x0700_0000 | (0x100 << 10)); // linhas 0..256 visíveis
        let mut bits = Vec::new();
        for _ in 0..4 {
            let next = gpu.next_event();
            gpu.sync(next, |_, _, _| {});
            while gpu.line_tick != 0 {
                let next = gpu.next_event();
                gpu.sync(next, |_, _, _| {});
            }
            bits.push(gpu.status() >> 31);
        }
        assert!(bits.windows(2).all(|w| w[0] != w[1]), "{bits:?}");
    }

    #[test]
    fn display_mode_reflects_in_status_and_pal_timing() {
        let mut gpu = Gpu::default();
        gpu.gp1(0x0800_0000 | 0b0100_1001); // 368 pixels, PAL
        assert_eq!(gpu.dot_divider(), 7);
        assert!(gpu.is_pal());
        assert_ne!(gpu.status() & (1 << 20), 0);
        assert_ne!(gpu.status() & (1 << 16), 0);
        gpu.gp1(0x0300_0000);
        assert_eq!(gpu.status() & (1 << 23), 0, "display ligado");
    }

    /// GPU com o display ligado em 320×240 e um retângulo azul em (16, 4).
    fn gpu_with_picture(scale: usize) -> Gpu {
        let mut gpu = Gpu::default();
        gpu.set_internal_scale(scale);
        gpu.gp1(0x0300_0000);
        gpu.gp1(0x0800_0001);
        for word in [0x02FF_0000, 4 << 16 | 16, 3 << 16 | 16] {
            gpu.gp0(word);
        }
        gpu.capture_frame();
        gpu
    }

    #[test]
    fn native_frame_is_unchanged_at_scale_one() {
        let gpu = gpu_with_picture(1);
        let (pixels, size) = gpu.frame();
        assert_eq!(size, (320, 240));
        assert_eq!(pixels, &gpu.frame[..]);
        assert_eq!(pixels[4 * 320 + 16], 0x0000_00FF);
        assert_eq!(pixels[4 * 320 + 15], 0);
    }

    #[test]
    fn upscaled_frame_is_scale_times_larger() {
        let gpu = gpu_with_picture(3);
        let (pixels, (width, height)) = gpu.frame();
        assert_eq!((width, height), (960, 720));
        let (native, _) = (&gpu.frame, gpu.frame_size);
        for y in 0..height {
            for x in 0..width {
                assert_eq!(pixels[y * width + x], native[(y / 3) * 320 + x / 3]);
            }
        }
    }

    #[test]
    fn rgb24_and_disabled_display_stay_native() {
        let mut gpu = gpu_with_picture(2);
        gpu.gp1(0x0800_0011); // 24 bits
        gpu.capture_frame();
        assert_eq!(gpu.frame().1, (320, 240));
        gpu.gp1(0x0800_0001);
        gpu.gp1(0x0300_0001);
        gpu.capture_frame();
        assert_eq!(gpu.frame().1, (320, 240));
    }

    #[test]
    fn changing_the_scale_waits_for_the_next_frame() {
        let mut gpu = gpu_with_picture(2);
        assert_eq!(gpu.frame().1, (640, 480));
        gpu.set_internal_scale(1);
        assert_eq!(gpu.frame().1, (320, 240));
        gpu.set_internal_scale(4);
        assert_eq!(gpu.frame().1, (320, 240), "sem quadro ampliado ainda");
        gpu.capture_frame();
        assert_eq!(gpu.frame().1, (1280, 960));
    }
}
