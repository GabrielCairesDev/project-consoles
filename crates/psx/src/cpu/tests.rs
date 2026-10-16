use super::*;
use crate::bus::BIOS_SIZE;
use crate::irq::Irq;

fn r(funct: u32, rs: u32, rt: u32, rd: u32, sa: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | (sa << 6) | funct
}

fn i(op: u32, rs: u32, rt: u32, imm: i32) -> u32 {
    (op << 26) | (rs << 21) | (rt << 16) | (imm as u32 & 0xFFFF)
}

const NOP: u32 = 0;

fn addiu(rt: u32, rs: u32, imm: i32) -> u32 {
    i(0x09, rs, rt, imm)
}
fn addu(rd: u32, rs: u32, rt: u32) -> u32 {
    r(0x21, rs, rt, rd, 0)
}
fn lw(rt: u32, base: u32, offset: i32) -> u32 {
    i(0x23, base, rt, offset)
}
fn mtc0(rt: u32, rd: u32) -> u32 {
    (0x10 << 26) | (4 << 21) | (rt << 16) | (rd << 11)
}
fn lui(rt: u32, imm: i32) -> u32 {
    i(0x0F, 0, rt, imm)
}
fn ori(rt: u32, rs: u32, imm: i32) -> u32 {
    i(0x0D, rs, rt, imm)
}

/// CPU com o programa no começo do BIOS (onde ela inicia).
fn machine(program: &[u32]) -> (Cpu, Bus) {
    let mut bios = Box::new([0; BIOS_SIZE]);
    for (n, word) in program.iter().enumerate() {
        bios[n * 4..n * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    (Cpu::default(), Bus::new(bios))
}

fn run(cpu: &mut Cpu, bus: &mut Bus, steps: usize) {
    for _ in 0..steps {
        cpu.step(bus);
    }
}

#[test]
fn load_result_is_visible_one_instruction_late() {
    let (mut cpu, mut bus) = machine(&[
        addiu(2, 0, 7),
        lw(2, 0, 0x100),
        addu(3, 2, 0),
        addu(4, 2, 0),
    ]);
    bus.write32(0x100, 42);
    run(&mut cpu, &mut bus, 4);
    assert_eq!(cpu.regs[3], 7, "delay slot do load vê o valor antigo");
    assert_eq!(cpu.regs[4], 42);
}

#[test]
fn write_in_load_delay_slot_wins() {
    let (mut cpu, mut bus) = machine(&[lw(2, 0, 0x100), addiu(2, 0, 5), addu(3, 2, 0)]);
    bus.write32(0x100, 42);
    run(&mut cpu, &mut bus, 3);
    assert_eq!(cpu.regs[3], 5);
}

#[test]
fn branch_delay_slot_always_executes() {
    let (mut cpu, mut bus) = machine(&[
        i(0x04, 0, 0, 2), // BEQ r0, r0, +2
        addiu(5, 0, 1),   // delay slot
        addiu(6, 0, 1),   // pulada
        addiu(7, 0, 1),   // destino
    ]);
    run(&mut cpu, &mut bus, 3);
    assert_eq!((cpu.regs[5], cpu.regs[6], cpu.regs[7]), (1, 0, 1));
}

#[test]
fn jal_links_past_the_delay_slot() {
    let target = (RESET_VECTOR & 0x0FFF_FFFF) / 4 + 4;
    let (mut cpu, mut bus) = machine(&[(0x03 << 26) | target, NOP]);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.regs[31], RESET_VECTOR + 8);
    assert_eq!(cpu.pc, RESET_VECTOR + 16);
}

#[test]
fn bltzal_links_even_when_not_taken() {
    // BLTZAL r0 (0 não é < 0): não desvia, mas grava o r31.
    let (mut cpu, mut bus) = machine(&[i(0x01, 0, 0x10, 4), NOP, NOP]);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.regs[31], RESET_VECTOR + 8);
    assert_eq!(cpu.pc, RESET_VECTOR + 8);
}

#[test]
fn add_overflow_raises_exception_without_writing() {
    let (mut cpu, mut bus) = machine(&[
        lui(1, 0x7FFF),
        ori(1, 1, 0xFFFF),
        addiu(3, 0, 9),
        r(0x20, 1, 1, 3, 0), // ADD r3, r1, r1
    ]);
    run(&mut cpu, &mut bus, 4);
    assert_eq!(cpu.regs[3], 9);
    assert_eq!(cpu.pc, 0x8000_0080);
    assert_eq!(cpu.cop0.epc, RESET_VECTOR + 12);
    assert_eq!((cpu.cop0.cause >> 2) & 0x1F, Exception::Overflow as u32);
}

#[test]
fn exception_in_delay_slot_points_to_the_branch() {
    let (mut cpu, mut bus) = machine(&[
        i(0x04, 0, 0, 4),
        r(0x0C, 0, 0, 0, 0), // SYSCALL no delay slot
    ]);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.cop0.epc, RESET_VECTOR);
    assert_ne!(cpu.cop0.cause & CAUSE_BD, 0);
    assert_eq!((cpu.cop0.cause >> 2) & 0x1F, Exception::Syscall as u32);
}

#[test]
fn misaligned_load_raises_address_error() {
    let (mut cpu, mut bus) = machine(&[lw(2, 0, 0x102)]);
    run(&mut cpu, &mut bus, 1);
    assert_eq!((cpu.cop0.cause >> 2) & 0x1F, Exception::LoadAddress as u32);
    assert_eq!(cpu.cop0.bad_vaddr, 0x102);
}

#[test]
fn division_edge_cases() {
    let (mut cpu, mut bus) = machine(&[
        addiu(1, 0, -7),
        r(0x1A, 1, 0, 0, 0), // DIV r1, r0
        r(0x10, 0, 0, 2, 0), // MFHI r2
        r(0x12, 0, 0, 3, 0), // MFLO r3
        r(0x1B, 1, 0, 0, 0), // DIVU r1, r0
        r(0x12, 0, 0, 4, 0),
        lui(5, -0x8000),
        addiu(6, 0, -1),
        r(0x1A, 5, 6, 0, 0), // 0x80000000 / -1
        r(0x12, 0, 0, 7, 0),
        r(0x10, 0, 0, 8, 0),
    ]);
    let start = bus.now;
    run(&mut cpu, &mut bus, 11);
    assert_eq!((cpu.regs[2], cpu.regs[3]), (-7i32 as u32, 1));
    assert_eq!(cpu.regs[4], 0xFFFF_FFFF);
    assert_eq!((cpu.regs[7], cpu.regs[8]), (0x8000_0000, 0));
    assert!(bus.now - start >= 3 * DIV_LATENCY, "MFLO espera o DIV");
}

#[test]
fn signed_and_unsigned_multiply() {
    let (mut cpu, mut bus) = machine(&[
        addiu(1, 0, -3),
        addiu(2, 0, 5),
        r(0x18, 1, 2, 0, 0), // MULT
        r(0x10, 0, 0, 3, 0),
        r(0x12, 0, 0, 4, 0),
        r(0x19, 1, 2, 0, 0), // MULTU
        r(0x10, 0, 0, 5, 0),
    ]);
    run(&mut cpu, &mut bus, 7);
    assert_eq!((cpu.regs[3], cpu.regs[4]), (0xFFFF_FFFF, -15i32 as u32));
    assert_eq!(cpu.regs[5], 4, "0xFFFFFFFD × 5 = 0x4_FFFF_FFF1");
}

#[test]
fn unaligned_load_and_store_pairs() {
    let (mut cpu, mut bus) = machine(&[
        i(0x26, 0, 1, 0x101), // LWR r1, 0x101
        i(0x22, 0, 1, 0x104), // LWL r1, 0x104 (junta com o load em andamento)
        NOP,
        i(0x2E, 0, 1, 0x201), // SWR r1, 0x201
        i(0x2A, 0, 1, 0x204), // SWL r1, 0x204
    ]);
    bus.write32(0x100, 0x4433_2211);
    bus.write32(0x104, 0x8877_6655);
    run(&mut cpu, &mut bus, 5);
    assert_eq!(cpu.regs[1], 0x5544_3322);
    assert_eq!(bus.read32(0x200), 0x4433_2200);
    assert_eq!(bus.read32(0x204), 0x0000_0055);
}

#[test]
fn hardware_interrupt_enters_handler_and_rfe_restores_mode() {
    let (mut cpu, mut bus) = machine(&[
        lui(1, 0),
        ori(1, 1, 0x0401), // IEc + IM2 (interrupção de hardware)
        mtc0(1, 12),
        NOP,
        NOP,
    ]);
    bus.write32(0x1F80_1074, 1);
    run(&mut cpu, &mut bus, 4);
    bus.irq.request(Irq::Vblank);
    let interrupted = cpu.pc;
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8000_0080);
    assert_eq!(cpu.cop0.epc, interrupted);
    assert_eq!((cpu.cop0.cause >> 2) & 0x1F, Exception::Interrupt as u32);
    assert_eq!(
        cpu.cop0.sr & 0x3F,
        0x04,
        "modo empilhado, interrupções desligadas"
    );

    // RFE no handler desempilha.
    bus.write32(0x80, 0x4200_0010);
    cpu.step(&mut bus);
    assert_eq!(cpu.cop0.sr & 0x3F, 0x01);
}

#[test]
fn isolated_cache_drops_stores() {
    let (mut cpu, mut bus) = machine(&[
        lui(1, 1),
        mtc0(1, 12), // SR.IsC
        addiu(2, 0, 0x55),
        i(0x2B, 0, 2, 0x100), // SW r2, 0x100
    ]);
    run(&mut cpu, &mut bus, 4);
    assert_eq!(bus.read32(0x100), 0);
}

#[test]
fn coprocessor_2_requires_enable() {
    let (mut cpu, mut bus) = machine(&[(0x12 << 26) | (4 << 21)]); // MTC2
    run(&mut cpu, &mut bus, 1);
    assert_eq!(
        (cpu.cop0.cause >> 2) & 0x1F,
        Exception::CoprocessorUnusable as u32
    );
    assert_eq!((cpu.cop0.cause >> 28) & 3, 2);
}
