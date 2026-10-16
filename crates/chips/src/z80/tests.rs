use super::flag::*;
use super::{Bus, Z80};

/// 64 KB de RAM plana, portas registradas e linha de interrupção manual.
struct TestBus {
    mem: Vec<u8>,
    outputs: Vec<(u16, u8)>,
    input_value: u8,
    int: bool,
}

impl TestBus {
    fn with_program(program: &[u8]) -> Self {
        let mut mem = vec![0; 0x10000];
        mem[..program.len()].copy_from_slice(program);
        Self {
            mem,
            outputs: Vec::new(),
            input_value: 0,
            int: false,
        }
    }
}

impl Bus for TestBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.mem[usize::from(addr)]
    }
    fn write(&mut self, addr: u16, value: u8) {
        self.mem[usize::from(addr)] = value;
    }
    fn input(&mut self, _port: u16) -> u8 {
        self.input_value
    }
    fn output(&mut self, port: u16, value: u8) {
        self.outputs.push((port, value));
    }
    fn int_line(&self) -> bool {
        self.int
    }
}

fn run(program: &[u8], steps: usize) -> (Z80, TestBus, u32) {
    let mut cpu = Z80::new();
    let mut bus = TestBus::with_program(program);
    cpu.sp = 0xFFF0;
    let cycles = (0..steps).map(|_| cpu.step(&mut bus)).sum();
    (cpu, bus, cycles)
}

#[test]
fn add_sets_overflow_half_and_sign() {
    // LD A,7Fh ; ADD A,1
    let (cpu, _, _) = run(&[0x3E, 0x7F, 0xC6, 0x01], 2);
    assert_eq!(cpu.a, 0x80);
    assert_eq!(cpu.f, S | H | PV);
}

#[test]
fn sub_borrow_sets_carry_and_negative() {
    // XOR A ; SUB 1
    let (cpu, _, _) = run(&[0xAF, 0xD6, 0x01], 2);
    assert_eq!(cpu.a, 0xFF);
    assert_eq!(cpu.f, S | Y | H | X | N | C);
}

#[test]
fn cp_takes_undocumented_bits_from_operand() {
    // XOR A ; CP 28h
    let (cpu, _, _) = run(&[0xAF, 0xFE, 0x28], 2);
    assert_eq!(cpu.a, 0);
    assert_eq!(cpu.f & (X | Y), X | Y);
    assert_ne!(cpu.f & C, 0);
}

#[test]
fn adc_and_sbc_use_carry() {
    // SCF ; LD A,10h ; ADC A,0Fh  -> 20h
    let (cpu, _, _) = run(&[0x37, 0x3E, 0x10, 0xCE, 0x0F], 3);
    assert_eq!(cpu.a, 0x20);
    // SCF ; LD A,10h ; SBC A,0Fh -> 00h
    let (cpu, _, _) = run(&[0x37, 0x3E, 0x10, 0xDE, 0x0F], 3);
    assert_eq!(cpu.a, 0x00);
    assert_ne!(cpu.f & Z, 0);
}

#[test]
fn daa_adjusts_bcd_addition_and_subtraction() {
    // LD A,15h ; ADD A,27h ; DAA -> 42h
    let (cpu, _, _) = run(&[0x3E, 0x15, 0xC6, 0x27, 0x27], 3);
    assert_eq!(cpu.a, 0x42);
    // LD A,42h ; SUB 15h ; DAA -> 27h
    let (cpu, _, _) = run(&[0x3E, 0x42, 0xD6, 0x15, 0x27], 3);
    assert_eq!(cpu.a, 0x27);
    assert_ne!(cpu.f & N, 0);
}

#[test]
fn inc_dec_overflow_edges_keep_carry() {
    // SCF ; LD B,7Fh ; INC B
    let (cpu, _, _) = run(&[0x37, 0x06, 0x7F, 0x04], 3);
    assert_eq!(cpu.b, 0x80);
    assert_eq!(cpu.f, S | H | PV | C);
    // LD C,80h ; DEC C
    let (cpu, _, _) = run(&[0x0E, 0x80, 0x0D], 2);
    assert_eq!(cpu.c, 0x7F);
    assert_eq!(cpu.f & (PV | H | N), PV | H | N);
}

#[test]
fn logic_ops_set_parity() {
    // LD A,0F0h ; AND 0Fh
    let (cpu, _, _) = run(&[0x3E, 0xF0, 0xE6, 0x0F], 2);
    assert_eq!(cpu.f, Z | H | PV);
    // LD A,1 ; OR 2  -> 03h, paridade par
    let (cpu, _, _) = run(&[0x3E, 0x01, 0xF6, 0x02], 2);
    assert_eq!(cpu.f, PV);
}

#[test]
fn add16_and_sbc16() {
    // LD HL,0FFFFh ; LD BC,1 ; ADD HL,BC
    let (cpu, _, _) = run(&[0x21, 0xFF, 0xFF, 0x01, 0x01, 0x00, 0x09], 3);
    assert_eq!(cpu.hl(), 0);
    assert_eq!(cpu.f & (C | H), C | H);
    // LD HL,8000h ; LD DE,1 ; OR A ; SBC HL,DE -> 7FFFh, overflow
    let (cpu, _, _) = run(&[0x21, 0x00, 0x80, 0x11, 0x01, 0x00, 0xB7, 0xED, 0x52], 4);
    assert_eq!(cpu.hl(), 0x7FFF);
    assert_ne!(cpu.f & PV, 0);
    assert_ne!(cpu.f & N, 0);
}

#[test]
fn cb_rotations_and_bits() {
    // LD B,81h ; RLC B ; SRL B ; BIT 7,B
    let (cpu, _, _) = run(&[0x06, 0x81, 0xCB, 0x00, 0xCB, 0x38, 0xCB, 0x78], 4);
    assert_eq!(cpu.b, 0x01);
    assert_ne!(cpu.f & Z, 0);
    // LD A,1 ; SLL A (não documentada) -> 03h
    let (cpu, _, _) = run(&[0x3E, 0x01, 0xCB, 0x37], 2);
    assert_eq!(cpu.a, 0x03);
}

#[test]
fn bit_hl_takes_undocumented_bits_from_memptr() {
    // LD A,(2800h) define WZ = 2801h ; LD HL,0 ; BIT 0,(HL)
    let (cpu, _, _) = run(&[0x3A, 0x00, 0x28, 0x21, 0x00, 0x00, 0xCB, 0x46], 3);
    assert_eq!(cpu.f & (X | Y), X | Y);
}

#[test]
fn indexed_ops_use_displacement_and_real_h() {
    // LD IX,1000h ; LD (IX+5),42h ; LD H,(IX+5)
    let program = [
        0xDD, 0x21, 0x00, 0x10, 0xDD, 0x36, 0x05, 0x42, 0xDD, 0x66, 0x05,
    ];
    let (cpu, bus, cycles) = run(&program, 3);
    assert_eq!(bus.mem[0x1005], 0x42);
    assert_eq!(cpu.h, 0x42);
    assert_eq!(cpu.ix, 0x1000);
    assert_eq!(cycles, 14 + 19 + 19);
}

#[test]
fn indexed_negative_displacement() {
    // LD IY,1000h ; LD (IY-1),A
    let (_, bus, _) = run(&[0xFD, 0x21, 0x00, 0x10, 0xFD, 0x77, 0xFF], 2);
    assert_eq!(bus.mem[0x0FFF], 0xFF);
}

#[test]
fn undocumented_ixh_ixl() {
    // LD IXH,12h ; LD IXL,34h ; LD A,IXH
    let (cpu, _, _) = run(&[0xDD, 0x26, 0x12, 0xDD, 0x2E, 0x34, 0xDD, 0x7C], 3);
    assert_eq!(cpu.ix, 0x1234);
    assert_eq!(cpu.a, 0x12);
}

#[test]
fn indexed_cb_copies_result_into_register() {
    // LD IX,1000h ; SET 0,(IX+2)->B ; RLC (IX+2)
    let mut program = vec![0xDD, 0x21, 0x00, 0x10, 0xDD, 0xCB, 0x02, 0xC0];
    program.extend([0xDD, 0xCB, 0x02, 0x06]);
    let (cpu, bus, cycles) = run(&program, 3);
    assert_eq!(cpu.b, 0x01);
    assert_eq!(bus.mem[0x1002], 0x02);
    assert_eq!(cycles, 14 + 23 + 23);
}

#[test]
fn ldir_copies_block_with_repeat_timing() {
    // LD HL,100h ; LD DE,200h ; LD BC,3 ; LDIR
    let mut program = vec![
        0x21, 0x00, 0x01, 0x11, 0x00, 0x02, 0x01, 0x03, 0x00, 0xED, 0xB0,
    ];
    program.resize(0x100, 0);
    program.extend([0xAA, 0xBB, 0xCC]);
    let (cpu, bus, cycles) = run(&program, 6);
    assert_eq!(&bus.mem[0x200..0x203], &[0xAA, 0xBB, 0xCC]);
    assert_eq!(cpu.bc(), 0);
    assert_eq!(cpu.f & PV, 0);
    assert_eq!(cpu.pc, 11);
    assert_eq!(cycles, 30 + 21 + 21 + 16);
}

#[test]
fn cpir_stops_on_match() {
    // LD HL,100h ; LD BC,10 ; LD A,0BBh ; CPIR
    let mut program = vec![0x21, 0x00, 0x01, 0x01, 0x0A, 0x00, 0x3E, 0xBB, 0xED, 0xB1];
    program.resize(0x100, 0);
    program.extend([0xAA, 0xBB, 0xCC]);
    let (cpu, _, _) = run(&program, 5);
    assert_eq!(cpu.hl(), 0x102);
    assert_eq!(cpu.bc(), 8);
    assert_ne!(cpu.f & Z, 0);
    assert_ne!(cpu.f & PV, 0);
}

#[test]
fn otir_writes_each_byte_to_port() {
    // LD HL,100h ; LD BC,02BEh ; OTIR
    let mut program = vec![0x21, 0x00, 0x01, 0x01, 0xBE, 0x02, 0xED, 0xB3];
    program.resize(0x100, 0);
    program.extend([0x11, 0x22]);
    let (cpu, bus, _) = run(&program, 4);
    assert_eq!(bus.outputs, vec![(0x01BE, 0x11), (0x00BE, 0x22)]);
    assert_eq!(cpu.b, 0);
    assert_ne!(cpu.f & Z, 0);
}

#[test]
fn call_ret_push_pop() {
    // LD BC,1234h ; PUSH BC ; POP DE ; CALL 10h ; ... 10h: RET
    let mut program = vec![0x01, 0x34, 0x12, 0xC5, 0xD1, 0xCD, 0x10, 0x00];
    program.resize(0x10, 0);
    program.push(0xC9);
    let (cpu, _, cycles) = run(&program, 5);
    assert_eq!(cpu.de(), 0x1234);
    assert_eq!(cpu.pc, 0x0008);
    assert_eq!(cpu.sp, 0xFFF0);
    assert_eq!(cycles, 10 + 11 + 10 + 17 + 10);
}

#[test]
fn conditional_jumps_and_djnz() {
    // LD B,3 ; DJNZ -2 (loop) ; XOR A ; JR Z,+1 ; HALT ; NOP
    let program = [0x06, 0x03, 0x10, 0xFE, 0xAF, 0x28, 0x01, 0x76, 0x00];
    let (cpu, _, cycles) = run(&program, 6);
    assert_eq!(cpu.b, 0);
    assert!(!cpu.halted);
    assert_eq!(cpu.pc, 8);
    assert_eq!(cycles, 7 + 13 + 13 + 8 + 4 + 12);
}

#[test]
fn exchange_instructions() {
    // LD HL,1111h ; LD DE,2222h ; EX DE,HL ; EXX ; EX AF,AF'
    let (cpu, _, _) = run(&[0x21, 0x11, 0x11, 0x11, 0x22, 0x22, 0xEB, 0xD9, 0x08], 5);
    assert_eq!(cpu.hl_alt, 0x2222);
    assert_eq!(cpu.de_alt, 0x1111);
    assert_eq!(cpu.hl(), 0);
    assert_eq!(cpu.af_alt, 0xFFFF);
}

#[test]
fn ei_delays_interrupt_by_one_instruction() {
    // IM 1 ; EI ; NOP ; NOP
    let mut cpu = Z80::new();
    let mut bus = TestBus::with_program(&[0xED, 0x56, 0xFB, 0x00, 0x00]);
    bus.int = true;
    cpu.sp = 0xFFF0;
    cpu.step(&mut bus); // IM 1
    cpu.step(&mut bus); // EI
    cpu.step(&mut bus); // NOP: ainda sem interrupção
    assert_eq!(cpu.pc, 4);
    assert_eq!(cpu.step(&mut bus), 13);
    assert_eq!(cpu.pc, 0x38);
    assert!(!cpu.iff1);
    assert_eq!(bus.mem[0xFFEE], 4);
}

#[test]
fn halt_waits_for_interrupt() {
    // IM 1 ; EI ; HALT
    let mut cpu = Z80::new();
    let mut bus = TestBus::with_program(&[0xED, 0x56, 0xFB, 0x76]);
    cpu.sp = 0xFFF0;
    for _ in 0..5 {
        cpu.step(&mut bus);
    }
    assert!(cpu.halted);
    assert_eq!(cpu.pc, 4);
    bus.int = true;
    cpu.step(&mut bus);
    assert!(!cpu.halted);
    assert_eq!(cpu.pc, 0x38);
    // Retorna para depois do HALT.
    assert_eq!(bus.mem[0xFFEE], 4);
}

#[test]
fn nmi_preserves_iff2_and_retn_restores() {
    // EI ; NOP ; ... 66h: RETN
    let mut program = vec![0xFB, 0x00];
    program.resize(0x66, 0);
    program.extend([0xED, 0x45]);
    let mut cpu = Z80::new();
    let mut bus = TestBus::with_program(&program);
    cpu.sp = 0xFFF0;
    cpu.step(&mut bus);
    cpu.request_nmi();
    assert_eq!(cpu.step(&mut bus), 11);
    assert_eq!(cpu.pc, 0x66);
    assert!(!cpu.iff1);
    assert!(cpu.iff2);
    cpu.step(&mut bus);
    assert!(cpu.iff1);
    assert_eq!(cpu.pc, 1);
}

#[test]
fn im2_reads_vector_table() {
    // LD A,80h ; LD I,A ; IM 2 ; EI ; NOP
    let mut cpu = Z80::new();
    let mut bus = TestBus::with_program(&[0x3E, 0x80, 0xED, 0x47, 0xED, 0x5E, 0xFB, 0x00]);
    bus.mem[0x80FF] = 0x34;
    bus.mem[0x8100] = 0x12;
    cpu.sp = 0xFFF0;
    for _ in 0..5 {
        cpu.step(&mut bus);
    }
    bus.int = true;
    assert_eq!(cpu.step(&mut bus), 19);
    assert_eq!(cpu.pc, 0x1234);
}

#[test]
fn io_instructions_use_full_port_address() {
    // LD A,12h ; OUT (34h),A ; LD BC,5678h ; OUT (C),A ; IN D,(C)
    let mut cpu = Z80::new();
    let mut bus = TestBus::with_program(&[
        0x3E, 0x12, 0xD3, 0x34, 0x01, 0x78, 0x56, 0xED, 0x79, 0xED, 0x50,
    ]);
    bus.input_value = 0x80;
    for _ in 0..5 {
        cpu.step(&mut bus);
    }
    assert_eq!(bus.outputs, vec![(0x1234, 0x12), (0x5678, 0x12)]);
    assert_eq!(cpu.d, 0x80);
    assert_ne!(cpu.f & S, 0);
}

#[test]
fn rld_rrd_rotate_nibbles() {
    // LD HL,100h ; LD A,12h ; RLD
    let mut program = vec![0x21, 0x00, 0x01, 0x3E, 0x12, 0xED, 0x6F];
    program.resize(0x100, 0);
    program.push(0x34);
    let (cpu, bus, _) = run(&program, 3);
    assert_eq!(cpu.a, 0x13);
    assert_eq!(bus.mem[0x100], 0x42);
}

#[test]
fn scf_ccf_follow_q_register() {
    // POP AF não altera Q: SCF usa (F | A) nos bits 3/5.
    let mut cpu = Z80::new();
    let mut bus = TestBus::with_program(&[0xF1, 0x37]);
    cpu.sp = 0x8000;
    bus.mem[0x8000] = 0x28; // F
    bus.mem[0x8001] = 0x00; // A
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.f & (X | Y), X | Y);

    // Depois de uma instrução que grava flags, Q = F: os bits 3/5 vêm só de A.
    // XOR A ; LD B,29h ; DEC B (F fica com X|Y) ; SCF
    let (cpu, _, _) = run(&[0xAF, 0x06, 0x29, 0x05, 0x37], 4);
    assert_eq!(cpu.f & (X | Y), 0);
}

#[test]
fn r_register_counts_prefixes() {
    // NOP ; LD IX,0 (DD conta) ; CB 00 (CB conta)
    let (cpu, _, _) = run(&[0x00, 0xDD, 0x21, 0x00, 0x00, 0xCB, 0x00], 3);
    assert_eq!(cpu.r, 5);
}

#[test]
fn neg_and_cpl() {
    // LD A,1 ; NEG ; CPL
    let (cpu, _, _) = run(&[0x3E, 0x01, 0xED, 0x44], 2);
    assert_eq!(cpu.a, 0xFF);
    assert_ne!(cpu.f & C, 0);
    let (cpu, _, _) = run(&[0x3E, 0x0F, 0x2F], 2);
    assert_eq!(cpu.a, 0xF0);
    assert_eq!(cpu.f & (H | N), H | N);
}
