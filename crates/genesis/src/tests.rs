use super::*;

/// ROM sintética: liga a tela em H40 com fundo branco e conta V-ints em
/// FF0000. Vetores: SSP = FFFE00, PC = 0x200, nível 6 → 0x300.
fn counting_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x1000];
    let put_long =
        |rom: &mut Vec<u8>, at: usize, v: u32| rom[at..at + 4].copy_from_slice(&v.to_be_bytes());
    put_long(&mut rom, 0, 0x00FF_FE00);
    put_long(&mut rom, 4, 0x0000_0200);
    put_long(&mut rom, 0x78, 0x0000_0300);
    let program: &[u16] = &[
        0x33FC, 0x8174, 0x00C0, 0x0004, // R1 = 74: tela, V-int, DMA, modo 5
        0x33FC, 0x8C81, 0x00C0, 0x0004, // R12 = 81: H40
        0x33FC, 0x8705, 0x00C0, 0x0004, // R7 = 05: fundo = cor 5
        0x23FC, 0xC00A, 0x0000, 0x00C0, 0x0004, // CRAM, endereço 0x0A
        0x33FC, 0x0EEE, 0x00C0, 0x0000, // cor 5 = branco
        0x46FC, 0x2000, // libera interrupções
        0x60FE, // BRA *
    ];
    let handler: &[u16] = &[
        0x5279, 0x00FF, 0x0000, // ADDQ.W #1,$FF0000
        0x4E73, // RTE
    ];
    for (i, w) in program.iter().enumerate() {
        rom[0x200 + i * 2..0x202 + i * 2].copy_from_slice(&w.to_be_bytes());
    }
    for (i, w) in handler.iter().enumerate() {
        rom[0x300 + i * 2..0x302 + i * 2].copy_from_slice(&w.to_be_bytes());
    }
    rom
}

fn vint_count(console: &mut Console) -> u16 {
    m68k::Bus::read_word(&mut console.bus, 0xFF_0000)
}

#[test]
fn rejects_empty_and_oversized_roms() {
    assert_eq!(
        Console::new(Vec::new(), 44_100).err(),
        Some(RomError::Empty)
    );
    assert!(matches!(
        Console::new(vec![0; MAX_ROM_SIZE + 1], 44_100),
        Err(RomError::TooLarge(_))
    ));
}

#[test]
fn smd_dumps_are_deinterleaved() {
    let raw = counting_rom();
    let mut raw16 = raw.clone();
    raw16.resize(SMD_BLOCK, 0);
    let mut smd = vec![0u8; SMD_HEADER];
    smd[8] = 0xAA;
    smd[9] = 0xBB;
    let half = SMD_BLOCK / 2;
    let mut block = vec![0u8; SMD_BLOCK];
    for i in 0..half {
        block[i] = raw16[i * 2 + 1];
        block[half + i] = raw16[i * 2];
    }
    smd.extend(block);
    assert_eq!(normalize_rom(smd), Ok(raw16));
}

#[test]
fn program_draws_background_and_counts_vblanks() {
    let mut console = Console::new(counting_rom(), 44_100).expect("ROM válida");
    for _ in 0..3 {
        console.run_frame();
    }
    let (width, height) = console.frame_size();
    assert_eq!((width, height), (320, 224));
    let stride = SCREEN_MAX_WIDTH;
    assert!(
        console
            .framebuffer()
            .chunks(stride)
            .take(height)
            .all(|row| row[..width].iter().all(|&p| p == 0xFFFFFF))
    );
    // Um V-int por quadro (o primeiro quadro configura antes da linha 224).
    let count = vint_count(&mut console);
    assert!((2..=3).contains(&count), "{count}");
}

#[test]
fn frame_produces_psg_audio() {
    let mut console = Console::new(counting_rom(), 48_000).expect("ROM válida");
    console.run_frame();
    let mut audio = Vec::new();
    console.drain_audio(&mut audio);
    // Estéreo: dois valores por amostra.
    let expected = (48_000.0 / FRAME_RATE) as usize * 2;
    assert!(audio.len().abs_diff(expected) <= 6, "{}", audio.len());
}

#[test]
fn save_and_load_state_resume_deterministically() {
    let mut original = Console::new(counting_rom(), 44_100).expect("ROM válida");
    for _ in 0..5 {
        original.run_frame();
    }
    let saved = original.save_state();
    for _ in 0..4 {
        original.run_frame();
    }
    let mut resumed = Console::new(counting_rom(), 44_100).expect("ROM válida");
    resumed.load_state(&saved).expect("estado válido");
    for _ in 0..4 {
        resumed.run_frame();
    }
    assert_eq!(resumed.save_state(), original.save_state());
    assert_eq!(vint_count(&mut resumed), vint_count(&mut original));
}

#[test]
fn load_state_rejects_other_rom_and_corruption() {
    let mut console = Console::new(counting_rom(), 44_100).expect("ROM válida");
    console.run_frame();
    let saved = console.save_state();
    let before = console.save_state();

    let mut other_rom = counting_rom();
    other_rom[0x800] = 1;
    let mut other = Console::new(other_rom, 44_100).expect("ROM válida");
    assert_eq!(other.load_state(&saved), Err(StateError::WrongRom));

    assert_eq!(console.load_state(b"lixo"), Err(StateError::NotAState));
    assert_eq!(
        console.load_state(&saved[..saved.len() - 1]),
        Err(StateError::Truncated)
    );
    // Um estado do Master System não é aceito.
    let mut sms_like = saved.clone();
    sms_like[..8].copy_from_slice(b"PCSMSST\0");
    assert_eq!(console.load_state(&sms_like), Err(StateError::NotAState));
    assert_eq!(console.save_state(), before);
}
