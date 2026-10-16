use super::*;

/// ROM com `banks` bancos; os dois primeiros bytes de cada banco são o
/// número dele (byte baixo, byte alto).
fn rom(kind: u8, banks: usize, ram_code: u8) -> Vec<u8> {
    let mut rom = vec![0; banks * ROM_BANK];
    for bank in 0..banks {
        rom[bank * ROM_BANK] = bank as u8;
        rom[bank * ROM_BANK + 1] = (bank >> 8) as u8;
    }
    rom[0x147] = kind;
    rom[0x149] = ram_code;
    rom
}

fn high_bank(cart: &Cart) -> u8 {
    cart.read_rom(0x4000)
}

#[test]
fn rejects_tiny_rom_and_unknown_type() {
    assert_eq!(
        Cart::new(vec![0; 0x100]).err(),
        Some(CartError::TooSmall(0x100))
    );
    assert_eq!(
        Cart::new(rom(0xFC, 2, 0)).err(),
        Some(CartError::UnsupportedType(0xFC))
    );
}

#[test]
fn rom_only_maps_two_banks() {
    let cart = Cart::new(rom(0x00, 2, 0)).expect("cartucho");
    assert_eq!(cart.read_rom(0x0000), 0);
    assert_eq!(high_bank(&cart), 1);
    assert_eq!(cart.read_ram(0xA000), 0xFF);
}

#[test]
fn mbc1_bank_zero_becomes_one_and_masks_to_size() {
    let mut cart = Cart::new(rom(0x01, 8, 0)).expect("cartucho");
    cart.write_rom(0x2000, 0);
    assert_eq!(high_bank(&cart), 1);
    cart.write_rom(0x2000, 5);
    assert_eq!(high_bank(&cart), 5);
    // 0x0D em 8 bancos: só os 3 bits baixos.
    cart.write_rom(0x2000, 0x0D);
    assert_eq!(high_bank(&cart), 5);
    // 0x20 tem os 5 bits zerados: vira 1.
    cart.write_rom(0x2000, 0x20);
    assert_eq!(high_bank(&cart), 1);
}

#[test]
fn mbc1_upper_bits_and_mode_one() {
    let mut cart = Cart::new(rom(0x01, 128, 0)).expect("cartucho");
    cart.write_rom(0x2000, 0x02);
    cart.write_rom(0x4000, 0x01);
    assert_eq!(high_bank(&cart), 0x22);
    assert_eq!(cart.read_rom(0x0000), 0, "modo 0: banco 0 fixo");
    cart.write_rom(0x6000, 1);
    assert_eq!(cart.read_rom(0x0000), 0x20, "modo 1: bits altos no banco 0");
}

#[test]
fn mbc1_ram_needs_enable_and_banks_in_mode_one() {
    let mut cart = Cart::new(rom(0x03, 4, 0x03)).expect("cartucho");
    assert!(cart.has_battery());
    cart.write_ram(0xA000, 0x42);
    assert_eq!(cart.read_ram(0xA000), 0xFF, "RAM desligada");
    cart.write_rom(0x0000, 0x0A);
    cart.write_ram(0xA000, 0x42);
    assert_eq!(cart.read_ram(0xA000), 0x42);
    cart.write_rom(0x6000, 1);
    cart.write_rom(0x4000, 2);
    assert_eq!(cart.read_ram(0xA000), 0, "banco 2 da RAM");
    cart.write_ram(0xA000, 0x99);
    cart.write_rom(0x4000, 0);
    assert_eq!(cart.read_ram(0xA000), 0x42);
    cart.write_rom(0x0000, 0x00);
    assert_eq!(cart.read_ram(0xA000), 0xFF);
}

#[test]
fn mbc1_multicart_uses_four_low_bits() {
    let mut data = rom(0x01, 64, 0);
    for i in 0x104..0x134 {
        data[i] = i as u8;
        data[0x10 * ROM_BANK + i] = i as u8;
    }
    let mut cart = Cart::new(data).expect("cartucho");
    cart.write_rom(0x4000, 1);
    cart.write_rom(0x2000, 0x03);
    assert_eq!(high_bank(&cart), 0x13);
}

#[test]
fn mbc2_uses_address_bit_8_and_four_bit_ram() {
    let mut cart = Cart::new(rom(0x06, 16, 0)).expect("cartucho");
    cart.write_rom(0x2100, 0x07);
    assert_eq!(high_bank(&cart), 7);
    cart.write_rom(0x0000, 0x0A);
    cart.write_ram(0xA001, 0xAB);
    assert_eq!(cart.read_ram(0xA001), 0xFB);
    // 512 posições espelhadas por toda a área.
    assert_eq!(cart.read_ram(0xA201), 0xFB);
}

#[test]
fn mbc3_rtc_latches_and_counts_emulated_time() {
    let mut cart = Cart::new(rom(0x10, 4, 0x03)).expect("cartucho");
    cart.write_rom(0x0000, 0x0A);
    cart.write_rom(0x4000, 0x08);
    cart.tick(CLOCK_HZ * 61);
    assert_eq!(
        cart.read_ram(0xA000),
        0,
        "antes do latch o valor é o antigo"
    );
    cart.write_rom(0x6000, 0);
    cart.write_rom(0x6000, 1);
    assert_eq!(cart.read_ram(0xA000), 1, "segundos");
    cart.write_rom(0x4000, 0x09);
    assert_eq!(cart.read_ram(0xA000), 1, "minutos");

    // Parado (bit 6 do controle), o relógio não anda.
    cart.write_rom(0x4000, 0x0C);
    cart.write_ram(0xA000, 0x40);
    cart.tick(CLOCK_HZ * 5);
    cart.write_rom(0x6000, 0);
    cart.write_rom(0x6000, 1);
    cart.write_rom(0x4000, 0x08);
    assert_eq!(cart.read_ram(0xA000), 1);
}

#[test]
fn rtc_day_counter_overflows_into_carry() {
    let mut rtc = Rtc {
        regs: [59, 59, 23, 0xFF, 0x01],
        ..Rtc::default()
    };
    rtc.advance_second();
    assert_eq!(rtc.regs, [0, 0, 0, 0, 0x80]);
}

#[test]
fn mbc5_allows_bank_zero_and_nine_bits() {
    let mut cart = Cart::new(rom(0x19, 512, 0)).expect("cartucho");
    cart.write_rom(0x2000, 0);
    assert_eq!(high_bank(&cart), 0);
    cart.write_rom(0x2000, 0x05);
    cart.write_rom(0x3000, 0x01);
    assert_eq!(
        (cart.read_rom(0x4001), high_bank(&cart)),
        (0x01, 0x05),
        "banco 0x105"
    );
}

#[test]
fn battery_ram_round_trips_and_rejects_wrong_size() {
    let mut cart = Cart::new(rom(0x13, 4, 0x03)).expect("cartucho");
    let saved = vec![7; 4 * RAM_BANK];
    assert!(cart.load_ram(&saved));
    assert_eq!(cart.ram(), saved.as_slice());
    assert!(!cart.load_ram(&[1, 2, 3]));
}

#[test]
fn state_round_trip() {
    let mut cart = Cart::new(rom(0x10, 8, 0x02)).expect("cartucho");
    cart.write_rom(0x0000, 0x0A);
    cart.write_rom(0x2000, 3);
    cart.write_ram(0xA010, 0x55);
    cart.tick(CLOCK_HZ + 7);
    let mut w = Writer::default();
    cart.save_state(&mut w);

    let mut other = Cart::new(rom(0x10, 8, 0x02)).expect("cartucho");
    let mut r = Reader::new(&w.bytes);
    other.load_state(&mut r).expect("estado válido");
    assert_eq!(r.finish(), Ok(()));
    assert_eq!(high_bank(&other), 3);
    assert_eq!(other.read_ram(0xA010), 0x55);
    assert_eq!(other.rtc, cart.rtc);
}
