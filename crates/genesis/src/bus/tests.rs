use super::*;
use crate::m68k::Bus as _;
use chips::z80::Bus as _;

fn system_with_rom(rom: Vec<u8>) -> System {
    System::new(rom, Psg::new(3_579_545, 44_100), Ym2612::new(44_100))
}

fn rom_with_sram(size: usize) -> Vec<u8> {
    let mut rom = vec![0; size];
    rom[0x1B0..0x1B2].copy_from_slice(b"RA");
    rom
}

#[test]
fn rom_and_ram_mapping() {
    let mut rom = vec![0; 0x2000];
    rom[0x100] = 0x12;
    rom[0x101] = 0x34;
    let mut bus = system_with_rom(rom);
    assert_eq!(bus.read_word(0x100), 0x1234);
    // ROM espelhada no espaço do cartucho.
    assert_eq!(bus.read_word(0x2100), 0x1234);
    bus.write_word(0xFF_0010, 0xBEEF);
    assert_eq!(bus.read_word(0xE0_0010), 0xBEEF);
    // Escrita na ROM é ignorada.
    bus.write_word(0x100, 0);
    assert_eq!(bus.read_word(0x100), 0x1234);
}

#[test]
fn sram_is_always_mapped_on_small_roms() {
    let mut bus = system_with_rom(rom_with_sram(0x10_0000));
    bus.write_byte(0x20_0001, 0x5A);
    assert_eq!(bus.read_byte(0x20_0001), 0x5A);
}

#[test]
fn sram_on_large_roms_needs_a130f1() {
    let mut rom = rom_with_sram(0x40_0000);
    rom[0x20_0001] = 0x77;
    let mut bus = system_with_rom(rom);
    assert_eq!(bus.read_byte(0x20_0001), 0x77, "começa mostrando a ROM");
    bus.write_byte(0xA1_30F1, 1);
    bus.write_byte(0x20_0001, 0x11);
    assert_eq!(bus.read_byte(0x20_0001), 0x11);
    bus.write_byte(0xA1_30F1, 0);
    assert_eq!(bus.read_byte(0x20_0001), 0x77);
}

#[test]
fn version_register_reports_ntsc_export() {
    let mut bus = system_with_rom(vec![0; 0x200]);
    assert_eq!(bus.read_byte(0xA1_0001), 0xA0);
}

#[test]
fn z80_busreq_and_reset() {
    let mut bus = system_with_rom(vec![0; 0x200]);
    assert!(bus.z80_reset, "o Z80 liga em reset");
    assert!(!bus.z80_running());
    bus.write_word(0xA1_1200, 0x0100);
    assert!(bus.z80_running());
    bus.write_word(0xA1_1100, 0x0100);
    assert!(!bus.z80_running());
    // Barramento cedido: bit 8 em 0.
    assert_eq!(bus.read_word(0xA1_1100) & 0x0100, 0);
    bus.write_word(0xA1_1100, 0x0000);
    assert_ne!(bus.read_word(0xA1_1100) & 0x0100, 0);
}

#[test]
fn z80_ram_seen_from_both_sides() {
    let mut bus = system_with_rom(vec![0; 0x200]);
    bus.write_byte(0xA0_0010, 0x42);
    assert_eq!(bus.read_word(0xA0_0010), 0x4242);
    assert_eq!(Z80Bus(&mut bus).read(0x0010), 0x42);
    // Espelho em 2000–3FFF.
    assert_eq!(Z80Bus(&mut bus).read(0x2010), 0x42);
}

#[test]
fn z80_bank_window_reads_68000_memory() {
    let mut rom = vec![0; 0x20_0000];
    rom[0x18_0005] = 0x99;
    let mut bus = system_with_rom(rom);
    // Banco 0x180000 >> 15 = 0x30: 9 bits escritos do menos significativo.
    let bank = 0x18_0000u32 >> 15;
    for i in 0..9 {
        Z80Bus(&mut bus).write(0x6000, ((bank >> i) & 1) as u8);
    }
    assert_eq!(Z80Bus(&mut bus).read(0x8005), 0x99);
}

#[test]
fn controller_is_read_through_io_port() {
    let mut bus = system_with_rom(vec![0; 0x200]);
    bus.write_byte(0xA1_0009, 0x40); // TH como saída
    bus.write_byte(0xA1_0003, 0x40);
    bus.ports[0].pad.right = true;
    assert_eq!(bus.read_byte(0xA1_0003) & 0x08, 0, "direita apertada = 0");
}

#[test]
fn dma_from_ram_copies_words_and_stalls_cpu() {
    let mut bus = system_with_rom(vec![0; 0x200]);
    bus.write_word(0xFF_0000, 0x1122);
    bus.write_word(0xFF_0002, 0x3344);
    let reg = |bus: &mut System, r: u16, v: u16| bus.write_word(0xC0_0004, 0x8000 | r << 8 | v);
    reg(&mut bus, 1, 0x14); // DMA ligado, tela desligada
    reg(&mut bus, 15, 2);
    reg(&mut bus, 19, 2);
    reg(&mut bus, 20, 0);
    reg(&mut bus, 21, 0x00);
    reg(&mut bus, 22, 0x80);
    reg(&mut bus, 23, 0x7F);
    // Comando VRAM write com DMA no endereço 0x0400.
    bus.write_word(0xC0_0004, 0x4400);
    bus.write_word(0xC0_0004, 0x0080);
    assert!(bus.dma_stall > 0);

    // Lê de volta pela porta de dados.
    bus.write_word(0xC0_0004, 0x0400);
    bus.write_word(0xC0_0004, 0x0000);
    assert_eq!(bus.read_word(0xC0_0000), 0x1122);
    assert_eq!(bus.read_word(0xC0_0000), 0x3344);
}

#[test]
fn psg_is_reachable_from_both_cpus() {
    let mut bus = system_with_rom(vec![0; 0x200]);
    bus.write_byte(0xC0_0011, 0x9F);
    Z80Bus(&mut bus).write(0x7F11, 0xBF);
    let mut out = Vec::new();
    bus.psg.run(3_579_545 / 60);
    bus.psg.drain(&mut out);
    assert!(!out.is_empty());
}

#[test]
fn state_round_trips_with_sram() {
    let mut bus = system_with_rom(rom_with_sram(0x1000));
    bus.write_byte(0xFF_1234, 7);
    bus.write_byte(0x20_0001, 9);
    bus.write_byte(0xA0_0001, 3);
    let mut w = Writer::default();
    bus.save_state(&mut w);
    let mut restored = system_with_rom(rom_with_sram(0x1000));
    let mut r = Reader::new(&w.bytes);
    restored.load_state(&mut r).expect("estado válido");
    assert_eq!(r.finish(), Ok(()));
    assert_eq!(restored.read_byte(0xFF_1234), 7);
    assert_eq!(restored.read_byte(0x20_0001), 9);
    assert_eq!(restored.read_byte(0xA0_0001), 3);

    // ROM sem SRAM não aceita estado com SRAM.
    let mut other = system_with_rom(vec![0; 0x1000]);
    assert!(other.load_state(&mut Reader::new(&w.bytes)).is_err());
}

#[test]
fn battery_ram_loads_only_with_matching_size() {
    let mut bus = system_with_rom(rom_with_sram(0x1000));
    let mut saved = vec![0u8; SRAM_SIZE];
    saved[1] = 0x42;
    assert!(bus.load_sram(&saved));
    assert_eq!(bus.read_byte(0x20_0001), 0x42);
    assert_eq!(bus.sram().map(<[u8]>::len), Some(SRAM_SIZE));
    assert!(!bus.load_sram(&[1, 2, 3]));
    let mut plain = system_with_rom(vec![0; 0x1000]);
    assert!(plain.sram().is_none() && !plain.load_sram(&saved));
}
