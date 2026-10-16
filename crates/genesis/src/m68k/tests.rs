use super::{Bus, Cpu, Registers, sr};

/// 16 MB planos (endereços de 24 bits).
struct Ram(Vec<u8>);

impl Ram {
    fn with_program(at: u32, words: &[u16]) -> Self {
        let mut mem = vec![0; 0x100_0000];
        for (i, w) in words.iter().enumerate() {
            let a = at as usize + i * 2;
            mem[a] = (w >> 8) as u8;
            mem[a + 1] = *w as u8;
        }
        Self(mem)
    }

    fn word(&self, addr: u32) -> u16 {
        u16::from(self.0[addr as usize]) << 8 | u16::from(self.0[addr as usize + 1])
    }
}

impl Bus for Ram {
    fn read_byte(&mut self, addr: u32) -> u8 {
        self.0[addr as usize]
    }
    fn read_word(&mut self, addr: u32) -> u16 {
        self.word(addr)
    }
    fn write_byte(&mut self, addr: u32, value: u8) {
        self.0[addr as usize] = value;
    }
    fn write_word(&mut self, addr: u32, value: u16) {
        self.0[addr as usize] = (value >> 8) as u8;
        self.0[addr as usize + 1] = value as u8;
    }
}

/// CPU em supervisor com o programa em 0x1000 e a pilha em 0x8000.
fn boot(words: &[u16]) -> (Cpu, Ram) {
    let mut ram = Ram::with_program(0x1000, words);
    let mut cpu = Cpu::new();
    cpu.set_registers(&Registers {
        ssp: 0x8000,
        sr: sr::S | sr::INT_MASK,
        pc: 0x1000,
        prefetch: [ram.word(0x1000), ram.word(0x1002)],
        ..Registers::default()
    });
    let _ = &mut ram;
    (cpu, ram)
}

#[test]
fn reset_loads_stack_and_pc_from_vectors() {
    let mut ram = Ram::with_program(0, &[0x0000, 0x8000, 0x0000, 0x0400]);
    ram.write_word(0x400, 0x4E71);
    let mut cpu = Cpu::new();
    cpu.reset(&mut ram);
    let r = cpu.registers();
    assert_eq!(r.ssp, 0x8000);
    assert_eq!(r.pc, 0x400);
    assert_eq!(r.prefetch[0], 0x4E71);
}

#[test]
fn moveq_add_and_branch() {
    // MOVEQ #5,D0 ; ADDQ.L #3,D0 ; BRA.S -2 (laço)
    let (mut cpu, mut ram) = boot(&[0x7005, 0x5680, 0x60FE]);
    assert_eq!(cpu.step(&mut ram), 4);
    assert_eq!(cpu.step(&mut ram), 8);
    assert_eq!(cpu.registers().d[0], 8);
    assert_eq!(cpu.step(&mut ram), 10);
    assert_eq!(cpu.registers().pc, 0x1004);
}

#[test]
fn jsr_rts_round_trip() {
    // JSR $1010 ; NOP ... $1010: RTS
    let mut words = vec![0x4EB9, 0x0000, 0x1010, 0x4E71];
    words.resize(8, 0x4E71);
    words.push(0x4E75);
    let (mut cpu, mut ram) = boot(&words);
    assert_eq!(cpu.step(&mut ram), 20);
    assert_eq!(cpu.registers().pc, 0x1010);
    assert_eq!(ram.word(0x7FFC), 0x0000);
    assert_eq!(ram.word(0x7FFE), 0x1006);
    assert_eq!(cpu.step(&mut ram), 16);
    assert_eq!(cpu.registers().pc, 0x1006);
}

#[test]
fn divide_by_zero_raises_exception() {
    // DIVU D1,D0 com D1 = 0; vetor 5 aponta para 0x2000.
    let (mut cpu, mut ram) = boot(&[0x80C1]);
    ram.write_word(0x14, 0x0000);
    ram.write_word(0x16, 0x2000);
    cpu.step(&mut ram);
    let r = cpu.registers();
    assert_eq!(r.pc, 0x2000);
    assert_eq!(r.ssp, 0x8000 - 6);
}

#[test]
fn odd_word_access_raises_address_error() {
    // MOVE.W (A0),D0 com A0 ímpar; vetor 3 aponta para 0x3000.
    let (mut cpu, mut ram) = boot(&[0x3010]);
    let mut regs = cpu.registers();
    regs.a[0] = 0x2001;
    cpu.set_registers(&regs);
    ram.write_word(0x0C, 0x0000);
    ram.write_word(0x0E, 0x3000);
    cpu.step(&mut ram);
    let r = cpu.registers();
    assert_eq!(r.pc, 0x3000);
    assert_eq!(r.ssp, 0x8000 - 14);
    // Endereço que falhou no quadro.
    assert_eq!(ram.word(0x8000 - 12), 0x0000);
    assert_eq!(ram.word(0x8000 - 10), 0x2001);
}

#[test]
fn interrupt_uses_autovector_and_masks_level() {
    struct Irq(Ram, u8);
    impl Bus for Irq {
        fn read_byte(&mut self, a: u32) -> u8 {
            self.0.read_byte(a)
        }
        fn read_word(&mut self, a: u32) -> u16 {
            self.0.read_word(a)
        }
        fn write_byte(&mut self, a: u32, v: u8) {
            self.0.write_byte(a, v)
        }
        fn write_word(&mut self, a: u32, v: u16) {
            self.0.write_word(a, v)
        }
        fn interrupt_level(&self) -> u8 {
            self.1
        }
        fn acknowledge_interrupt(&mut self, _: u8) {
            self.1 = 0;
        }
    }
    let (mut cpu, ram) = boot(&[0x4E71, 0x4E71]);
    let mut bus = Irq(ram, 6);
    bus.0.write_word(0x78, 0x0000); // vetor 30 (nível 6)
    bus.0.write_word(0x7A, 0x4000);
    // Máscara 7: nada acontece.
    cpu.step(&mut bus);
    assert_eq!(cpu.registers().pc, 0x1002);
    // Máscara 3: aceita o nível 6.
    let mut regs = cpu.registers();
    regs.sr = sr::S | 0x0300;
    cpu.set_registers(&regs);
    bus.1 = 6;
    cpu.step(&mut bus);
    let r = cpu.registers();
    assert_eq!(r.pc, 0x4000);
    assert_eq!(r.sr & sr::INT_MASK, 0x0600);
    assert_eq!(bus.0.word(0x8000 - 2), 0x1002);
}

#[test]
fn state_round_trips() {
    let (mut cpu, mut ram) = boot(&[0x7005, 0x5680, 0x60FE]);
    cpu.step(&mut ram);
    let mut w = chips::state::Writer::default();
    cpu.save_state(&mut w);

    let mut restored = Cpu::new();
    let mut r = chips::state::Reader::new(&w.bytes);
    restored.load_state(&mut r).expect("estado válido");
    assert_eq!(r.finish(), Ok(()));
    assert_eq!(restored.registers(), cpu.registers());
    assert_eq!(restored.step(&mut ram), cpu.step(&mut ram));
    assert_eq!(restored.registers(), cpu.registers());
}
