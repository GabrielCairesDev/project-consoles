//! Roda uma ROM por alguns quadros e mostra o que ela enviou pela serial
//! (útil para ROMs de teste).
//!
//! cargo run -p gameboy --release --example gb_serial -- teste.gb [quadros]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("uso: gb_serial <rom> [quadros]")?;
    let frames: u32 = args.next().map_or(Ok(600), |s| s.parse())?;
    let mut console = gameboy::Console::new(std::fs::read(path)?, 44_100)?;
    for _ in 0..frames {
        console.run_frame();
    }
    let out = console.serial_output();
    println!("{}", String::from_utf8_lossy(out));
    println!("{out:?}");
    let hram: Vec<u8> = (0xFF80..0xFF88).map(|a| console.peek(a)).collect();
    println!("HRAM FF80–FF87 = {hram:02X?}");
    println!(
        "A F B C D E H L = {:02X?}  PC = {:04X}",
        console.cpu_registers(),
        console.pc()
    );
    Ok(())
}
