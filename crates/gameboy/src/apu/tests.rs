use super::*;

const RATE: u32 = 44_100;

/// APU com um contador de timer simulado (para o frame sequencer).
struct Rig {
    apu: Apu,
    counter: u16,
}

impl Rig {
    fn new() -> Self {
        let mut apu = Apu::new(RATE);
        apu.write_register(0xFF26, 0x00);
        apu.write_register(0xFF26, 0x80);
        apu.write_register(0xFF24, 0x77);
        apu.write_register(0xFF25, 0xFF);
        Self { apu, counter: 0 }
    }

    fn reg(&mut self, addr: u16, value: u8) {
        self.apu.write_register(addr, value);
    }

    fn run(&mut self, seconds: f64) {
        let cycles = (f64::from(CPU_CLOCK / 4) * seconds) as u32;
        for _ in 0..cycles {
            self.counter = self.counter.wrapping_add(4);
            self.apu.tick(self.counter & 0x1000 != 0);
        }
    }

    /// Roda e devolve o canal esquerdo, sem o nível médio.
    fn render(&mut self, seconds: f64) -> Vec<f32> {
        self.apu.samples.clear();
        self.run(seconds);
        let mut out = Vec::new();
        self.apu.drain(&mut out);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let mean = left.iter().sum::<f32>() / left.len().max(1) as f32;
        left.iter().map(|s| s - mean).collect()
    }

    fn status(&self) -> u8 {
        self.apu.read(0xFF26) & 0x0F
    }
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count()
}

/// Canal 2 em 440 Hz: freq = 2048 − 131072 / 440 ≈ 1750.
fn square_440(rig: &mut Rig) {
    rig.reg(0xFF16, 0x80); // duty 50%
    rig.reg(0xFF17, 0xF0); // volume 15, sem envelope
    rig.reg(0xFF18, (1750 & 0xFF) as u8);
    rig.reg(0xFF19, 0x80 | (1750 >> 8) as u8);
}

#[test]
fn square_channel_plays_requested_frequency() {
    let mut rig = Rig::new();
    square_440(&mut rig);
    assert_eq!(rig.status(), 0b0010);
    let samples = rig.render(1.0);
    assert!(peak(&samples) > 0.1);
    let hz = zero_crossings(&samples) as f64 / 2.0;
    assert!((430.0..=450.0).contains(&hz), "{hz} Hz");
}

#[test]
fn envelope_fades_out() {
    let mut rig = Rig::new();
    square_440(&mut rig);
    rig.reg(0xFF17, 0xF1); // diminui a cada 1/64 s
    rig.reg(0xFF19, 0x80 | (1750 >> 8) as u8);
    let early = peak(&rig.render(0.05));
    rig.run(0.3);
    let late = peak(&rig.render(0.05));
    assert!(late < early * 0.1, "{early} {late}");
}

#[test]
fn length_counter_stops_channel() {
    let mut rig = Rig::new();
    rig.reg(0xFF16, 0x80 | 0x3F); // duração 1/256 s
    rig.reg(0xFF17, 0xF0);
    rig.reg(0xFF19, 0xC7);
    rig.run(0.02);
    assert_eq!(rig.status() & 0b0010, 0);
}

#[test]
fn dac_off_disables_channel() {
    let mut rig = Rig::new();
    square_440(&mut rig);
    rig.reg(0xFF17, 0x00);
    assert_eq!(rig.status() & 0b0010, 0);
}

#[test]
fn panning_routes_channels() {
    let mut rig = Rig::new();
    square_440(&mut rig);
    rig.reg(0xFF25, 0x02); // canal 2 só à direita
    rig.apu.samples.clear();
    rig.run(0.05);
    let mut out = Vec::new();
    rig.apu.drain(&mut out);
    let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
    let right: Vec<f32> = out.chunks(2).map(|f| f[1]).collect();
    let spread = |v: &[f32]| {
        v.iter().fold(f32::MIN, |m, &s| m.max(s)) - v.iter().fold(f32::MAX, |m, &s| m.min(s))
    };
    assert!(spread(&left) < 1e-6);
    assert!(spread(&right) > 0.1);
}

#[test]
fn sweep_overflow_disables_channel_1() {
    let mut rig = Rig::new();
    rig.reg(0xFF10, 0x11); // período 1, soma, shift 1
    rig.reg(0xFF12, 0xF0);
    rig.reg(0xFF13, 0x00);
    rig.reg(0xFF14, 0x87); // freq 0x700: 0x700 + 0x380 > 2047
    assert_eq!(rig.status() & 1, 0, "estouro já no trigger");

    rig.reg(0xFF14, 0x84); // 0x400: sobe até estourar
    assert_eq!(rig.status() & 1, 1);
    rig.run(0.1);
    assert_eq!(rig.status() & 1, 0);
}

#[test]
fn wave_channel_plays_wave_ram() {
    let mut rig = Rig::new();
    for i in 0..16 {
        rig.reg(0xFF30 + i, if i < 8 { 0xFF } else { 0x00 });
    }
    rig.reg(0xFF1A, 0x80);
    rig.reg(0xFF1C, 0x20); // volume 100%
    // 32 amostras por ciclo: freq = 2048 − 65536 / 220.
    let freq = 2048 - 65_536 / 220;
    rig.reg(0xFF1D, (freq & 0xFF) as u8);
    rig.reg(0xFF1E, 0x80 | (freq >> 8) as u8);
    assert_eq!(rig.status() & 0b0100, 0b0100);
    let hz = zero_crossings(&rig.render(1.0)) as f64 / 2.0;
    assert!((210.0..=230.0).contains(&hz), "{hz} Hz");
}

#[test]
fn noise_channel_produces_sound() {
    let mut rig = Rig::new();
    rig.reg(0xFF21, 0xF0);
    rig.reg(0xFF22, 0x20);
    rig.reg(0xFF23, 0x80);
    assert_eq!(rig.status() & 0b1000, 0b1000);
    assert!(peak(&rig.render(0.1)) > 0.05);
}

#[test]
fn power_off_clears_registers_but_keeps_wave_ram() {
    let mut rig = Rig::new();
    square_440(&mut rig);
    rig.reg(0xFF30, 0x12);
    rig.reg(0xFF26, 0x00);
    assert_eq!(rig.apu.read(0xFF26), 0x70);
    assert_eq!(rig.apu.read(0xFF17), 0x00);
    assert_eq!(rig.apu.read(0xFF30), 0x12);
    rig.reg(0xFF17, 0xF0);
    assert_eq!(rig.apu.read(0xFF17), 0x00, "escrita ignorada desligada");
}

#[test]
fn unused_bits_read_as_one() {
    let rig = Rig::new();
    assert_eq!(rig.apu.read(0xFF10), 0x80);
    assert_eq!(rig.apu.read(0xFF13), 0xFF);
    assert_eq!(rig.apu.read(0xFF1A), 0x7F);
    assert_eq!(rig.apu.read(0xFF27), 0xFF);
}

#[test]
fn produces_stereo_at_requested_rate() {
    let mut rig = Rig::new();
    rig.apu = Apu::new(48_000);
    rig.run(1.0);
    let mut out = Vec::new();
    rig.apu.drain(&mut out);
    assert!(out.len().abs_diff(96_000) <= 4, "{}", out.len());
}

#[test]
fn state_round_trip_continues_identically() {
    let mut a = Rig::new();
    square_440(&mut a);
    a.reg(0xFF21, 0xF3);
    a.reg(0xFF22, 0x31);
    a.reg(0xFF23, 0xC0);
    a.run(0.03);
    let mut w = Writer::default();
    a.apu.save_state(&mut w);

    let mut b = Rig::new();
    b.counter = a.counter;
    let mut r = Reader::new(&w.bytes);
    b.apu.load_state(&mut r).expect("estado válido");
    assert_eq!(r.finish(), Ok(()));
    a.apu.samples.clear();
    a.apu.phase = 0;
    a.apu.acc = [0.0; 2];
    a.apu.acc_count = 0;
    b.apu.samples.clear();
    assert_eq!(a.render(0.05), b.render(0.05));
}
