//! Conformidade do Z80 com ZEXDOC/ZEXALL (Frank D. Cringle).
//!
//! Os programas não são versionados (licença GPL). Para rodar:
//!
//! ```text
//! ZEX_PATH=/caminho/zexdoc.com cargo test -p chips --release --test zex -- --ignored --nocapture
//! ```
//!
//! Harness CP/M mínimo: o programa é carregado em 0x0100, a chamada BDOS em
//! 0x0005 é interceptada (funções 2 e 9, saída de texto) e o salto para
//! 0x0000 encerra a execução.

use chips::z80::{Bus, Z80};

struct CpmBus {
    mem: Vec<u8>,
}

impl Bus for CpmBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.mem[usize::from(addr)]
    }
    fn write(&mut self, addr: u16, value: u8) {
        self.mem[usize::from(addr)] = value;
    }
    fn input(&mut self, _port: u16) -> u8 {
        0xFF
    }
    fn output(&mut self, _port: u16, _value: u8) {}
    fn int_line(&self) -> bool {
        false
    }
}

/// Executa a chamada BDOS e devolve o texto impresso.
fn bdos(cpu: &Z80, bus: &CpmBus) -> String {
    match cpu.c {
        2 => char::from(cpu.e).to_string(),
        9 => bus.mem[usize::from(cpu.de())..]
            .iter()
            .take_while(|&&b| b != b'$')
            .map(|&b| char::from(b))
            .collect(),
        _ => String::new(),
    }
}

#[test]
#[ignore = "requer ZEX_PATH apontando para zexdoc.com ou zexall.com"]
fn zex_suite_passes() {
    let path = std::env::var("ZEX_PATH").expect("defina ZEX_PATH");
    let program = std::fs::read(&path).expect("não foi possível ler ZEX_PATH");

    let mut bus = CpmBus {
        mem: vec![0; 0x10000],
    };
    bus.mem[0x100..0x100 + program.len()].copy_from_slice(&program);
    // 0x0005: RET, para voltar logo após a chamada BDOS interceptada.
    bus.mem[0x0005] = 0xC9;
    let mut cpu = Z80::new();
    cpu.pc = 0x0100;
    cpu.sp = 0xF000;

    let mut output = String::new();
    loop {
        if cpu.pc == 0x0005 {
            let text = bdos(&cpu, &bus);
            print!("{text}");
            output.push_str(&text);
        }
        if cpu.pc == 0x0000 {
            break;
        }
        cpu.step(&mut bus);
    }

    assert!(!output.contains("ERROR"), "falhas na suíte ZEX:\n{output}");
    assert!(
        output.contains("Tests complete"),
        "suíte incompleta:\n{output}"
    );
}
