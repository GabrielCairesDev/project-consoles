use super::*;

/// ROM só com cartucho básico: conta em FF80 a cada V-blank (interrupção)
/// e escreve "ok" na serial no começo.
fn counting_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    let program: &[u8] = &[
        0x3E, b'o', 0xE0, 0x01, 0x3E, 0x81, 0xE0, 0x02, // SB = 'o', SC = 81
        0x3E, 0x01, 0xE0, 0xFF, // IE = V-blank
        0xAF, 0xE0, 0x0F, // IF = 0
        0xFB, // EI
        0x76, 0x18, 0xFD, // HALT; JR -3
    ];
    rom[0x100..0x100 + program.len()].copy_from_slice(program);
    // V-blank: INC (FF80); RETI
    rom[0x40..0x45].copy_from_slice(&[0x21, 0x80, 0xFF, 0x34, 0xD9]);
    rom
}

fn vblank_count(console: &Console) -> u8 {
    console.bus.peek(0xFF80)
}

#[test]
fn rejects_bad_roms() {
    assert!(matches!(
        Console::new(vec![0; 0x10], 44_100),
        Err(RomError::Cart(CartError::TooSmall(_)))
    ));
    assert!(matches!(
        Console::new(vec![0; MAX_ROM_SIZE + 1], 44_100),
        Err(RomError::TooLarge(_))
    ));
}

#[test]
fn one_vblank_per_frame_and_serial_output() {
    let mut console = Console::new(counting_rom(), 44_100).expect("ROM válida");
    for _ in 0..10 {
        console.run_frame();
    }
    let count = vblank_count(&console);
    assert!((9..=10).contains(&count), "{count}");
    assert_eq!(console.serial_output(), b"o");
}

#[test]
fn input_change_requests_joypad_interrupt() {
    let mut console = Console::new(counting_rom(), 44_100).expect("ROM válida");
    console.bus.joypad.write(0x20);
    console.set_input(Buttons {
        down: true,
        ..Buttons::default()
    });
    assert_ne!(console.bus.interrupt_flag & bus::INT_JOYPAD, 0);
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
}

#[test]
fn load_state_rejects_other_rom_and_corruption() {
    let mut console = Console::new(counting_rom(), 44_100).expect("ROM válida");
    console.run_frame();
    let saved = console.save_state();
    let before = console.save_state();

    let mut other_rom = counting_rom();
    other_rom[0x7000] = 1;
    let mut other = Console::new(other_rom, 44_100).expect("ROM válida");
    assert_eq!(other.load_state(&saved), Err(StateError::WrongRom));
    assert_eq!(console.load_state(b"lixo"), Err(StateError::NotAState));
    assert_eq!(
        console.load_state(&saved[..saved.len() - 1]),
        Err(StateError::Truncated)
    );
    assert_eq!(console.save_state(), before);
}

#[test]
fn battery_ram_only_for_carts_with_battery() {
    let plain = Console::new(counting_rom(), 44_100).expect("ROM válida");
    assert!(plain.battery_ram().is_none());

    let mut rom = counting_rom();
    rom[0x147] = 0x03; // MBC1 + RAM + bateria
    rom[0x149] = 0x02; // 8 KB
    let mut console = Console::new(rom, 44_100).expect("ROM válida");
    let saved = vec![0x5A; 0x2000];
    assert!(console.load_battery_ram(&saved));
    assert_eq!(console.battery_ram(), Some(saved.as_slice()));
    assert!(!console.load_battery_ram(&[0; 10]));
}
