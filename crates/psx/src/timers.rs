//! Os 3 timers (contadores de 16 bits em 0x1F801100–0x1F80112F).
//!
//! As fontes que seguem o clock da CPU (clock do sistema, ÷8 e o dot clock
//! da GPU) são contadas sob demanda, a partir dos ciclos passados desde a
//! última sincronização. As de vídeo (hblank) e os modos de sincronismo
//! chegam pela GPU, a cada linha e no começo e fim do V-blank.

const SYNC_ENABLE: u16 = 1;
const RESET_ON_TARGET: u16 = 1 << 3;
const IRQ_ON_TARGET: u16 = 1 << 4;
const IRQ_ON_MAX: u16 = 1 << 5;
const IRQ_REPEAT: u16 = 1 << 6;
const IRQ_TOGGLE: u16 = 1 << 7;
/// Bit 10: 1 = sem pedido de interrupção (é invertido).
const IRQ_NOT_REQUESTED: u16 = 1 << 10;
const REACHED_TARGET: u16 = 1 << 11;
const REACHED_MAX: u16 = 1 << 12;

/// O dot clock é o clock da GPU (11/7 do da CPU) dividido pelo divisor do
/// modo de vídeo.
const GPU_CLOCK_NUM: u64 = 11;
const GPU_CLOCK_DEN: u64 = 7;

#[derive(Clone, Default)]
struct Timer {
    counter: u16,
    mode: u16,
    target: u16,
    /// Já pediu a interrupção (no modo de uma vez só).
    irq_done: bool,
    /// Parado pelo modo de sincronismo.
    paused: bool,
}

impl Timer {
    fn sync_mode(&self) -> Option<u16> {
        (self.mode & SYNC_ENABLE != 0).then_some((self.mode >> 1) & 3)
    }

    fn source(&self) -> u16 {
        (self.mode >> 8) & 3
    }

    /// Valor em que o contador volta a 0 no próximo tique.
    fn wrap(&self) -> u16 {
        if self.mode & RESET_ON_TARGET != 0 {
            self.target
        } else {
            0xFFFF
        }
    }

    fn hit(&mut self, flag: u16, irq_enable: u16) -> bool {
        self.mode |= flag;
        if self.mode & irq_enable == 0 || (self.mode & IRQ_REPEAT == 0 && self.irq_done) {
            return false;
        }
        self.irq_done = true;
        if self.mode & IRQ_TOGGLE != 0 {
            self.mode ^= IRQ_NOT_REQUESTED;
            self.mode & IRQ_NOT_REQUESTED == 0
        } else {
            // Pulso: o bit 10 cai por um instante e volta a 1.
            true
        }
    }

    /// Avança `n` tiques; `true` se pediu interrupção.
    fn advance(&mut self, mut n: u64) -> bool {
        if self.paused || n == 0 {
            return false;
        }
        let mut irq = false;
        let target = u64::from(self.target);
        let mut c = u64::from(self.counter);
        while n > 0 {
            let wrap = u64::from(self.wrap());
            // Contador acima do alvo (alvo mudou): vai até 0xFFFF.
            let limit = if c <= wrap { wrap } else { 0xFFFF };
            let to_limit = limit - c;
            if n <= to_limit {
                let next = c + n;
                if c < target && target <= next {
                    irq |= self.hit(REACHED_TARGET, IRQ_ON_TARGET);
                }
                if next == 0xFFFF {
                    irq |= self.hit(REACHED_MAX, IRQ_ON_MAX);
                }
                c = next;
                n = 0;
            } else {
                if c < target && target <= limit {
                    irq |= self.hit(REACHED_TARGET, IRQ_ON_TARGET);
                }
                if limit == 0xFFFF && c < 0xFFFF {
                    irq |= self.hit(REACHED_MAX, IRQ_ON_MAX);
                }
                n -= to_limit + 1;
                c = 0;
                // Voltas inteiras não mudam nada além das flags, que já
                // estão marcadas: pula direto para a última.
                let period = wrap + 1;
                if n > period {
                    n %= period;
                    if target <= wrap {
                        irq |= self.hit(REACHED_TARGET, IRQ_ON_TARGET);
                    }
                }
            }
        }
        self.counter = c as u16;
        irq
    }

    /// Tiques até o próximo pedido de interrupção, se houver um.
    fn ticks_to_irq(&self) -> Option<u64> {
        if self.paused || self.mode & (IRQ_ON_TARGET | IRQ_ON_MAX) == 0 {
            return None;
        }
        if self.mode & IRQ_REPEAT == 0 && self.irq_done {
            return None;
        }
        let c = u64::from(self.counter);
        let target = u64::from(self.target);
        let wrap = u64::from(self.wrap());
        let mut best = u64::MAX;
        if self.mode & IRQ_ON_TARGET != 0 {
            best = if c < target {
                target - c
            } else {
                // Volta a 0 e conta até o alvo.
                let limit = if c <= wrap { wrap } else { 0xFFFF };
                limit - c + 1 + target
            };
        }
        if self.mode & IRQ_ON_MAX != 0 {
            let limit = if c <= wrap { wrap } else { 0xFFFF };
            if limit == 0xFFFF {
                best = best.min(0xFFFF - c);
            }
        }
        (best != u64::MAX).then_some(best.max(1))
    }
}

#[derive(Clone, Default)]
pub(crate) struct Timers {
    timers: [Timer; 3],
    last_sync: u64,
    /// Restos das divisões de clock, em ciclos da CPU × denominador.
    dot_remainder: u64,
    div8_remainder: u64,
    in_hblank: bool,
    in_vblank: bool,
}

/// Interrupções pedidas: bit `n` = timer `n`.
pub(crate) type TimerIrqs = u8;

impl Timers {
    /// Avança os contadores até o ciclo `now`. `dot_divider` é o divisor
    /// do dot clock no modo de vídeo atual.
    pub(crate) fn sync(&mut self, now: u64, dot_divider: u64) -> TimerIrqs {
        let elapsed = now.saturating_sub(self.last_sync);
        self.last_sync = now;
        if elapsed == 0 {
            return 0;
        }
        let dots = {
            let total = elapsed * GPU_CLOCK_NUM + self.dot_remainder;
            let per_dot = GPU_CLOCK_DEN * dot_divider;
            self.dot_remainder = total % per_dot;
            total / per_dot
        };
        let div8 = {
            let total = elapsed + self.div8_remainder;
            self.div8_remainder = total % 8;
            total / 8
        };
        let mut irqs = 0;
        for (n, timer) in self.timers.iter_mut().enumerate() {
            let ticks = match (n, timer.source()) {
                (0, 1 | 3) => dots,
                // Fonte hblank: contada por `hblank`.
                (1, 1 | 3) => 0,
                (2, 2 | 3) => div8,
                _ => elapsed,
            };
            if timer.advance(ticks) {
                irqs |= 1 << n;
            }
        }
        irqs
    }

    /// Ciclos da CPU até o próximo pedido de interrupção de um timer que
    /// conta sozinho (as fontes de vídeo chegam pelos eventos da GPU).
    pub(crate) fn cycles_to_next_irq(&self, dot_divider: u64) -> Option<u64> {
        self.timers
            .iter()
            .enumerate()
            .filter_map(|(n, timer)| {
                let ticks = timer.ticks_to_irq()?;
                match (n, timer.source()) {
                    (0, 1 | 3) => {
                        Some((ticks * GPU_CLOCK_DEN * dot_divider).div_ceil(GPU_CLOCK_NUM))
                    }
                    (1, 1 | 3) => None,
                    (2, 2 | 3) => Some(ticks * 8),
                    _ => Some(ticks),
                }
            })
            .min()
    }

    /// Começo do hblank de uma linha. Chamar depois de [`Timers::sync`].
    pub(crate) fn hblank(&mut self) -> TimerIrqs {
        self.in_hblank = true;
        let mut irqs = 0;
        let t0 = &mut self.timers[0];
        match t0.sync_mode() {
            // Pausa durante o hblank.
            Some(0) => t0.paused = true,
            Some(1) => t0.counter = 0,
            // Zera no hblank e só conta dentro dele.
            Some(2) => {
                t0.counter = 0;
                t0.paused = false;
            }
            // Espera o primeiro hblank e depois conta livre.
            Some(3) => {
                t0.paused = false;
                t0.mode &= !SYNC_ENABLE;
            }
            _ => {}
        }
        let t1 = &mut self.timers[1];
        if matches!(t1.source(), 1 | 3) && t1.advance(1) {
            irqs |= 1 << 1;
        }
        irqs
    }

    /// Fim do hblank (começo da parte visível da linha seguinte).
    pub(crate) fn hblank_end(&mut self) {
        self.in_hblank = false;
        let t0 = &mut self.timers[0];
        if matches!(t0.sync_mode(), Some(0 | 2)) {
            t0.paused = t0.sync_mode() == Some(2);
        }
    }

    /// Começo (`true`) ou fim do V-blank. Chamar depois de [`Timers::sync`].
    pub(crate) fn vblank(&mut self, entering: bool) {
        self.in_vblank = entering;
        let t1 = &mut self.timers[1];
        match t1.sync_mode() {
            // Pausa durante o V-blank.
            Some(0) => t1.paused = entering,
            // Zera no V-blank.
            Some(1) if entering => t1.counter = 0,
            // Zera no V-blank e só conta dentro dele.
            Some(2) => {
                if entering {
                    t1.counter = 0;
                }
                t1.paused = !entering;
            }
            // Espera o primeiro V-blank e depois conta livre.
            Some(3) if entering => {
                t1.paused = false;
                t1.mode &= !SYNC_ENABLE;
            }
            _ => {}
        }
    }

    pub(crate) fn read(&mut self, offset: u32) -> u32 {
        let timer = &mut self.timers[((offset >> 4) & 3).min(2) as usize];
        match offset & 0xF {
            0x0 => u32::from(timer.counter),
            0x4 => {
                let mode = timer.mode;
                // Ler o modo apaga as flags de alvo e de 0xFFFF.
                timer.mode &= !(REACHED_TARGET | REACHED_MAX);
                u32::from(mode)
            }
            0x8 => u32::from(timer.target),
            _ => 0,
        }
    }

    pub(crate) fn write(&mut self, offset: u32, value: u32) {
        let n = ((offset >> 4) & 3) as usize;
        if n > 2 {
            return;
        }
        let in_hblank = self.in_hblank;
        let in_vblank = self.in_vblank;
        let timer = &mut self.timers[n];
        let value = value as u16;
        match offset & 0xF {
            0x0 => timer.counter = value,
            0x4 => {
                timer.mode = (value & 0x3FF) | IRQ_NOT_REQUESTED;
                timer.counter = 0;
                timer.irq_done = false;
                timer.paused = match (n, timer.sync_mode()) {
                    (0, Some(0)) => in_hblank,
                    (1, Some(0)) => in_vblank,
                    (0, Some(2)) => !in_hblank,
                    (1, Some(2)) => !in_vblank,
                    (0 | 1, Some(3)) => true,
                    (2, Some(0 | 3)) => true,
                    _ => false,
                };
            }
            0x8 => timer.target = value,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timers_with(n: usize, mode: u32, target: u32) -> Timers {
        let mut t = Timers::default();
        t.write((n as u32) << 4 | 8, target);
        t.write((n as u32) << 4 | 4, mode);
        t
    }

    #[test]
    fn counts_system_clock_and_resets_on_target() {
        let mut t = timers_with(2, u32::from(RESET_ON_TARGET), 99);
        t.sync(250, 10);
        // 0..=99 é um período de 100 tiques: 250 tiques → 50.
        assert_eq!(t.read(0x20), 50);
        assert_ne!(t.read(0x24) & u32::from(REACHED_TARGET), 0);
        assert_eq!(t.read(0x24) & u32::from(REACHED_TARGET), 0, "ler apaga");
    }

    #[test]
    fn divider_by_eight_keeps_the_remainder() {
        let mut t = timers_with(2, 2 << 8, 0);
        t.sync(7, 10);
        assert_eq!(t.read(0x20), 0);
        t.sync(8, 10);
        assert_eq!(t.read(0x20), 1);
        t.sync(8 * 100, 10);
        assert_eq!(t.read(0x20), 100);
    }

    #[test]
    fn irq_on_target_once_or_repeated() {
        let mode = u32::from(RESET_ON_TARGET | IRQ_ON_TARGET);
        let mut t = timers_with(0, mode, 9);
        assert_eq!(t.cycles_to_next_irq(10), Some(9));
        assert_eq!(t.sync(9, 10), 1);
        assert_eq!(t.sync(30, 10), 0, "uma vez só");
        assert_eq!(t.cycles_to_next_irq(10), None);

        let mut t = timers_with(0, mode | u32::from(IRQ_REPEAT), 9);
        assert_eq!(t.sync(9, 10), 1);
        assert_eq!(t.sync(19, 10), 1, "repete");
    }

    #[test]
    fn overflow_irq_at_ffff() {
        let mut t = timers_with(1, u32::from(IRQ_ON_MAX), 0);
        assert_eq!(t.cycles_to_next_irq(10), Some(0xFFFF));
        assert_eq!(t.sync(0xFFFE, 10), 0);
        assert_eq!(t.sync(0xFFFF, 10), 2);
        t.sync(0x10001, 10);
        assert_eq!(t.read(0x10), 1, "voltou a 0 e contou mais 1");
    }

    #[test]
    fn hblank_source_and_vblank_reset() {
        // Timer 1 contando hblank, zerado no V-blank (sincronismo modo 1).
        let mut t = timers_with(1, (1 << 8) | (1 << 1) | 1, 0);
        for _ in 0..5 {
            t.hblank();
        }
        assert_eq!(t.read(0x10), 5);
        t.vblank(true);
        assert_eq!(t.read(0x10), 0);
    }

    #[test]
    fn timer2_sync_modes_0_and_3_stop_the_counter() {
        let mut t = timers_with(2, 1, 0);
        t.sync(100, 10);
        assert_eq!(t.read(0x20), 0);
        let mut t = timers_with(2, (1 << 1) | 1, 0);
        t.sync(100, 10);
        assert_eq!(t.read(0x20), 100);
    }

    #[test]
    fn dot_clock_follows_gpu_clock() {
        // Divisor 10 (256 pixels): 7 ciclos de CPU = 11 da GPU.
        let mut t = timers_with(0, 1 << 8, 0);
        t.sync(70, 10);
        assert_eq!(t.read(0x00), 11);
    }
}
