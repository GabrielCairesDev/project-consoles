//! Barramento: mapa de memória, registradores de I/O, DMA e o relógio
//! global (ciclos da CPU) com os eventos dos dispositivos.
//!
//! Mapa físico:
//! - 0x0000_0000: RAM de 2 MB, espelhada até 8 MB;
//! - 0x1F00_0000: expansão 1 (vazia, lê 0xFF);
//! - 0x1F80_0000: scratchpad de 1 KB;
//! - 0x1F80_1000: I/O;
//! - 0x1F80_2000: expansão 2 (vazia);
//! - 0x1FC0_0000: BIOS de 512 KB;
//! - 0xFFFE_0130: controle do cache (em KSEG2).
//!
//! KUSEG, KSEG0 e KSEG1 são a mesma memória física.

use crate::cdrom::Cdrom;
use crate::dma::{self, Dma};
use crate::gpu::{Gpu, VideoEvent};
use crate::irq::{Interrupts, Irq};
use crate::sio::Sio;
use crate::spu::Spu;
use crate::timers::{TimerIrqs, Timers};

pub(crate) const RAM_SIZE: usize = 2 * 1024 * 1024;
pub(crate) const BIOS_SIZE: usize = 512 * 1024;
const SCRATCHPAD_SIZE: usize = 1024;

const RAM_END: u32 = 0x0080_0000;
const SCRATCHPAD: u32 = 0x1F80_0000;
const IO: u32 = 0x1F80_1000;
const IO_END: u32 = 0x1F80_2000;
const EXPANSION_1: u32 = 0x1F00_0000;
const EXPANSION_2_END: u32 = 0x1F80_4000;
const BIOS: u32 = 0x1FC0_0000;
const BIOS_END: u32 = BIOS + BIOS_SIZE as u32;
const CACHE_CONTROL: u32 = 0xFFFE_0130;

/// Custo aproximado de cada acesso, em ciclos da CPU, além do ciclo da
/// instrução. Valores médios medidos por outros emuladores; o BIOS fica
/// num barramento de 8 bits, bem mais lento.
const RAM_TICKS: u64 = 3;
const BIOS_TICKS: u64 = 18;
const IO_TICKS: u64 = 2;
const RAM_FETCH_TICKS: u64 = 4;
const BIOS_FETCH_TICKS: u64 = 20;

/// Tira os bits de segmento (KSEG0/KSEG1); KSEG2 fica como está.
const REGION_MASK: [u32; 8] = [
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0x7FFF_FFFF,
    0x1FFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
];

#[inline]
fn physical(addr: u32) -> u32 {
    addr & REGION_MASK[(addr >> 29) as usize]
}

/// Lê `W` bytes (little-endian) de `memory` a partir de `index`.
#[inline]
fn load<const W: usize>(memory: &[u8], index: usize) -> u32 {
    let mut bytes = [0; 4];
    bytes[..W].copy_from_slice(&memory[index..index + W]);
    u32::from_le_bytes(bytes)
}

#[inline]
fn store<const W: usize>(memory: &mut [u8], index: usize, value: u32) {
    memory[index..index + W].copy_from_slice(&value.to_le_bytes()[..W]);
}

#[derive(Clone)]
pub(crate) struct Bus {
    /// Relógio global, em ciclos da CPU.
    pub(crate) now: u64,
    /// Próximo ciclo em que algum dispositivo precisa ser atualizado.
    pub(crate) next_event: u64,
    pub(crate) ram: Box<[u8; RAM_SIZE]>,
    bios: Box<[u8; BIOS_SIZE]>,
    scratchpad: Box<[u8; SCRATCHPAD_SIZE]>,
    /// Registradores de atraso do barramento (0x1F801000–0x1F801020) e o
    /// tamanho da RAM (0x1F801060): só guardados.
    memory_control: [u32; 9],
    ram_size: u32,
    cache_control: u32,
    pub(crate) irq: Interrupts,
    dma: Dma,
    timers: Timers,
    pub(crate) gpu: Gpu,
    pub(crate) cdrom: Cdrom,
    spu: Spu,
    pub(crate) sio: Sio,
}

impl Bus {
    pub(crate) fn new(bios: Box<[u8; BIOS_SIZE]>) -> Self {
        let mut bus = Self {
            now: 0,
            next_event: 0,
            ram: Box::new([0; RAM_SIZE]),
            bios,
            scratchpad: Box::new([0; SCRATCHPAD_SIZE]),
            memory_control: [0; 9],
            ram_size: 0x0000_0B88,
            cache_control: 0,
            irq: Interrupts::default(),
            dma: Dma::default(),
            timers: Timers::default(),
            gpu: Gpu::default(),
            cdrom: Cdrom::default(),
            spu: Spu::default(),
            sio: Sio::default(),
        };
        bus.reschedule();
        bus
    }

    pub(crate) fn icache_enabled(&self) -> bool {
        self.cache_control & (1 << 11) != 0
    }

    /// Custo de buscar uma instrução fora do cache.
    pub(crate) fn fetch_ticks(&self, addr: u32) -> u64 {
        let p = physical(addr);
        if (BIOS..BIOS_END).contains(&p) {
            BIOS_FETCH_TICKS
        } else {
            RAM_FETCH_TICKS
        }
    }

    /// Instrução em `addr`, sem passar tempo. Código fora da RAM e do BIOS
    /// lê 0 (NOP).
    pub(crate) fn peek_code(&self, addr: u32) -> u32 {
        let p = physical(addr);
        if p < RAM_END {
            load::<4>(&self.ram[..], (p as usize) & (RAM_SIZE - 4))
        } else if (BIOS..BIOS_END).contains(&p) {
            load::<4>(&self.bios[..], (p - BIOS) as usize & !3)
        } else {
            0
        }
    }

    /// Busca de instrução. Na RAM e no BIOS é a leitura normal; em alguns
    /// registradores de I/O (SPU, DMA) a CPU executa o valor lido, como no
    /// hardware; no scratchpad e no resto do I/O há erro de barramento
    /// (`None`).
    pub(crate) fn fetch(&mut self, addr: u32) -> Option<u32> {
        let p = physical(addr);
        if p < RAM_END || (BIOS..BIOS_END).contains(&p) {
            return Some(self.peek_code(p));
        }
        let offset = p.wrapping_sub(IO);
        match offset {
            0x080..=0x0FF | 0xC00..=0xFFF => Some(self.io_read(p, 4)),
            _ => None,
        }
    }

    /// Palavra da memória, sem passar tempo nem efeitos de I/O.
    pub(crate) fn peek32(&self, addr: u32) -> u32 {
        let p = physical(addr) & !3;
        if (SCRATCHPAD..SCRATCHPAD + SCRATCHPAD_SIZE as u32).contains(&p) {
            load::<4>(&self.scratchpad[..], (p - SCRATCHPAD) as usize)
        } else {
            self.peek_code(p)
        }
    }

    pub(crate) fn read8(&mut self, addr: u32) -> u32 {
        self.read::<1>(addr)
    }

    pub(crate) fn read16(&mut self, addr: u32) -> u32 {
        self.read::<2>(addr)
    }

    pub(crate) fn read32(&mut self, addr: u32) -> u32 {
        self.read::<4>(addr)
    }

    pub(crate) fn write8(&mut self, addr: u32, value: u32) {
        self.write::<1>(addr, value);
    }

    pub(crate) fn write16(&mut self, addr: u32, value: u32) {
        self.write::<2>(addr, value);
    }

    pub(crate) fn write32(&mut self, addr: u32, value: u32) {
        self.write::<4>(addr, value);
    }

    fn read<const W: usize>(&mut self, addr: u32) -> u32 {
        let p = physical(addr);
        if p < RAM_END {
            self.now += RAM_TICKS;
            return load::<W>(&self.ram[..], (p as usize) & (RAM_SIZE - 1));
        }
        if (SCRATCHPAD..SCRATCHPAD + SCRATCHPAD_SIZE as u32).contains(&p) {
            return load::<W>(&self.scratchpad[..], (p - SCRATCHPAD) as usize);
        }
        if (BIOS..BIOS_END).contains(&p) {
            self.now += BIOS_TICKS;
            return load::<W>(&self.bios[..], (p - BIOS) as usize);
        }
        if (IO..IO_END).contains(&p) {
            self.now += IO_TICKS;
            return self.io_read(p, W);
        }
        if p == CACHE_CONTROL {
            return self.cache_control;
        }
        if (EXPANSION_1..EXPANSION_2_END).contains(&p) {
            // Nada conectado: barramento em 1.
            return u32::MAX >> (32 - 8 * W as u32);
        }
        0
    }

    fn write<const W: usize>(&mut self, addr: u32, value: u32) {
        let p = physical(addr);
        if p < RAM_END {
            store::<W>(&mut self.ram[..], (p as usize) & (RAM_SIZE - 1), value);
        } else if (SCRATCHPAD..SCRATCHPAD + SCRATCHPAD_SIZE as u32).contains(&p) {
            store::<W>(&mut self.scratchpad[..], (p - SCRATCHPAD) as usize, value);
        } else if (IO..IO_END).contains(&p) {
            self.io_write(p, value, W);
        } else if p == CACHE_CONTROL {
            self.cache_control = value;
        }
    }

    fn io_read(&mut self, p: u32, width: usize) -> u32 {
        let offset = p - IO;
        let shift = (offset & 3) * 8;
        match offset {
            0x000..=0x023 => self.memory_control[(offset / 4) as usize] >> shift,
            0x040..=0x04F => {
                self.sync_sio();
                self.sio.read(offset - 0x40)
            }
            0x060..=0x063 => self.ram_size >> shift,
            0x070..=0x073 => u32::from(self.irq.status()) >> shift,
            0x074..=0x077 => u32::from(self.irq.mask()) >> shift,
            0x080..=0x0FF => self.dma.read((offset - 0x80) & !3) >> shift,
            0x100..=0x12F => {
                self.sync_devices();
                self.timers.read(offset - 0x100)
            }
            // Registradores de 8 bits: leituras maiores repetem o byte.
            // A fila de dados também é lida em 16 bits.
            0x802 if width == 2 => {
                self.sync_cdrom();
                u32::from(self.cdrom.read(2)) | u32::from(self.cdrom.read(2)) << 8
            }
            0x800..=0x803 => {
                self.sync_cdrom();
                (u32::from(self.cdrom.read(offset & 3)) * 0x0101_0101) >> (32 - 8 * width as u32)
            }
            0x810..=0x813 => self.gpu.read(),
            0x814..=0x817 => {
                self.sync_devices();
                self.gpu.status() >> shift
            }
            // MDEC: parado e com as filas vazias.
            0x820..=0x823 => 0,
            0x824..=0x827 => 0x8004_0000 >> shift,
            0xC00..=0xFFF => {
                let low = u32::from(self.spu.read16(offset & !1 & 0x3FF));
                if width == 4 {
                    low | u32::from(self.spu.read16((offset + 2) & 0x3FF)) << 16
                } else {
                    low >> ((offset & 1) * 8)
                }
            }
            _ => 0,
        }
    }

    fn io_write(&mut self, p: u32, value: u32, width: usize) {
        let offset = p - IO;
        match offset {
            0x000..=0x023 => self.memory_control[(offset / 4) as usize] = value,
            0x040..=0x04F => {
                self.sync_sio();
                self.sio.write(offset - 0x40, value, self.now);
                self.reschedule();
            }
            0x060..=0x063 => self.ram_size = value,
            0x070..=0x073 => self.irq.acknowledge(value as u16),
            0x074..=0x077 => self.irq.set_mask(value as u16),
            0x080..=0x0FF => {
                let (start, rising) = self.dma.write((offset - 0x80) & !3, value);
                if rising {
                    self.irq.request(Irq::Dma);
                }
                if let Some(channel) = start {
                    self.run_dma(channel);
                }
            }
            0x100..=0x12F => {
                self.sync_devices();
                self.timers.write(offset - 0x100, value);
                self.reschedule();
            }
            0x800..=0x803 => {
                self.sync_cdrom();
                for i in 0..width as u32 {
                    self.cdrom
                        .write((offset + i) & 3, (value >> (8 * i)) as u8, self.now);
                }
                self.raise_cdrom_irq();
                self.reschedule();
            }
            0x810..=0x813 => {
                self.gpu.gp0(value);
                if self.gpu.take_irq() {
                    self.irq.request(Irq::Gpu);
                }
            }
            0x814..=0x817 => {
                self.sync_devices();
                self.gpu.gp1(value);
                self.reschedule();
            }
            0xC00..=0xFFF => {
                self.spu.write16(offset & !1 & 0x3FF, value as u16);
                if width == 4 {
                    self.spu.write16((offset + 2) & 0x3FF, (value >> 16) as u16);
                }
            }
            _ => {}
        }
    }

    fn ram_word(&self, addr: u32) -> u32 {
        load::<4>(&self.ram[..], (addr as usize) & (RAM_SIZE - 4))
    }

    fn set_ram_word(&mut self, addr: u32, value: u32) {
        store::<4>(&mut self.ram[..], (addr as usize) & (RAM_SIZE - 4), value);
    }

    /// Executa a transferência do canal de uma vez. A CPU fica parada pelo
    /// tempo equivalente (uma palavra por ciclo).
    fn run_dma(&mut self, ch: usize) {
        let channel = self.dma.channels[ch];
        let mut addr = channel.madr & 0x1F_FFFC;
        let mut cycles = 0;
        if channel.sync_mode() == 2 {
            // Lista ligada (GPU): cabeçalho com o tamanho e o próximo nó.
            // O limite evita travar numa lista circular.
            for _ in 0..RAM_SIZE / 4 {
                let header = self.ram_word(addr);
                let count = header >> 24;
                for k in 1..=count {
                    let word = self.ram_word(addr.wrapping_add(4 * k));
                    self.gpu.gp0(word);
                }
                cycles += u64::from(count) + 1;
                if header & 0x80_0000 != 0 {
                    break;
                }
                addr = header & 0x1F_FFFC;
            }
            self.dma.channels[ch].madr = 0x00FF_FFFF;
        } else {
            let words = channel.word_count();
            let step = if channel.decrement() {
                4u32.wrapping_neg()
            } else {
                4
            };
            for n in 0..words {
                if ch == dma::OTC {
                    let value = if n + 1 == words {
                        0x00FF_FFFF
                    } else {
                        addr.wrapping_sub(4) & 0x1F_FFFF
                    };
                    self.set_ram_word(addr, value);
                } else if channel.reads_ram() {
                    let word = self.ram_word(addr);
                    match ch {
                        dma::GPU => self.gpu.gp0(word),
                        dma::SPU => self.spu.dma_write(word),
                        _ => {}
                    }
                } else {
                    let word = match ch {
                        dma::GPU => self.gpu.read(),
                        dma::CDROM => self.cdrom.dma_read(),
                        dma::SPU => self.spu.dma_read(),
                        _ => 0,
                    };
                    self.set_ram_word(addr, word);
                }
                addr = addr.wrapping_add(step) & 0x1F_FFFC;
            }
            cycles += u64::from(words);
            if channel.sync_mode() == 1 {
                self.dma.channels[ch].madr = addr;
                self.dma.channels[ch].bcr &= 0xFFFF;
            }
        }
        self.now += cycles;
        if self.gpu.take_irq() {
            self.irq.request(Irq::Gpu);
        }
        if self.dma.finish(ch) {
            self.irq.request(Irq::Dma);
        }
    }

    fn raise_timer_irqs(irq: &mut Interrupts, irqs: TimerIrqs) {
        const TIMER_IRQS: [Irq; 3] = [Irq::Timer0, Irq::Timer1, Irq::Timer2];
        for (n, timer_irq) in TIMER_IRQS.into_iter().enumerate() {
            if irqs & (1 << n) != 0 {
                irq.request(timer_irq);
            }
        }
    }

    /// Atualiza os dispositivos com eventos até agora e agenda o próximo.
    pub(crate) fn run_events(&mut self) {
        self.sync_devices();
        self.reschedule();
    }

    fn sync_devices(&mut self) {
        let now = self.now;
        let timers = &mut self.timers;
        let irq = &mut self.irq;
        self.gpu.sync(now, |gpu, event, at| {
            let irqs = timers.sync(at, gpu.dot_divider());
            Self::raise_timer_irqs(irq, irqs);
            match event {
                VideoEvent::HblankStart => {
                    let irqs = timers.hblank();
                    Self::raise_timer_irqs(irq, irqs);
                }
                VideoEvent::HblankEnd => timers.hblank_end(),
                VideoEvent::Vblank(entering) => {
                    timers.vblank(entering);
                    if entering {
                        irq.request(Irq::Vblank);
                    }
                }
            }
        });
        let irqs = self.timers.sync(now, self.gpu.dot_divider());
        Self::raise_timer_irqs(&mut self.irq, irqs);
        self.sync_sio();
        self.sync_cdrom();
    }

    fn sync_cdrom(&mut self) {
        self.cdrom.sync(self.now);
        self.raise_cdrom_irq();
    }

    fn raise_cdrom_irq(&mut self) {
        if self.cdrom.take_irq() {
            self.irq.request(Irq::Cdrom);
        }
    }

    fn sync_sio(&mut self) {
        if self.sio.sync(self.now) {
            self.irq.request(Irq::Controller);
        }
    }

    fn reschedule(&mut self) {
        let mut next = self.gpu.next_event();
        if let Some(cycles) = self.timers.cycles_to_next_irq(self.gpu.dot_divider()) {
            next = next.min(self.now + cycles);
        }
        for at in [self.sio.next_event(), self.cdrom.next_event()]
            .into_iter()
            .flatten()
        {
            next = next.min(at);
        }
        self.next_event = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus() -> Bus {
        Bus::new(Box::new([0; BIOS_SIZE]))
    }

    #[test]
    fn segments_mirror_the_same_ram() {
        let mut bus = bus();
        bus.write32(0x0000_1000, 0xDEAD_BEEF);
        assert_eq!(bus.read32(0x8000_1000), 0xDEAD_BEEF);
        assert_eq!(bus.read32(0xA000_1000), 0xDEAD_BEEF);
        assert_eq!(bus.read32(0x0020_1000), 0xDEAD_BEEF, "espelho de 2 MB");
        assert_eq!(bus.read16(0x8000_1002), 0xDEAD);
        assert_eq!(bus.read8(0x8000_1000), 0xEF);
    }

    #[test]
    fn bios_is_read_only_and_expansion_reads_ones() {
        let mut rom = Box::new([0; BIOS_SIZE]);
        rom[..4].copy_from_slice(&[1, 2, 3, 4]);
        let mut bus = Bus::new(rom);
        bus.write32(0xBFC0_0000, 0);
        assert_eq!(bus.read32(0xBFC0_0000), 0x0403_0201);
        assert_eq!(bus.read8(0x1F00_0084), 0xFF);
    }

    #[test]
    fn otc_dma_builds_the_ordering_table() {
        let mut bus = bus();
        bus.write32(0x1F80_10F0, 0x0800_0000);
        bus.write32(0x1F80_10E0, 0x100);
        bus.write32(0x1F80_10E4, 3);
        bus.write32(0x1F80_10E8, 0x1100_0002);
        assert_eq!(bus.read32(0x100), 0xFC);
        assert_eq!(bus.read32(0xFC), 0xF8);
        assert_eq!(bus.read32(0xF8), 0x00FF_FFFF);
        assert_eq!(bus.read32(0x1F80_10E8) & (1 << 24), 0, "terminou");
    }

    #[test]
    fn vblank_raises_irq_once_per_frame() {
        let mut bus = bus();
        bus.write32(0x1F80_1074, 1);
        let mut vblanks = 0;
        while bus.now < 571_212 * 3 {
            bus.now = bus.next_event;
            bus.run_events();
            if bus.irq.pending() {
                vblanks += 1;
                bus.write32(0x1F80_1070, 0);
            }
        }
        assert_eq!(vblanks, 3);
    }

    #[test]
    fn timer_irq_is_scheduled_on_time() {
        let mut bus = bus();
        bus.write32(0x1F80_1074, 1 << Irq::Timer2 as u32);
        bus.write32(0x1F80_1128, 1000);
        bus.write32(0x1F80_1124, 0x18); // reset e IRQ no alvo
        let start = bus.now;
        while !bus.irq.pending() {
            bus.now = bus.next_event;
            bus.run_events();
        }
        assert_eq!(bus.now - start, 1000);
    }
}
