//! SPU (0x1F801C00–0x1F801FFF): 24 vozes ADPCM com ADSR, ruído, modulação
//! de pitch, reverb e 512 KB de RAM própria.
//!
//! Gera uma amostra estéreo a cada 768 ciclos da CPU (44,1 kHz), sob
//! demanda: o barramento chama [`Spu::sync`] antes de cada acesso e a cada
//! linha de vídeo. Uma voz que lê o endereço do IRQA (ou uma transferência
//! ou captura nele) gera a IRQ 9.
//!
//! O som do CD (CD-DA e XA-ADPCM) chega já a 44,1 kHz numa fila do
//! controlador do CD-ROM: uma amostra por amostra gerada aqui.

mod envelope;
mod reverb;
mod voice;

use std::collections::VecDeque;

use envelope::Volume;
use reverb::Reverb;
use voice::Voice;

/// Ciclos da CPU por amostra.
pub(crate) const TICKS_PER_SAMPLE: u64 = 768;
pub(crate) const RAM_SIZE: usize = 512 * 1024;
const RAM_HALFWORDS: usize = RAM_SIZE / 2;
const VOICES: usize = 24;
/// Áudio guardado sem ninguém ler (meio segundo); o mais antigo é
/// descartado.
const MAX_PENDING: usize = 44_100;

const REGISTERS: usize = 0x200;

// Registradores de controle (índice em meias-palavras a partir de 0x1C00).
const MVOL: usize = 0x180 / 2;
const EVOL: usize = 0x184 / 2;
const KON: usize = 0x188 / 2;
const KOFF: usize = 0x18C / 2;
const PMON: usize = 0x190 / 2;
const NON: usize = 0x194 / 2;
const EON: usize = 0x198 / 2;
const ENDX: usize = 0x19C / 2;
const ESA: usize = 0x1A2 / 2;
const IRQA: usize = 0x1A4 / 2;
const TSA: usize = 0x1A6 / 2;
const DATA: usize = 0x1A8 / 2;
const ATTR: usize = 0x1AA / 2;
const RAM_CTRL: usize = 0x1AC / 2;
const STATX: usize = 0x1AE / 2;
const CD_VOLUME: usize = 0x1B0 / 2;
const MVOLX: usize = 0x1B8 / 2;
const REVERB: usize = 0x1C0 / 2;
const VOLX: usize = 0x200 / 2;

const ATTR_ENABLE: u16 = 1 << 15;
const ATTR_UNMUTE: u16 = 1 << 14;
const ATTR_REVERB: u16 = 1 << 7;
const ATTR_IRQ: u16 = 1 << 6;
const ATTR_CD_REVERB: u16 = 1 << 2;
const ATTR_CD_AUDIO: u16 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Transfer {
    Stop,
    Manual,
    DmaWrite,
    DmaRead,
}

/// Gerador de ruído compartilhado pelas vozes com NON.
#[derive(Clone, Copy, Default)]
struct Noise {
    level: u16,
    timer: i32,
}

impl Noise {
    fn tick(&mut self, attr: u16) {
        let step = i32::from((attr >> 8) & 3) + 4;
        let shift = (attr >> 10) & 0xF;
        self.timer -= step;
        if self.timer >= 0 {
            return;
        }
        let l = self.level;
        let parity = (l >> 15 ^ l >> 12 ^ l >> 11 ^ l >> 10 ^ 1) & 1;
        self.level = l << 1 | parity;
        let reload = 0x20000 >> shift;
        self.timer += reload;
        if self.timer < 0 {
            self.timer += reload;
        }
    }
}

#[derive(Clone)]
pub(crate) struct Spu {
    /// Valores escritos, devolvidos nas leituras dos registradores comuns.
    registers: Box<[u16; REGISTERS]>,
    ram: Box<[u16; RAM_HALFWORDS]>,
    voices: [Voice; VOICES],
    main_volume: [Volume; 2],
    reverb: Reverb,
    noise: Noise,
    endx: u32,
    /// Endereço atual da transferência, em bytes.
    transfer_address: u32,
    fifo: VecDeque<u16>,
    irq: bool,
    irq_rising: bool,
    /// Posição nos buffers de captura (0..0x1FF).
    capture: u16,
    next_tick: u64,
    output: Vec<i16>,
}

impl Default for Spu {
    fn default() -> Self {
        Self {
            registers: Box::new([0; REGISTERS]),
            ram: vec![0; RAM_HALFWORDS]
                .try_into()
                .expect("tamanho da RAM do SPU"),
            voices: std::array::from_fn(|_| Voice::default()),
            main_volume: [Volume::default(); 2],
            reverb: Reverb::default(),
            noise: Noise::default(),
            endx: 0,
            transfer_address: 0,
            fifo: VecDeque::with_capacity(32),
            irq: false,
            irq_rising: false,
            capture: 0,
            next_tick: TICKS_PER_SAMPLE,
            output: Vec::new(),
        }
    }
}

impl Spu {
    fn attr(&self) -> u16 {
        self.registers[ATTR]
    }

    fn transfer_mode(&self) -> Transfer {
        match (self.attr() >> 4) & 3 {
            0 => Transfer::Stop,
            1 => Transfer::Manual,
            2 => Transfer::DmaWrite,
            _ => Transfer::DmaRead,
        }
    }

    /// Flags de 24 bits espalhados em dois registradores.
    fn flags(&self, index: usize) -> u32 {
        u32::from(self.registers[index]) | u32::from(self.registers[index + 1]) << 16
    }

    // --- Registradores ---

    pub(crate) fn read16(&self, offset: u32) -> u16 {
        let index = (offset as usize / 2) % REGISTERS;
        match index {
            0..0xC0 => {
                let voice = &self.voices[index / 8];
                match index % 8 {
                    6 => voice.adsr.level as u16,
                    7 => voice.repeat,
                    _ => self.registers[index],
                }
            }
            ENDX => self.endx as u16,
            i if i == ENDX + 1 => (self.endx >> 16) as u16,
            STATX => self.status(),
            MVOLX => self.main_volume[0].level as u16,
            i if i == MVOLX + 1 => self.main_volume[1].level as u16,
            VOLX..0x130 => {
                let n = index - VOLX;
                self.voices[n / 2].volume[n % 2].level as u16
            }
            _ => self.registers[index],
        }
    }

    /// STATX: modo atual (bits 0–5), IRQ (6), pedidos de DMA (7–9) e a
    /// metade dos buffers de captura (11).
    fn status(&self) -> u16 {
        let attr = self.attr();
        let mode = self.transfer_mode();
        attr & 0x3F
            | u16::from(self.irq) << 6
            | (attr & 0x20) << 2
            | u16::from(mode == Transfer::DmaWrite) << 8
            | u16::from(mode == Transfer::DmaRead) << 9
            | u16::from(self.capture >= 0x100) << 11
    }

    pub(crate) fn write16(&mut self, offset: u32, value: u16) {
        let index = (offset as usize / 2) % REGISTERS;
        self.registers[index] = value;
        match index {
            0..0xC0 => self.write_voice(index / 8, index % 8, value),
            MVOL | 0xC1 => self.main_volume[index - MVOL].set(value),
            KON | 0xC5 => self.key_on(index - KON, value),
            KOFF | 0xC7 => {
                for v in Self::voices_in(index - KOFF, value) {
                    self.voices[v].adsr.key_off();
                }
            }
            ESA => self.reverb.set_base(value),
            TSA => self.transfer_address = u32::from(value) * 8,
            DATA => {
                if self.fifo.len() < 32 {
                    self.fifo.push_back(value);
                }
                if self.transfer_mode() == Transfer::Manual {
                    self.flush_fifo();
                }
            }
            ATTR => {
                if value & ATTR_IRQ == 0 {
                    self.irq = false;
                }
                if self.transfer_mode() == Transfer::Manual {
                    self.flush_fifo();
                }
            }
            REVERB.. if index < REVERB + 32 => self.reverb.registers[index - REVERB] = value,
            _ => {}
        }
    }

    fn write_voice(&mut self, n: usize, register: usize, value: u16) {
        let voice = &mut self.voices[n];
        match register {
            0 | 1 => voice.volume[register].set(value),
            2 => voice.pitch = value,
            3 => voice.start = value,
            4 => voice.adsr.config = voice.adsr.config & 0xFFFF_0000 | u32::from(value),
            5 => voice.adsr.config = voice.adsr.config & 0xFFFF | u32::from(value) << 16,
            6 => voice.adsr.level = value as i16,
            _ => voice.set_repeat(value),
        }
    }

    /// Vozes ligadas em `value`, no registrador baixo (0) ou alto (1).
    fn voices_in(high: usize, value: u16) -> impl Iterator<Item = usize> {
        (0..16)
            .filter(move |bit| value & (1 << bit) != 0)
            .map(move |bit| bit + 16 * high)
            .filter(|&v| v < VOICES)
    }

    fn key_on(&mut self, high: usize, value: u16) {
        for v in Self::voices_in(high, value) {
            self.voices[v].key_on(&self.ram);
            self.endx &= !(1 << v);
        }
    }

    // --- RAM e transferências ---

    fn flush_fifo(&mut self) {
        while let Some(value) = self.fifo.pop_front() {
            self.write_ram(value);
        }
    }

    fn write_ram(&mut self, value: u16) {
        let at = self.transfer_address;
        self.check_irq(at, 2);
        self.ram[at as usize / 2] = value;
        self.transfer_address = (at + 2) % RAM_SIZE as u32;
    }

    fn read_ram(&mut self) -> u16 {
        let at = self.transfer_address;
        self.check_irq(at, 2);
        self.transfer_address = (at + 2) % RAM_SIZE as u32;
        self.ram[at as usize / 2]
    }

    /// Palavra escrita pelo DMA (canal 4).
    pub(crate) fn dma_write(&mut self, word: u32) {
        self.write_ram(word as u16);
        self.write_ram((word >> 16) as u16);
    }

    /// Palavra lida pelo DMA (canal 4).
    pub(crate) fn dma_read(&mut self) -> u32 {
        let low = self.read_ram();
        u32::from(low) | u32::from(self.read_ram()) << 16
    }

    /// IRQ 9 quando um acesso de `len` bytes em `address` passa pelo IRQA.
    fn check_irq(&mut self, address: u32, len: u32) {
        let attr = self.attr();
        if attr & (ATTR_ENABLE | ATTR_IRQ) != ATTR_ENABLE | ATTR_IRQ || self.irq {
            return;
        }
        let target = u32::from(self.registers[IRQA]) * 8;
        if (address..address + len).contains(&target) {
            self.irq = true;
            self.irq_rising = true;
        }
    }

    /// Se a IRQ 9 subiu desde a última consulta.
    pub(crate) fn take_irq(&mut self) -> bool {
        std::mem::take(&mut self.irq_rising)
    }

    // --- Som ---

    /// Gera as amostras até `now`, consumindo uma amostra do som do CD
    /// (`cd`) por vez; sem som na fila, o CD fica em silêncio.
    pub(crate) fn sync(&mut self, now: u64, cd: &mut VecDeque<[i16; 2]>) {
        while self.next_tick <= now {
            self.next_tick += TICKS_PER_SAMPLE;
            self.tick(cd.pop_front().unwrap_or_default());
        }
    }

    fn tick(&mut self, cd: [i16; 2]) {
        let attr = self.attr();
        self.noise.tick(attr);
        let (pmon, non, eon) = (self.flags(PMON), self.flags(NON), self.flags(EON));
        let noise = self.noise.level as i16;
        let mut mix = [0i32; 2];
        let mut reverb_in = [0i32; 2];
        let mut previous = 0;
        for v in 0..VOICES {
            let bit = 1 << v;
            let modulator = (v > 0 && pmon & bit != 0).then_some(previous);
            let voice = &mut self.voices[v];
            let ended = voice.tick(&self.ram, modulator, (non & bit != 0).then_some(noise));
            if ended {
                self.endx |= bit;
            }
            let (address, output) = (voice.address, voice.output);
            let volumes = [voice.volume[0].level, voice.volume[1].level];
            previous = output;
            self.check_irq(u32::from(address) * 8, 16);
            for side in 0..2 {
                let sample = (i32::from(output) * i32::from(volumes[side])) >> 15;
                mix[side] += sample;
                if eon & bit != 0 {
                    reverb_in[side] += sample;
                }
            }
        }
        self.capture(cd);
        // O som do CD não depende do bit de ligado nem do mudo do SPU.
        let mut cd_out = [0; 2];
        for side in 0..2 {
            let volume = i32::from(self.registers[CD_VOLUME + side] as i16);
            let sample = (i32::from(cd[side]) * volume) >> 15;
            if attr & ATTR_CD_AUDIO != 0 {
                cd_out[side] = sample;
                if attr & ATTR_CD_REVERB != 0 {
                    reverb_in[side] += sample;
                }
            }
        }

        let reverb = self.reverb.tick(
            &mut self.ram,
            reverb_in.map(|s| s.clamp(-0x8000, 0x7FFF)),
            attr & ATTR_REVERB != 0,
        );
        let unmuted = attr & (ATTR_ENABLE | ATTR_UNMUTE) == ATTR_ENABLE | ATTR_UNMUTE;
        for side in 0..2 {
            let echo = (reverb[side] * i32::from(self.registers[EVOL + side] as i16)) >> 15;
            let sum = if unmuted { mix[side] + echo } else { 0 } + cd_out[side];
            let volume = i32::from(self.main_volume[side].level);
            let out = (sum.clamp(-0x8000, 0x7FFF) * volume) >> 15;
            self.output.push(out.clamp(-0x8000, 0x7FFF) as i16);
            self.main_volume[side].tick();
        }
        if self.output.len() > 2 * MAX_PENDING {
            self.output.drain(..MAX_PENDING);
        }
    }

    /// Buffers de captura: o CD antes do volume e as vozes 1 e 3, depois
    /// do envelope.
    fn capture(&mut self, cd: [i16; 2]) {
        let position = u32::from(self.capture) * 2;
        let samples = [cd[0], cd[1], self.voices[1].output, self.voices[3].output];
        for (buffer, sample) in samples.into_iter().enumerate() {
            let at = buffer as u32 * 0x400 + position;
            self.ram[at as usize / 2] = sample as u16;
            if self.registers[RAM_CTRL] & 0xC != 0 {
                self.check_irq(at, 2);
            }
        }
        self.capture = (self.capture + 1) & 0x1FF;
    }

    /// Move as amostras geradas (estéreo intercalado, 44,1 kHz) para `out`,
    /// em -1..1.
    pub(crate) fn drain(&mut self, out: &mut Vec<f32>) {
        out.extend(self.output.drain(..).map(|s| f32::from(s) / 32768.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: u16 = ATTR_ENABLE | ATTR_UNMUTE;

    fn write(spu: &mut Spu, register: usize, value: u16) {
        spu.write16(register as u32 * 2, value);
    }

    /// SPU ligado, com volume máximo e um bloco em loop em 0x1000.
    fn playing(nibbles: u16) -> Spu {
        let mut spu = Spu::default();
        write(&mut spu, ATTR, ON);
        write(&mut spu, MVOL, 0x3FFF);
        write(&mut spu, MVOL + 1, 0x3FFF);
        write(&mut spu, RAM_CTRL, 4);
        write(&mut spu, TSA, 0x1000 / 8);
        write(&mut spu, ATTR, ON | 0x10);
        // Shift 0, filtro 0, fim com repetição e começo de loop.
        write(&mut spu, DATA, 0x0700);
        for _ in 0..7 {
            write(&mut spu, DATA, nibbles);
        }
        write(&mut spu, ATTR, ON);
        spu
    }

    #[test]
    fn manual_and_dma_transfers() {
        let mut spu = playing(0x1234);
        assert_eq!(spu.ram[0x800..0x802], [0x0700, 0x1234]);
        write(&mut spu, TSA, 0x2000 / 8);
        write(&mut spu, ATTR, ON | 0x20);
        assert_eq!(spu.read16(STATX as u32 * 2) & 0x3A0, 0x1A0, "DMA write");
        spu.dma_write(0xBBBB_AAAA);
        write(&mut spu, TSA, 0x2000 / 8);
        write(&mut spu, ATTR, ON | 0x30);
        assert_eq!(spu.read16(STATX as u32 * 2) & 0x3A0, 0x2A0, "DMA read");
        assert_eq!(spu.dma_read(), 0xBBBB_AAAA);
    }

    #[test]
    fn voice_plays_and_is_mixed() {
        let mut spu = playing(0x7777);
        write(&mut spu, 0, 0x3FFF);
        write(&mut spu, 1, 0x3FFF);
        write(&mut spu, 2, 0x1000);
        write(&mut spu, 3, 0x1000 / 8);
        write(&mut spu, 4, 0x000F);
        write(&mut spu, 5, 0x1FC0);
        write(&mut spu, KON, 1);
        spu.sync(100 * TICKS_PER_SAMPLE, &mut VecDeque::new());
        let mut out = Vec::new();
        spu.drain(&mut out);
        assert_eq!(out.len(), 200);
        // Amostras 0x7000 com envelope ~0,5 e volumes ~1,0.
        let last = &out[198..];
        assert!(last[0] > 0.4 && last[0] == last[1], "{last:?}");
        // O decay, exponencial, cai até 0x3FFF no primeiro passo.
        assert_eq!(spu.read16(0xC), 0x3FFF, "ENVX em sustain");
        assert_ne!(
            spu.read16(ENDX as u32 * 2) & 1,
            0,
            "passou pelo fim de loop"
        );

        write(&mut spu, KOFF, 1);
        spu.sync(200 * TICKS_PER_SAMPLE, &mut VecDeque::new());
        assert_eq!(spu.read16(0xC), 0, "release rápido");
        write(&mut spu, KON, 1);
        assert_eq!(spu.read16(ENDX as u32 * 2) & 1, 0, "Key On limpa o ENDX");
    }

    #[test]
    fn mute_silences_output() {
        let mut spu = playing(0x7777);
        write(&mut spu, 0, 0x3FFF);
        write(&mut spu, 2, 0x1000);
        write(&mut spu, 3, 0x1000 / 8);
        write(&mut spu, KON, 1);
        write(&mut spu, ATTR, ATTR_ENABLE);
        spu.sync(50 * TICKS_PER_SAMPLE, &mut VecDeque::new());
        let mut out = Vec::new();
        spu.drain(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn cd_audio_is_mixed_with_its_volume_and_captured() {
        let mut spu = Spu::default();
        // Mudo, mas com a entrada do CD ligada: o CD não respeita o mudo.
        write(&mut spu, ATTR, ATTR_ENABLE | ATTR_CD_AUDIO);
        write(&mut spu, MVOL, 0x3FFF);
        write(&mut spu, MVOL + 1, 0x3FFF);
        write(&mut spu, CD_VOLUME, 0x4000);
        write(&mut spu, CD_VOLUME + 1, 0x7FFF);
        let mut cd: VecDeque<_> = [[0x2000, -0x2000], [0x1000, 0x1000]].into();
        spu.sync(3 * TICKS_PER_SAMPLE, &mut cd);
        assert!(cd.is_empty());
        let mut out = Vec::new();
        spu.drain(&mut out);
        let out: Vec<i32> = out.iter().map(|&s| (s * 32768.0) as i32).collect();
        // Volume 0x4000 é metade; o principal (0x3FFF) quase 1.
        assert_eq!(
            out[..2],
            [0x0FFF, -0x2000],
            "o deslocamento arredonda para baixo"
        );
        assert_eq!(out[4..], [0, 0], "fila vazia: silêncio");
        // Captura antes do volume: esquerda em 0x000, direita em 0x400.
        assert_eq!(spu.ram[..2], [0x2000, 0x1000]);
        assert_eq!(spu.ram[0x200..0x202], [0xE000, 0x1000]);

        // Sem o bit 0, o CD só vai para a captura.
        write(&mut spu, ATTR, ATTR_ENABLE | ATTR_UNMUTE);
        let mut cd: VecDeque<_> = [[0x2000, 0x2000]].into();
        spu.sync(4 * TICKS_PER_SAMPLE, &mut cd);
        let mut out = Vec::new();
        spu.drain(&mut out);
        assert_eq!(out, [0.0, 0.0]);
        assert_eq!(spu.ram[3], 0x2000);
    }

    #[test]
    fn voice_reading_irq_address_raises_irq_once_until_acknowledged() {
        let mut spu = playing(0);
        write(&mut spu, IRQA, 0x1000 / 8);
        write(&mut spu, ATTR, ON | ATTR_IRQ);
        write(&mut spu, 3, 0x1000 / 8);
        write(&mut spu, KON, 1);
        spu.sync(TICKS_PER_SAMPLE, &mut VecDeque::new());
        assert!(spu.take_irq());
        assert_ne!(spu.read16(STATX as u32 * 2) & 0x40, 0);
        spu.sync(10 * TICKS_PER_SAMPLE, &mut VecDeque::new());
        assert!(!spu.take_irq(), "só uma vez até reconhecer");
        write(&mut spu, ATTR, ON);
        assert_eq!(spu.read16(STATX as u32 * 2) & 0x40, 0);
        write(&mut spu, ATTR, ON | ATTR_IRQ);
        spu.sync(11 * TICKS_PER_SAMPLE, &mut VecDeque::new());
        assert!(spu.take_irq());
    }

    #[test]
    fn transfer_through_irq_address_raises_irq() {
        let mut spu = Spu::default();
        write(&mut spu, IRQA, 0x3000 / 8);
        write(&mut spu, ATTR, ON | ATTR_IRQ | 0x20);
        write(&mut spu, TSA, 0x2FF8 / 8);
        for _ in 0..2 {
            spu.dma_write(0);
            assert!(!spu.take_irq());
        }
        spu.dma_write(0);
        assert!(spu.take_irq());
    }

    #[test]
    fn noise_is_a_lfsr() {
        let mut noise = Noise::default();
        let mut levels = std::collections::HashSet::new();
        // Shift 15, passo 7: avança quase a cada amostra.
        for _ in 0..2000 {
            noise.tick(0x3F00);
            levels.insert(noise.level);
        }
        assert!(levels.len() > 1000);
    }

    #[test]
    fn pending_audio_is_bounded() {
        let mut spu = Spu::default();
        spu.sync(
            10 * MAX_PENDING as u64 * TICKS_PER_SAMPLE,
            &mut VecDeque::new(),
        );
        assert!(spu.output.len() <= 2 * MAX_PENDING);
    }
}
