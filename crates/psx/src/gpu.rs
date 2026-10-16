//! GPU: registradores GP0/GP1, GPUSTAT e o tempo de vídeo (linhas, hblank
//! e V-blank, NTSC e PAL).
//!
//! Por enquanto os comandos de desenho do GP0 são aceitos e descartados; o
//! rasterizador chega na etapa de vídeo.

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
    /// Bits 0–10 do GPUSTAT (vêm do comando E1 do GP0).
    draw_mode: u32,
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
    /// Palavra lida em GPUREAD.
    read_latch: u32,
    /// Chegou ao V-blank desde a última consulta.
    pub(crate) frame_done: bool,
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
            draw_mode: 0,
            display_mode: 0,
            display_disabled: true,
            irq: false,
            dma_direction: 0,
            display_start: (0, 0),
            horizontal_range: (0, 0),
            vertical_range: (0, 0),
            read_latch: 0,
            frame_done: false,
        };
        gpu.reset();
        gpu
    }
}

impl Gpu {
    fn reset(&mut self) {
        self.draw_mode = 0;
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
            }
            on_event(self, VideoEvent::Vblank(vblank), at);
        }
    }

    pub(crate) fn status(&self) -> u32 {
        let mut stat = self.draw_mode & 0x7FF;
        // Sem entrelaçado, o bit do campo fica em 1.
        stat |= u32::from(self.odd_field || !self.interlaced()) << 13;
        // Modo de vídeo: bits 0–5 → 17–22, bit 6 → 16, bit 7 → 14.
        stat |= (self.display_mode & 0x3F) << 17;
        stat |= ((self.display_mode >> 6) & 1) << 16;
        stat |= ((self.display_mode >> 7) & 1) << 14;
        stat |= u32::from(self.display_disabled) << 23;
        stat |= u32::from(self.irq) << 24;
        // Sempre pronta para comandos e DMA; nada a enviar à CPU.
        stat |= 0b101 << 26;
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

    pub(crate) fn read(&self) -> u32 {
        self.read_latch
    }

    pub(crate) fn gp0(&mut self, word: u32) {
        if word >> 24 == 0xE1 {
            self.draw_mode = word & 0x7FF;
        }
    }

    pub(crate) fn gp1(&mut self, word: u32) {
        let value = word & 0x00FF_FFFF;
        match word >> 24 {
            0x00 => self.reset(),
            0x02 => self.irq = false,
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
            // Versão da GPU.
            0x10..=0x1F if value & 7 == 7 => self.read_latch = 2,
            _ => {}
        }
    }
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
}
