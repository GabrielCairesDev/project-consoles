use super::*;
use crate::bus::{INT_TIMER, INT_VBLANK};
use crate::cart::Cart;

/// Barramento com `program` em 0x100 (cartucho só ROM) e o LCD desligado,
/// para a PPU não interferir.
fn machine(program: &[u8]) -> (Cpu, Bus) {
    let mut rom = vec![0; 0x8000];
    rom[0x100..0x100 + program.len()].copy_from_slice(program);
    let mut bus = Bus::new(
        Cart::new(rom).expect("cartucho"),
        crate::apu::Apu::new(44_100),
    );
    bus.ppu.write(0xFF40, 0);
    bus.interrupt_flag = 0;
    (Cpu::default(), bus)
}

/// Executa uma instrução e devolve os ciclos (T) gastos.
fn step(cpu: &mut Cpu, bus: &mut Bus) -> u64 {
    let start = bus.cycles;
    cpu.step(bus);
    bus.cycles - start
}

#[test]
fn starts_in_post_boot_state() {
    let cpu = Cpu::default();
    assert_eq!((cpu.a, cpu.f, cpu.sp, cpu.pc), (0x01, 0xB0, 0xFFFE, 0x0100));
}

#[test]
fn add_and_sub_set_half_carry_and_carry() {
    // LD A,0x0F; ADD A,0x01; LD A,0xF0; ADD A,0x20; SUB 0x31
    let (mut cpu, mut bus) = machine(&[0x3E, 0x0F, 0xC6, 0x01, 0x3E, 0xF0, 0xC6, 0x20, 0xD6, 0x31]);
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    assert_eq!((cpu.a, cpu.f), (0x10, H));
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    assert_eq!((cpu.a, cpu.f), (0x10, C));
    step(&mut cpu, &mut bus);
    // 0x10 - 0x31: empresta no nibble e no byte.
    assert_eq!((cpu.a, cpu.f), (0xDF, N | H | C));
}

#[test]
fn daa_corrects_bcd_after_add_and_sub() {
    // LD A,0x45; ADD A,0x38; DAA; SUB 0x09; DAA
    let (mut cpu, mut bus) = machine(&[0x3E, 0x45, 0xC6, 0x38, 0x27, 0xD6, 0x09, 0x27]);
    for _ in 0..3 {
        step(&mut cpu, &mut bus);
    }
    assert_eq!(cpu.a, 0x83);
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    assert_eq!(cpu.a, 0x74);
}

#[test]
fn pop_af_clears_low_nibble_of_f() {
    // LD BC,0x12FF; PUSH BC; POP AF
    let (mut cpu, mut bus) = machine(&[0x01, 0xFF, 0x12, 0xC5, 0xF1]);
    for _ in 0..3 {
        step(&mut cpu, &mut bus);
    }
    assert_eq!((cpu.a, cpu.f), (0x12, 0xF0));
}

#[test]
fn instruction_cycles_match_hardware() {
    // NOP; LD BC,nn; CALL nn → 0x0110: RET NZ (Z=1, não salta), RET
    let mut program = vec![0x00, 0x01, 0x00, 0x00, 0xCD, 0x10, 0x01];
    program.resize(0x10, 0);
    program.extend([0xC0, 0xC9]);
    let (mut cpu, mut bus) = machine(&program);
    assert_eq!(step(&mut cpu, &mut bus), 4);
    assert_eq!(step(&mut cpu, &mut bus), 12);
    assert_eq!(step(&mut cpu, &mut bus), 24);
    assert_eq!(cpu.pc, 0x0110);
    assert_eq!(step(&mut cpu, &mut bus), 8, "RET NZ sem saltar (Z ligado)");
    assert_eq!(step(&mut cpu, &mut bus), 16);
    assert_eq!(cpu.pc, 0x0107);
}

#[test]
fn ei_enables_interrupts_after_next_instruction() {
    // EI; NOP; NOP
    let (mut cpu, mut bus) = machine(&[0xFB, 0x00, 0x00]);
    bus.interrupt_enable = INT_VBLANK;
    bus.interrupt_flag = INT_VBLANK;
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    assert_eq!(cpu.pc, 0x0102, "a instrução depois do EI ainda roda");
    assert_eq!(step(&mut cpu, &mut bus), 20, "atendimento leva 5 M-ciclos");
    assert_eq!(cpu.pc, 0x0040);
    assert_eq!(bus.interrupt_flag & INT_VBLANK, 0);
    assert_eq!(bus.peek(cpu.sp), 0x02);
}

#[test]
fn higher_priority_interrupt_wins() {
    let (mut cpu, mut bus) = machine(&[0x00]);
    cpu.ime = true;
    bus.interrupt_enable = 0x1F;
    bus.interrupt_flag = INT_TIMER | 0x10;
    step(&mut cpu, &mut bus);
    assert_eq!(cpu.pc, 0x0050);
    assert_eq!(bus.interrupt_flag, 0x10);
}

#[test]
fn push_over_ie_can_cancel_interrupt() {
    // SP = 0x0000: o byte alto do PC vai para 0xFFFF (IE) e o zera.
    let (mut cpu, mut bus) = machine(&[0x00]);
    cpu.ime = true;
    cpu.pc = 0x0000;
    cpu.sp = 0x0000;
    bus.interrupt_enable = INT_VBLANK;
    bus.interrupt_flag = INT_VBLANK;
    step(&mut cpu, &mut bus);
    assert_eq!(cpu.pc, 0x0000);
    assert_eq!(bus.interrupt_flag, INT_VBLANK, "nada foi atendido");
}

#[test]
fn halt_waits_for_interrupt_even_with_ime_off() {
    // HALT; INC A
    let (mut cpu, mut bus) = machine(&[0x76, 0x3C]);
    bus.interrupt_enable = INT_TIMER;
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    assert_eq!(cpu.pc, 0x0101, "parado no HALT");
    bus.interrupt_flag = INT_TIMER;
    step(&mut cpu, &mut bus);
    assert_eq!((cpu.pc, cpu.a), (0x0102, 0x02), "acorda sem atender");
}

#[test]
fn halt_bug_repeats_next_byte() {
    // HALT com interrupção pendente e IME desligado; INC A executa duas vezes.
    let (mut cpu, mut bus) = machine(&[0x76, 0x3C, 0x00]);
    bus.interrupt_enable = INT_TIMER;
    bus.interrupt_flag = INT_TIMER;
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    assert_eq!((cpu.a, cpu.pc), (0x03, 0x0102));
}

#[test]
fn cb_ops_on_memory_take_extra_cycles() {
    // LD HL,0xC000; LD (HL),0x81; RLC (HL); BIT 0,(HL); SWAP (HL)
    let (mut cpu, mut bus) = machine(&[
        0x21, 0x00, 0xC0, 0x36, 0x81, 0xCB, 0x06, 0xCB, 0x46, 0xCB, 0x36,
    ]);
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    assert_eq!(step(&mut cpu, &mut bus), 16);
    assert_eq!((bus.peek(0xC000), cpu.f), (0x03, C));
    assert_eq!(step(&mut cpu, &mut bus), 12);
    assert_eq!(cpu.f, H | C, "bit 0 ligado: Z desligado");
    step(&mut cpu, &mut bus);
    assert_eq!(bus.peek(0xC000), 0x30);
}

#[test]
fn add_sp_uses_low_byte_flags() {
    // LD SP,0x00FF; ADD SP,-1 (0xFF)
    let (mut cpu, mut bus) = machine(&[0x31, 0xFF, 0x00, 0xE8, 0xFF]);
    step(&mut cpu, &mut bus);
    assert_eq!(step(&mut cpu, &mut bus), 16);
    assert_eq!((cpu.sp, cpu.f), (0x00FE, H | C));
}

#[test]
fn illegal_opcode_locks_cpu() {
    let (mut cpu, mut bus) = machine(&[0xD3, 0x3C]);
    step(&mut cpu, &mut bus);
    for _ in 0..10 {
        step(&mut cpu, &mut bus);
    }
    assert_eq!((cpu.pc, cpu.a), (0x0101, 0x01));
}

#[test]
fn state_round_trip() {
    let (mut cpu, mut bus) = machine(&[0xFB, 0x76]);
    step(&mut cpu, &mut bus);
    let mut w = Writer::default();
    cpu.save_state(&mut w);
    let mut other = Cpu::default();
    let mut r = Reader::new(&w.bytes);
    other.load_state(&mut r).expect("estado válido");
    assert_eq!(r.finish(), Ok(()));
    assert_eq!(other, cpu);
}
