use super::*;

/// PS-EXE mínimo: `text` carregado em 0x80010000.
fn exe(text: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 0x800];
    bytes[..8].copy_from_slice(b"PS-X EXE");
    bytes[0x10..0x14].copy_from_slice(&0x8001_0000u32.to_le_bytes());
    bytes[0x14..0x18].copy_from_slice(&0x1234u32.to_le_bytes());
    bytes[0x18..0x1C].copy_from_slice(&0x8001_0000u32.to_le_bytes());
    bytes[0x1C..0x20].copy_from_slice(&(text.len() as u32).to_le_bytes());
    bytes[0x30..0x34].copy_from_slice(&0x801F_FF00u32.to_le_bytes());
    bytes[0x34..0x38].copy_from_slice(&0xF0u32.to_le_bytes());
    bytes.extend_from_slice(text);
    bytes
}

#[test]
fn rejects_bios_of_wrong_size() {
    assert!(matches!(
        Console::new(&[0; 1000]),
        Err(BiosError::WrongSize(1000))
    ));
}

#[test]
fn rejects_invalid_exe() {
    let mut console = Console::new(&[0; BIOS_SIZE]).expect("BIOS");
    assert_eq!(console.load_exe(b"lixo"), Err(ExeError::NotAnExe));
    let mut big = exe(&[]);
    big[0x18..0x1C].copy_from_slice(&0x801F_FFF0u32.to_le_bytes());
    big[0x1C..0x20].copy_from_slice(&0x100u32.to_le_bytes());
    big.resize(0x900, 0);
    assert_eq!(console.load_exe(&big), Err(ExeError::OutOfRange));
}

/// BIOS falso que pula para o shell e um programa que chama putchar (A0h,
/// função 3Ch) com "ok".
#[test]
fn exe_is_started_at_the_shell_and_putchar_is_captured() {
    let mut bios = vec![0; BIOS_SIZE];
    // lui t0, 0x8003; jr t0; nop
    for (n, w) in [0x3C08_8003u32, 0x0100_0008, 0].iter().enumerate() {
        bios[n * 4..n * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    let mut console = Console::new(&bios).expect("BIOS");

    let program: Vec<u32> = vec![
        0x2409_003C, // addiu t1, zero, 0x3C
        0x2404_006F, // addiu a0, zero, 'o'
        0x2408_00A0, // addiu t0, zero, 0xA0
        0x0100_F809, // jalr t0
        0x0000_0000,
        0x2404_006B, // addiu a0, zero, 'k'
        0x0100_F809, // jalr t0
        0x0000_0000,
        0x1000_FFFF, // b .
        0x0000_0000,
    ];
    let text: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    console.load_exe(&exe(&text)).expect("PS-EXE");
    // Em 0xA0, um "jr ra" para voltar.
    console.bus.write32(0xA0, 0x03E0_0008);
    console.run_frame();
    assert_eq!(console.tty_output(), b"ok");
    assert_eq!(console.cpu.reg(28), 0x1234, "gp do cabeçalho");
    assert_eq!(console.cpu.reg(29), 0x801F_FFF0, "pilha do cabeçalho");
}

#[test]
fn frame_lasts_one_ntsc_field() {
    let mut console = Console::new(&[0; BIOS_SIZE]).expect("BIOS");
    console.run_frame();
    let first = console.cycles();
    console.run_frame();
    // 263 linhas × 3413 tiques × 7/11 ≈ 571212 ciclos.
    assert!((first + 571_000..first + 571_500).contains(&console.cycles()));
}

#[test]
fn enhancements_round_trip_and_default_to_native() {
    let mut console = Console::new(&[0; BIOS_SIZE]).expect("BIOS");
    assert_eq!(console.enhancements(), Enhancements::default());
    assert_eq!(
        Enhancements::default().resolution,
        InternalResolution::Native
    );
    for scale in 1..=4 {
        let resolution = InternalResolution::from_scale(scale).expect("1 a 4");
        assert_eq!(resolution.scale(), scale);
        for true_color in [false, true] {
            for precise_geometry in [false, true] {
                for texture_filtering in [false, true] {
                    let enhancements = Enhancements {
                        resolution,
                        true_color,
                        precise_geometry,
                        texture_filtering,
                    };
                    console.set_enhancements(enhancements);
                    assert_eq!(console.enhancements(), enhancements);
                }
            }
        }
    }
    assert_eq!(InternalResolution::from_scale(0), None);
    assert_eq!(InternalResolution::from_scale(5), None);
}
