//! Roda uma ROM por alguns quadros e mostra o que ela enviou pela serial
//! (útil para ROMs de teste).
//!
//! cargo run -p gameboy --release --example gb_serial -- teste.gb [quadros] [endereço hex]
//!
//! Com o endereço, mostra também os 8 bytes da memória a partir dele.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("uso: gb_serial <rom> [quadros]")?;
    let frames: u32 = args.next().map_or(Ok(600), |s| s.parse())?;
    let peek = args
        .next()
        .map(|s| u16::from_str_radix(&s, 16))
        .transpose()?;
    let mut console = gameboy::Console::new(std::fs::read(path)?, 44_100)?;
    for _ in 0..frames {
        console.run_frame();
    }
    let out = console.serial_output();
    println!("{}", String::from_utf8_lossy(out));
    println!("{out:?}");
    let hram: Vec<u8> = (0xFF80..0xFF88).map(|a| console.peek(a)).collect();
    println!("HRAM FF80–FF87 = {hram:02X?}");
    if let Some(addr) = peek {
        let bytes: Vec<u8> = (0..8).map(|i| console.peek(addr.wrapping_add(i))).collect();
        println!("{addr:04X} = {bytes:02X?}");
    }
    println!(
        "A F B C D E H L = {:02X?}  PC = {:04X}",
        console.cpu_registers(),
        console.pc()
    );
    Ok(())
}
