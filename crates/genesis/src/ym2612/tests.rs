use super::*;

const RATE: u32 = 44_100;

fn reg(ym: &mut Ym2612, part: u16, r: u8, v: u8) {
    ym.write(part * 2, r);
    ym.write(part * 2 + 1, v);
}

/// Canal 1 com algoritmo 7: só O1 audível (TL 0), os outros mudos.
fn single_operator(ym: &mut Ym2612, fnum: u16, block: u8) {
    reg(ym, 0, 0xB0, 0x07);
    for (offset, tl) in [(0x00, 0x00), (0x04, 0x7F), (0x08, 0x7F), (0x0C, 0x7F)] {
        reg(ym, 0, 0x30 + offset, 0x01); // MUL 1
        reg(ym, 0, 0x40 + offset, tl);
        reg(ym, 0, 0x50 + offset, 0x1F); // AR máximo
        reg(ym, 0, 0x60 + offset, 0x00);
        reg(ym, 0, 0x70 + offset, 0x00);
        reg(ym, 0, 0x80 + offset, 0x0F); // RR máximo
    }
    reg(ym, 0, 0xA4, (block << 3) | (fnum >> 8) as u8);
    reg(ym, 0, 0xA0, fnum as u8);
}

fn key(ym: &mut Ym2612, on: bool) {
    reg(ym, 0, 0x28, if on { 0x10 } else { 0x00 });
}

/// Roda `seconds` e devolve só o canal esquerdo.
fn render(ym: &mut Ym2612, seconds: f64) -> Vec<f32> {
    ym.run((f64::from(crate::MASTER_CLOCK) * seconds) as u32);
    let mut out = Vec::new();
    ym.drain(&mut out);
    out.chunks(2).map(|f| f[0]).collect()
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

#[test]
fn frequency_latch_applies_on_low_byte() {
    let mut ym = Ym2612::new(RATE);
    reg(&mut ym, 0, 0xA4, 0x22); // bloco 4, fnum alto 2
    assert_eq!(ym.channels[0].fnum, 0);
    reg(&mut ym, 0, 0xA0, 0x34);
    assert_eq!((ym.channels[0].block, ym.channels[0].fnum), (4, 0x234));
}

#[test]
fn silent_without_key_on() {
    let mut ym = Ym2612::new(RATE);
    single_operator(&mut ym, 1083, 4);
    assert_eq!(peak(&render(&mut ym, 0.05)), 0.0);
}

#[test]
fn tone_has_requested_frequency() {
    let mut ym = Ym2612::new(RATE);
    // 440 Hz: fnum = 440 · 2^20 / (53267 · 2^(4-1)) ≈ 1083.
    single_operator(&mut ym, 1083, 4);
    key(&mut ym, true);
    let samples = render(&mut ym, 1.0);
    assert!(peak(&samples) > 0.1);
    let hz = zero_crossings(&samples) as f64 / 2.0;
    assert!((430.0..=450.0).contains(&hz), "{hz} Hz");
}

#[test]
fn multiplier_doubles_frequency() {
    let mut ym = Ym2612::new(RATE);
    single_operator(&mut ym, 1083, 4);
    reg(&mut ym, 0, 0x30, 0x02); // MUL 2
    key(&mut ym, true);
    let hz = zero_crossings(&render(&mut ym, 1.0)) as f64 / 2.0;
    assert!((860.0..=900.0).contains(&hz), "{hz} Hz");
}

#[test]
fn total_level_attenuates() {
    let loud = {
        let mut ym = Ym2612::new(RATE);
        single_operator(&mut ym, 1083, 4);
        key(&mut ym, true);
        peak(&render(&mut ym, 0.1))
    };
    let quiet = {
        let mut ym = Ym2612::new(RATE);
        single_operator(&mut ym, 1083, 4);
        reg(&mut ym, 0, 0x40, 8); // 8 × 0,75 dB = 6 dB
        key(&mut ym, true);
        peak(&render(&mut ym, 0.1))
    };
    let ratio = quiet / loud;
    assert!((0.45..=0.56).contains(&ratio), "{ratio}");
}

#[test]
fn key_off_releases_to_silence() {
    let mut ym = Ym2612::new(RATE);
    single_operator(&mut ym, 1083, 4);
    key(&mut ym, true);
    render(&mut ym, 0.05);
    key(&mut ym, false);
    let tail = render(&mut ym, 0.2);
    assert!(peak(&tail[tail.len() - 100..]) < 0.001);
}

#[test]
fn slow_attack_ramps_up() {
    let mut ym = Ym2612::new(RATE);
    single_operator(&mut ym, 1083, 4);
    reg(&mut ym, 0, 0x50, 0x0A); // ataque lento
    key(&mut ym, true);
    let samples = render(&mut ym, 0.5);
    let early = peak(&samples[..2000]);
    let late = peak(&samples[samples.len() - 2000..]);
    assert!(early < late * 0.5, "{early} {late}");
}

#[test]
fn pan_left_only_silences_right() {
    let mut ym = Ym2612::new(RATE);
    single_operator(&mut ym, 1083, 4);
    reg(&mut ym, 0, 0xB4, 0x80);
    key(&mut ym, true);
    ym.run(crate::MASTER_CLOCK / 20);
    let mut out = Vec::new();
    ym.drain(&mut out);
    let right: Vec<f32> = out.chunks(2).map(|f| f[1]).collect();
    let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
    assert_eq!(peak(&right), 0.0);
    assert!(peak(&left) > 0.1);
}

#[test]
fn dac_replaces_channel_six() {
    let mut ym = Ym2612::new(RATE);
    reg(&mut ym, 0, 0x2B, 0x80);
    reg(&mut ym, 0, 0x2A, 0xFF);
    let high = render(&mut ym, 0.01);
    reg(&mut ym, 0, 0x2A, 0x00);
    let low = render(&mut ym, 0.01);
    assert!(high.last().is_some_and(|&s| s > 0.15));
    assert!(low.last().is_some_and(|&s| s < -0.15));
}

#[test]
fn timer_a_sets_flag_until_reset() {
    let mut ym = Ym2612::new(RATE);
    reg(&mut ym, 0, 0x24, 0xFF); // TA = 1020: estoura a cada 4 amostras
    reg(&mut ym, 0, 0x25, 0x00);
    reg(&mut ym, 0, 0x27, 0x05); // liga A e o flag A
    ym.run(MCLK_PER_SAMPLE * 3);
    assert_eq!(ym.read_status() & 1, 0);
    ym.run(MCLK_PER_SAMPLE);
    assert_eq!(ym.read_status() & 1, 1);
    reg(&mut ym, 0, 0x27, 0x15); // zera o flag, mantém ligado
    assert_eq!(ym.read_status() & 1, 0);
}

#[test]
fn timer_b_counts_in_steps_of_16_samples() {
    let mut ym = Ym2612::new(RATE);
    reg(&mut ym, 0, 0x26, 0xFE); // estoura após 2 passos = 32 amostras
    reg(&mut ym, 0, 0x27, 0x0A);
    ym.run(MCLK_PER_SAMPLE * 31);
    assert_eq!(ym.read_status() & 2, 0);
    ym.run(MCLK_PER_SAMPLE);
    assert_eq!(ym.read_status() & 2, 2);
}

#[test]
fn channel_3_special_mode_uses_its_own_frequency() {
    let mut ym = Ym2612::new(RATE);
    // Canal 3 (índice 2): só O1 audível, frequência normal 440 Hz.
    reg(&mut ym, 0, 0xB2, 0x07);
    for offset in [0x02, 0x06, 0x0A, 0x0E] {
        reg(&mut ym, 0, 0x30 + offset, 0x01);
        reg(
            &mut ym,
            0,
            0x40 + offset,
            if offset == 0x02 { 0 } else { 0x7F },
        );
        reg(&mut ym, 0, 0x50 + offset, 0x1F);
        reg(&mut ym, 0, 0x80 + offset, 0x0F);
    }
    reg(&mut ym, 0, 0xA6, 0x24);
    reg(&mut ym, 0, 0xA2, 0x3B); // 1083
    // O1 no modo especial: A9/AD com uma oitava acima (bloco 5, 1083).
    reg(&mut ym, 0, 0xAD, (5 << 3) | 0x04);
    reg(&mut ym, 0, 0xA9, 0x3B);
    reg(&mut ym, 0, 0x27, 0x40);
    reg(&mut ym, 0, 0x28, 0x12);
    let hz = zero_crossings(&render(&mut ym, 1.0)) as f64 / 2.0;
    assert!((860.0..=900.0).contains(&hz), "{hz} Hz");
}

#[test]
fn produces_stereo_at_requested_rate() {
    let mut ym = Ym2612::new(48_000);
    ym.run(crate::MASTER_CLOCK);
    let mut out = Vec::new();
    ym.drain(&mut out);
    assert!(out.len().abs_diff(96_000) <= 4, "{}", out.len());
}

#[test]
fn state_round_trip_continues_identically() {
    let mut a = Ym2612::new(RATE);
    single_operator(&mut a, 1083, 4);
    reg(&mut a, 0, 0x22, 0x0B); // LFO ligado
    reg(&mut a, 0, 0xB4, 0xC7); // vibrato máximo
    key(&mut a, true);
    render(&mut a, 0.05);
    let mut w = Writer::default();
    a.save_state(&mut w);

    let mut b = Ym2612::new(RATE);
    let mut r = Reader::new(&w.bytes);
    b.load_state(&mut r).expect("estado válido");
    assert_eq!(r.finish(), Ok(()));
    a.samples.clear();
    a.phase = 0;
    a.acc = [0.0; 2];
    a.acc_count = 0;
    assert_eq!(render(&mut a, 0.05), render(&mut b, 0.05));
}
