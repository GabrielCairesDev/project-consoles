//! Decodificação e execução.
//!
//! Os grupos seguem os 4 bits altos do opcode. Encodings que se sobrepõem
//! (ADDX dentro de ADD, CMPM dentro de EOR, EXG/ABCD dentro de AND, etc.)
//! são testados antes do caso geral. Opcodes inválidos geram a exceção de
//! instrução ilegal.

use super::arith::{self, Flags};
use super::ea::{Access, Class, Loc, valid};
use super::sr::{C, N, V, X, Z};
use super::{Bus, Cpu, Size, Step};

/// Vetores de exceção.
const ILLEGAL: u32 = 4;
const ZERO_DIVIDE: u32 = 5;
const CHK: u32 = 6;
const TRAPV: u32 = 7;
const PRIVILEGE: u32 = 8;
const LINE_A: u32 = 10;
const LINE_F: u32 = 11;
const TRAP_BASE: u32 = 32;

fn mode_reg(op: u16) -> (u16, u16) {
    ((op >> 3) & 7, op & 7)
}

fn reg_hi(op: u16) -> usize {
    usize::from((op >> 9) & 7)
}

fn sign_extend(size: Size, v: u32) -> u32 {
    match size {
        Size::Byte => v as u8 as i8 as i32 as u32,
        Size::Word => v as u16 as i16 as i32 as u32,
        Size::Long => v,
    }
}

impl Cpu {
    pub(super) fn execute<B: Bus>(&mut self, bus: &mut B) -> Step<()> {
        let op = self.ir;
        match op >> 12 {
            0x0 => self.group_0(bus, op),
            0x1 => self.op_move(bus, op, Size::Byte),
            0x2 => self.op_move(bus, op, Size::Long),
            0x3 => self.op_move(bus, op, Size::Word),
            0x4 => self.group_4(bus, op),
            0x5 => self.group_5(bus, op),
            0x6 => self.op_branch(bus, op),
            0x7 => self.op_moveq(bus, op),
            0x8 => self.group_8(bus, op),
            0x9 => self.group_add_sub(bus, op, false),
            0xA => self.illegal(bus, LINE_A),
            0xB => self.group_b(bus, op),
            0xC => self.group_c(bus, op),
            0xD => self.group_add_sub(bus, op, true),
            0xE => self.group_e(bus, op),
            _ => self.illegal(bus, LINE_F),
        }
    }

    fn illegal<B: Bus>(&mut self, bus: &mut B, vector: u32) -> Step<()> {
        let pc = self.instr_pc;
        self.raise(bus, vector, pc, 6)
    }

    fn privileged<B: Bus>(&mut self, bus: &mut B) -> Option<Step<()>> {
        if self.supervisor() {
            None
        } else {
            let pc = self.instr_pc;
            Some(self.raise(bus, PRIVILEGE, pc, 6))
        }
    }

    // ---- Flags ------------------------------------------------------------

    fn set_nz(&mut self, size: Size, value: u32) {
        let v = value & size.mask();
        self.set_flag(N, v & size.msb() != 0);
        self.set_flag(Z, v == 0);
    }

    /// Lógica/MOVE: N e Z do resultado, V e C zerados, X intacto.
    fn set_logic(&mut self, size: Size, value: u32) {
        self.set_nz(size, value);
        self.set_flag(V, false);
        self.set_flag(C, false);
    }

    /// Soma/subtração: todos os flags (X só se `set_x`).
    fn set_arith(&mut self, size: Size, f: Flags, set_x: bool) {
        self.set_nz(size, f.result);
        self.set_flag(V, f.overflow);
        self.set_flag(C, f.carry);
        if set_x {
            self.set_flag(X, f.carry);
        }
    }

    /// ADDX/SUBX/NEGX: Z só é zerado, nunca ligado (operações encadeadas).
    fn set_arith_extended(&mut self, size: Size, f: Flags) {
        let z = self.flag(Z) && f.result & size.mask() == 0;
        self.set_arith(size, f, true);
        self.set_flag(Z, z);
    }

    fn condition(&self, cc: u16) -> bool {
        let (n, z, v, c) = (self.flag(N), self.flag(Z), self.flag(V), self.flag(C));
        match cc & 0xF {
            0 => true,
            1 => false,
            2 => !c && !z,
            3 => c || z,
            4 => !c,
            5 => c,
            6 => !z,
            7 => z,
            8 => !v,
            9 => v,
            10 => !n,
            11 => n,
            12 => n == v,
            13 => n != v,
            14 => !z && n == v,
            _ => z || n != v,
        }
    }

    // ---- Grupo 0: bits, imediatos e MOVEP ---------------------------------

    fn group_0<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if op & 0x0138 == 0x0108 {
            return self.op_movep(bus, op);
        }
        if op & 0x0100 != 0 {
            // Bit dinâmico: número do bit em Dn.
            let bit = self.d[reg_hi(op)];
            return self.op_bit(bus, op, bit, false);
        }
        if op & 0x0F00 == 0x0800 {
            if !valid(mode, reg, Class::Data) || (mode == 7 && reg == 4) {
                return self.illegal(bus, ILLEGAL);
            }
            let bit = u32::from(self.fetch_ext(bus)? & 0xFF);
            return self.op_bit(bus, op, bit, true);
        }
        let kind = (op >> 9) & 7;
        // ORI/ANDI/EORI para CCR (.b) e SR (.w).
        if matches!(kind, 0 | 1 | 5) && mode == 7 && reg == 4 {
            return match (op >> 6) & 3 {
                0 => self.op_logic_ccr(bus, kind, false),
                1 => self.op_logic_ccr(bus, kind, true),
                _ => self.illegal(bus, ILLEGAL),
            };
        }
        let Some(size) = Size::from_bits(op >> 6) else {
            return self.illegal(bus, ILLEGAL);
        };
        let class = if kind == 6 {
            Class::DataNoImmediate
        } else {
            Class::DataAlterable
        };
        if !matches!(kind, 0 | 1 | 2 | 3 | 5 | 6)
            || !valid(mode, reg, class)
            || (kind == 6 && mode == 7 && reg >= 2)
        {
            return self.illegal(bus, ILLEGAL);
        }
        let imm = match size {
            Size::Long => self.fetch_ext_long(bus)?,
            _ => u32::from(self.fetch_ext(bus)?) & size.mask(),
        };
        let mut loc = self.ea(bus, mode, reg, size, true)?;
        let dst = self.read_loc(bus, &mut loc, size)?;
        let result = match kind {
            0 => {
                let r = dst | imm;
                self.set_logic(size, r);
                r
            }
            1 => {
                let r = dst & imm;
                self.set_logic(size, r);
                r
            }
            5 => {
                let r = dst ^ imm;
                self.set_logic(size, r);
                r
            }
            2 => {
                let f = arith::sub(size, imm, dst, false);
                self.set_arith(size, f, true);
                f.result
            }
            3 => {
                let f = arith::add(size, imm, dst, false);
                self.set_arith(size, f, true);
                f.result
            }
            _ => {
                let f = arith::sub(size, imm, dst, false);
                let x = self.flag(X);
                self.set_arith(size, f, false);
                self.set_flag(X, x);
                if mode == 0 && size == Size::Long {
                    self.idle(2);
                }
                return self.prefetch(bus);
            }
        };
        if mode == 0 && size == Size::Long {
            self.idle(4);
        }
        self.prefetch_then_write(bus, loc, size, result)
    }

    /// Prefetch e escrita do resultado, na ordem do hardware: em Dn a ordem
    /// não importa; em memória o 68000 grava antes do prefetch.
    fn prefetch_then_write<B: Bus>(
        &mut self,
        bus: &mut B,
        mut loc: Loc,
        size: Size,
        value: u32,
    ) -> Step<()> {
        self.write_loc(bus, &mut loc, size, value)?;
        self.prefetch(bus)
    }

    fn op_logic_ccr<B: Bus>(&mut self, bus: &mut B, kind: u16, to_sr: bool) -> Step<()> {
        if to_sr && let Some(r) = self.privileged(bus) {
            return r;
        }
        let imm = self.fetch_ext(bus)?;
        let current = self.sr;
        let value = match kind {
            0 => current | imm,
            1 => current & imm,
            _ => current ^ imm,
        };
        if to_sr {
            self.set_sr(value);
        } else {
            self.set_ccr(value);
        }
        self.idle(8);
        // O 68000 recarrega a fila depois de mudar SR/CCR.
        let pc = self.pc;
        self.jump(bus, pc)
    }

    fn op_bit<B: Bus>(&mut self, bus: &mut B, op: u16, bit: u32, immediate: bool) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        let kind = (op >> 6) & 3;
        let class = if kind == 0 {
            Class::Data
        } else {
            Class::DataAlterable
        };
        if !valid(mode, reg, class) || (immediate && kind == 0 && mode == 7 && reg == 4) {
            return self.illegal(bus, ILLEGAL);
        }
        if mode == 0 {
            let n = bit % 32;
            let r = usize::from(reg);
            let mask = 1u32 << n;
            self.set_flag(Z, self.d[r] & mask == 0);
            match kind {
                0 => self.idle(2),
                1 => {
                    self.d[r] ^= mask;
                    self.idle(if n < 16 { 2 } else { 4 });
                }
                2 => {
                    self.d[r] &= !mask;
                    self.idle(if n < 16 { 4 } else { 6 });
                }
                _ => {
                    self.d[r] |= mask;
                    self.idle(if n < 16 { 2 } else { 4 });
                }
            }
            return self.prefetch(bus);
        }
        let mut loc = self.ea(bus, mode, reg, Size::Byte, true)?;
        let value = self.read_loc(bus, &mut loc, Size::Byte)?;
        let mask = 1u32 << (bit % 8);
        self.set_flag(Z, value & mask == 0);
        let result = match kind {
            0 => {
                if mode == 7 && reg == 4 {
                    self.idle(2);
                }
                return self.prefetch(bus);
            }
            1 => value ^ mask,
            2 => value & !mask,
            _ => value | mask,
        };
        self.prefetch_then_write(bus, loc, Size::Byte, result)
    }

    fn op_movep<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let dreg = reg_hi(op);
        let areg = usize::from(op & 7);
        let disp = self.fetch_ext(bus)? as i16;
        let mut addr = self.a[areg].wrapping_add_signed(i32::from(disp));
        let (long, to_memory) = match (op >> 6) & 3 {
            0 => (false, false),
            1 => (true, false),
            2 => (false, true),
            _ => (true, true),
        };
        let count = if long { 4 } else { 2 };
        if to_memory {
            let value = self.d[dreg];
            for i in (0..count).rev() {
                self.write_byte(bus, addr, (value >> (i * 8)) as u8);
                addr = addr.wrapping_add(2);
            }
        } else {
            let mut value = 0u32;
            for _ in 0..count {
                value = value << 8 | u32::from(self.read_byte(bus, addr));
                addr = addr.wrapping_add(2);
            }
            if long {
                self.d[dreg] = value;
            } else {
                self.d[dreg] = (self.d[dreg] & 0xFFFF_0000) | value;
            }
        }
        self.prefetch(bus)
    }

    // ---- MOVE -------------------------------------------------------------

    fn op_move<B: Bus>(&mut self, bus: &mut B, op: u16, size: Size) -> Step<()> {
        let (src_mode, src_reg) = mode_reg(op);
        let dst_reg = (op >> 9) & 7;
        let dst_mode = (op >> 6) & 7;
        if !valid(src_mode, src_reg, Class::All)
            || (size == Size::Byte && src_mode == 1)
            || !(dst_mode == 1 || valid(dst_mode, dst_reg, Class::DataAlterable))
            || (dst_mode == 1 && size == Size::Byte)
        {
            return self.illegal(bus, ILLEGAL);
        }
        let mut src = self.ea(bus, src_mode, src_reg, size, true)?;
        let value = self.read_loc(bus, &mut src, size)?;
        if dst_mode == 1 {
            self.a[usize::from(dst_reg)] = sign_extend(size, value);
            return self.prefetch(bus);
        }
        // Ordem do hardware: em -(An) o prefetch vem antes da escrita; em
        // abs.L com origem na memória, a escrita acontece antes de buscar a
        // última palavra.
        let source_in_memory = src_mode >= 2 && !(src_mode == 7 && src_reg == 4);
        match (dst_mode, dst_reg) {
            (4, _) => {
                let mut dst =
                    self.ea_with(bus, dst_mode, dst_reg, size, false, Access::MoveDestination)?;
                self.set_logic(size, value);
                self.prefetch(bus)?;
                self.write_loc(bus, &mut dst, size, value)
            }
            (7, 1) if source_in_memory => {
                let hi = self.fetch_ext(bus)?;
                let lo = self.take_ext();
                let addr = u32::from(hi) << 16 | u32::from(lo);
                self.set_logic(size, value);
                let pc = self.pc.wrapping_sub(4);
                self.write_mem(bus, size, addr, value)
                    .map_err(|e| super::AddressError { pc: Some(pc), ..e })?;
                self.irc = self.read_word(bus, self.pc, true)?;
                self.prefetch(bus)
            }
            _ => {
                let dst =
                    self.ea_with(bus, dst_mode, dst_reg, size, false, Access::MoveDestination)?;
                self.set_logic(size, value);
                self.prefetch_then_write(bus, dst, size, value)
            }
        }
    }

    fn op_moveq<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        if op & 0x0100 != 0 {
            return self.illegal(bus, ILLEGAL);
        }
        let value = op as u8 as i8 as i32 as u32;
        self.d[reg_hi(op)] = value;
        self.set_logic(Size::Long, value);
        self.prefetch(bus)
    }

    // ---- Grupo 4: diversos ------------------------------------------------

    fn group_4<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        match op {
            0x4AFC => return self.illegal(bus, ILLEGAL),
            0x4E70 => {
                if let Some(r) = self.privileged(bus) {
                    return r;
                }
                bus.reset_devices();
                self.idle(128);
                return self.prefetch(bus);
            }
            0x4E71 => return self.prefetch(bus),
            0x4E72 => {
                if let Some(r) = self.privileged(bus) {
                    return r;
                }
                let imm = self.fetch_ext(bus)?;
                self.set_sr(imm);
                self.stop();
                return self.prefetch(bus);
            }
            0x4E73 => {
                if let Some(r) = self.privileged(bus) {
                    return r;
                }
                let new_sr = self.pop_word(bus)?;
                let pc = self.pop_long(bus)?;
                self.set_sr(new_sr);
                return self.jump(bus, pc);
            }
            0x4E75 => {
                let pc = self.pop_long(bus)?;
                return self.jump(bus, pc);
            }
            0x4E76 => {
                if self.flag(V) {
                    let pc = self.pc;
                    return self.raise(bus, TRAPV, pc, 6);
                }
                return self.prefetch(bus);
            }
            0x4E77 => {
                let ccr = self.pop_word(bus)?;
                let pc = self.pop_long(bus)?;
                self.set_ccr(ccr);
                return self.jump(bus, pc);
            }
            _ => {}
        }
        match op & 0xFFF0 {
            0x4E40 => {
                let pc = self.pc;
                return self.raise(bus, TRAP_BASE + u32::from(op & 0xF), pc, 6);
            }
            0x4E50 => {
                let r = usize::from(op & 7);
                if op & 8 == 0 {
                    // LINK An,#d16
                    let disp = self.fetch_ext(bus)? as i16;
                    let value = if r == 7 {
                        self.a[7].wrapping_sub(4)
                    } else {
                        self.a[r]
                    };
                    self.push_long(bus, value)?;
                    self.a[r] = self.a[7];
                    self.a[7] = self.a[7].wrapping_add_signed(i32::from(disp));
                } else {
                    // UNLK An
                    self.a[7] = self.a[r];
                    let value = self.pop_long(bus)?;
                    self.a[r] = value;
                }
                return self.prefetch(bus);
            }
            0x4E60 => {
                if let Some(r) = self.privileged(bus) {
                    return r;
                }
                let r = usize::from(op & 7);
                if op & 8 == 0 {
                    self.inactive_sp = self.a[r];
                } else {
                    self.a[r] = self.inactive_sp;
                }
                return self.prefetch(bus);
            }
            _ => {}
        }
        match op & 0xFFC0 {
            0x4E80 => return self.op_jsr_jmp(bus, op, true),
            0x4EC0 => return self.op_jsr_jmp(bus, op, false),
            0x40C0 => return self.op_move_from_sr(bus, op),
            0x44C0 => return self.op_move_to_sr(bus, op, false),
            0x46C0 => return self.op_move_to_sr(bus, op, true),
            0x4800 => return self.op_nbcd(bus, op),
            0x4840 => {
                if mode == 0 {
                    let r = usize::from(reg);
                    let v = self.d[r].rotate_left(16);
                    self.d[r] = v;
                    self.set_logic(Size::Long, v);
                    return self.prefetch(bus);
                }
                return self.op_pea(bus, op);
            }
            0x4880 | 0x48C0 if mode == 0 => {
                let r = usize::from(reg);
                let v = if op & 0x40 == 0 {
                    let v = sign_extend(Size::Byte, self.d[r]) & 0xFFFF;
                    self.d[r] = (self.d[r] & 0xFFFF_0000) | v;
                    self.set_logic(Size::Word, v);
                    return self.prefetch(bus);
                } else {
                    sign_extend(Size::Word, self.d[r])
                };
                self.d[r] = v;
                self.set_logic(Size::Long, v);
                return self.prefetch(bus);
            }
            0x4880 | 0x48C0 => return self.op_movem(bus, op, false),
            0x4C80 | 0x4CC0 => return self.op_movem(bus, op, true),
            0x4AC0 => return self.op_tas(bus, op),
            _ => {}
        }
        if op & 0xF1C0 == 0x41C0 {
            return self.op_lea(bus, op);
        }
        if op & 0xF1C0 == 0x4180 {
            return self.op_chk(bus, op);
        }
        // NEGX, CLR, NEG, NOT, TST.
        let Some(size) = Size::from_bits(op >> 6) else {
            return self.illegal(bus, ILLEGAL);
        };
        match op & 0xFF00 {
            0x4000 | 0x4200 | 0x4400 | 0x4600 => self.op_unary(bus, op, size),
            0x4A00 => {
                if !valid(mode, reg, Class::DataAlterable) {
                    return self.illegal(bus, ILLEGAL);
                }
                let mut loc = self.ea(bus, mode, reg, size, true)?;
                let v = self.read_loc(bus, &mut loc, size)?;
                self.set_logic(size, v);
                self.prefetch(bus)
            }
            _ => self.illegal(bus, ILLEGAL),
        }
    }

    fn op_unary<B: Bus>(&mut self, bus: &mut B, op: u16, size: Size) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::DataAlterable) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, size, true)?;
        // O 68000 lê o operando até no CLR.
        let v = self.read_loc(bus, &mut loc, size)?;
        let result = match op & 0xFF00 {
            0x4000 => {
                let f = arith::sub(size, v, 0, self.flag(X));
                self.set_arith_extended(size, f);
                f.result
            }
            0x4200 => {
                self.set_logic(size, 0);
                0
            }
            0x4400 => {
                let f = arith::sub(size, v, 0, false);
                self.set_arith(size, f, true);
                f.result
            }
            _ => {
                let r = !v & size.mask();
                self.set_logic(size, r);
                r
            }
        };
        if mode == 0 && size == Size::Long {
            self.idle(2);
        }
        self.prefetch_then_write(bus, loc, size, result)
    }

    fn op_move_from_sr<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::DataAlterable) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, Size::Word, true)?;
        if mode == 0 {
            self.idle(2);
        } else {
            self.read_loc(bus, &mut loc, Size::Word)?;
        }
        let value = u32::from(self.sr);
        self.prefetch_then_write(bus, loc, Size::Word, value)
    }

    fn op_move_to_sr<B: Bus>(&mut self, bus: &mut B, op: u16, to_sr: bool) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::Data) {
            return self.illegal(bus, ILLEGAL);
        }
        if to_sr && let Some(r) = self.privileged(bus) {
            return r;
        }
        let mut loc = self.ea(bus, mode, reg, Size::Word, true)?;
        let value = self.read_loc(bus, &mut loc, Size::Word)? as u16;
        if to_sr {
            self.set_sr(value);
        } else {
            self.set_ccr(value);
        }
        self.idle(4);
        let pc = self.pc;
        self.jump(bus, pc)
    }

    fn op_nbcd<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::DataAlterable) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, Size::Byte, true)?;
        let v = self.read_loc(bus, &mut loc, Size::Byte)? as u8;
        let r = arith::sbcd(v, 0, self.flag(X));
        self.set_bcd(r);
        if mode == 0 {
            self.idle(2);
        }
        self.prefetch_then_write(bus, loc, Size::Byte, u32::from(r.result))
    }

    fn set_bcd(&mut self, r: arith::Bcd) {
        self.set_flag(X, r.carry);
        self.set_flag(C, r.carry);
        self.set_flag(N, r.result & 0x80 != 0);
        self.set_flag(V, r.overflow);
        if r.result != 0 {
            self.set_flag(Z, false);
        }
    }

    fn op_pea<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::Control) {
            return self.illegal(bus, ILLEGAL);
        }
        let addr = self.control_addr(bus, mode, reg, true)?;
        if mode == 6 || (mode == 7 && reg == 3) {
            self.idle(4);
        }
        self.push_long(bus, addr)?;
        self.prefetch(bus)
    }

    fn op_lea<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::Control) {
            return self.illegal(bus, ILLEGAL);
        }
        let addr = self.control_addr(bus, mode, reg, true)?;
        if mode == 6 || (mode == 7 && reg == 3) {
            self.idle(4);
        }
        self.a[reg_hi(op)] = addr;
        self.prefetch(bus)
    }

    fn op_jsr_jmp<B: Bus>(&mut self, bus: &mut B, op: u16, jsr: bool) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::Control) {
            return self.illegal(bus, ILLEGAL);
        }
        let target = self.control_addr(bus, mode, reg, false)?;
        self.idle(match (mode, reg) {
            (2, _) | (7, 1) => 0,
            (6, _) | (7, 3) => 6,
            _ => 2,
        });
        if !jsr {
            return self.jump(bus, target);
        }
        // JSR busca o destino antes de empilhar: com destino ímpar, a pilha
        // fica intacta.
        let ret = self.pc;
        let ir = self
            .read_word(bus, target, true)
            .map_err(|e| super::AddressError {
                pc: Some(target.wrapping_sub(4)),
                ..e
            })?;
        self.push_long(bus, ret)?;
        self.irc = self.read_word(bus, target.wrapping_add(2), true)?;
        self.ir = ir;
        self.pc = target.wrapping_add(2);
        Ok(())
    }

    fn op_tas<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::DataAlterable) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, Size::Byte, true)?;
        let v = self.read_loc(bus, &mut loc, Size::Byte)?;
        self.set_logic(Size::Byte, v);
        if mode != 0 {
            self.idle(2);
        }
        self.prefetch_then_write(bus, loc, Size::Byte, v | 0x80)
    }

    fn op_chk<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::Data) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, Size::Word, true)?;
        let bound = self.read_loc(bus, &mut loc, Size::Word)? as u16 as i16;
        let value = self.d[reg_hi(op)] as u16 as i16;
        self.idle(6);
        // Flags não documentados, como medidos: Z, V e C zerados; N só
        // muda quando há exceção. O limite superior é testado primeiro.
        self.set_flag(Z, false);
        self.set_flag(V, false);
        self.set_flag(C, false);
        if value > bound || value < 0 {
            self.set_flag(N, value < 0);
            let pc = self.pc;
            let idle = if value > bound { 4 } else { 6 };
            return self.raise(bus, CHK, pc, idle);
        }
        self.prefetch(bus)
    }

    fn op_movem<B: Bus>(&mut self, bus: &mut B, op: u16, to_registers: bool) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        let ok = if to_registers {
            mode == 3 || valid(mode, reg, Class::Control)
        } else {
            mode == 4 || valid(mode, reg, Class::ControlAlterable)
        };
        if !ok {
            return self.illegal(bus, ILLEGAL);
        }
        let size = if op & 0x40 != 0 {
            Size::Long
        } else {
            Size::Word
        };
        let mask = self.fetch_ext(bus)?;
        let r = usize::from(reg);

        if !to_registers && mode == 4 {
            // -(An): máscara invertida (bit 0 = A7), grava de trás para frente.
            let initial = self.a[r];
            let mut addr = initial;
            for i in 0..16 {
                if mask & (1 << i) != 0 {
                    let index = 15 - i;
                    let value = if index >= 8 {
                        // O 68000 grava o valor original de An.
                        if index - 8 == r {
                            initial
                        } else {
                            self.a[index - 8]
                        }
                    } else {
                        self.d[index]
                    };
                    addr = addr.wrapping_sub(size.bytes());
                    if size == Size::Long {
                        // Palavra baixa primeiro, como no hardware.
                        self.write_word(bus, addr.wrapping_add(2), value as u16)?;
                        self.write_word(bus, addr, (value >> 16) as u16)?;
                    } else {
                        self.write_mem(bus, size, addr, value)?;
                    }
                }
            }
            self.a[r] = addr;
            return self.prefetch(bus);
        }

        let mut addr = if mode == 3 {
            self.a[r]
        } else {
            let a = self.control_addr(bus, mode, reg, true)?;
            if mode == 6 || (mode == 7 && reg == 3) {
                self.idle(2);
            }
            a
        };
        for i in 0..16 {
            if mask & (1 << i) == 0 {
                continue;
            }
            if to_registers {
                let read = self.read_mem(bus, size, addr, false);
                if read.is_err() && mode == 3 {
                    // Medido: num erro de endereço em (An)+, An avança 2.
                    self.a[r] = addr.wrapping_add(2);
                }
                let value = sign_extend(size, read?);
                if i < 8 {
                    self.d[i] = value;
                } else {
                    self.a[i - 8] = value;
                }
            } else {
                let value = if i < 8 { self.d[i] } else { self.a[i - 8] };
                self.write_mem(bus, size, addr, value)?;
            }
            addr = addr.wrapping_add(size.bytes());
        }
        if to_registers {
            // Leitura extra no fim, característica do 68000.
            self.read_word(bus, addr, false)?;
            if mode == 3 {
                self.a[r] = addr;
            }
        }
        self.prefetch(bus)
    }

    // ---- Grupo 5: ADDQ/SUBQ, Scc, DBcc ------------------------------------

    fn group_5<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        let Some(size) = Size::from_bits(op >> 6) else {
            return self.op_scc_dbcc(bus, op);
        };
        if !valid(mode, reg, Class::Alterable) || (mode == 1 && size == Size::Byte) {
            return self.illegal(bus, ILLEGAL);
        }
        let data = match (op >> 9) & 7 {
            0 => 8,
            n => u32::from(n),
        };
        let subtract = op & 0x0100 != 0;
        if mode == 1 {
            let r = usize::from(reg);
            self.a[r] = if subtract {
                self.a[r].wrapping_sub(data)
            } else {
                self.a[r].wrapping_add(data)
            };
            self.idle(if size == Size::Long { 2 } else { 4 });
            return self.prefetch(bus);
        }
        let mut loc = self.ea(bus, mode, reg, size, true)?;
        let dst = self.read_loc(bus, &mut loc, size)?;
        let f = if subtract {
            arith::sub(size, data, dst, false)
        } else {
            arith::add(size, data, dst, false)
        };
        self.set_arith(size, f, true);
        if mode == 0 && size == Size::Long {
            self.idle(4);
        }
        self.prefetch_then_write(bus, loc, size, f.result)
    }

    fn op_scc_dbcc<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        let cc = (op >> 8) & 0xF;
        if mode == 1 {
            let r = usize::from(reg);
            if self.condition(cc) {
                self.idle(4);
                self.fetch_ext(bus)?;
                return self.prefetch(bus);
            }
            let counter = (self.d[r] as u16).wrapping_sub(1);
            self.d[r] = (self.d[r] & 0xFFFF_0000) | u32::from(counter);
            let base = self.pc;
            let disp = self.irc as i16;
            let target = base.wrapping_add_signed(i32::from(disp));
            self.idle(2);
            if counter != 0xFFFF {
                self.take_ext();
                return self.jump(bus, target);
            }
            // Contador esgotado: o 68000 ainda lê a palavra no destino.
            self.read_word(bus, target, true)?;
            self.fetch_ext(bus)?;
            return self.prefetch(bus);
        }
        if !valid(mode, reg, Class::DataAlterable) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, Size::Byte, true)?;
        let value = if self.condition(cc) { 0xFF } else { 0 };
        if mode == 0 {
            if value != 0 {
                self.idle(2);
            }
        } else {
            self.read_loc(bus, &mut loc, Size::Byte)?;
        }
        self.prefetch_then_write(bus, loc, Size::Byte, value)
    }

    // ---- Grupo 6: Bcc, BRA, BSR -------------------------------------------

    fn op_branch<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let cc = (op >> 8) & 0xF;
        let base = self.pc;
        let short = op as u8;
        let disp = if short == 0 {
            i32::from(self.irc as i16)
        } else {
            i32::from(short as i8)
        };
        let target = base.wrapping_add_signed(disp);
        if cc == 1 {
            // BSR: endereço de retorno logo após a instrução.
            let ret = if short == 0 {
                base.wrapping_add(2)
            } else {
                base
            };
            self.idle(2);
            self.push_long(bus, ret)?;
            return self.jump(bus, target);
        }
        if self.condition(cc) {
            self.idle(2);
            return self.jump(bus, target);
        }
        self.idle(4);
        if short == 0 {
            self.fetch_ext(bus)?;
        }
        self.prefetch(bus)
    }

    // ---- Grupo 8: OR, DIVU/DIVS, SBCD -------------------------------------

    fn group_8<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        match op & 0x01C0 {
            0x00C0 => return self.op_div(bus, op, false),
            0x01C0 => return self.op_div(bus, op, true),
            _ => {}
        }
        if op & 0x01F0 == 0x0100 {
            return self.op_bcd(bus, op, false);
        }
        self.op_logic(bus, op, |a, b| a | b)
    }

    fn op_div<B: Bus>(&mut self, bus: &mut B, op: u16, signed: bool) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::Data) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, Size::Word, true)?;
        let divisor = self.read_loc(bus, &mut loc, Size::Word)? as u16;
        let r = reg_hi(op);
        let dividend = self.d[r];
        if divisor == 0 {
            self.set_flag(N, false);
            self.set_flag(Z, false);
            self.set_flag(V, false);
            self.set_flag(C, false);
            let pc = self.pc;
            return self.raise(bus, ZERO_DIVIDE, pc, 8);
        }
        self.set_flag(C, false);
        // Overflow: V ligado, N e Z inalterados, registrador intacto.
        if signed {
            let quotient = i64::from(dividend as i32) / i64::from(divisor as i16);
            let remainder = i64::from(dividend as i32) % i64::from(divisor as i16);
            if quotient < i64::from(i16::MIN) || quotient > i64::from(i16::MAX) {
                // Medido: todo overflow de DIVS sai pelo caminho curto.
                self.idle(if (dividend as i32) < 0 { 14 } else { 12 });
                self.set_flag(V, true);
            } else {
                self.idle(arith::divs_cycles(dividend as i32, divisor as i16) - 4);
                let q = quotient as u32 & 0xFFFF;
                self.d[r] = (remainder as u32) << 16 | q;
                self.set_flag(V, false);
                self.set_nz(Size::Word, q);
            }
        } else {
            self.idle(arith::divu_cycles(dividend, divisor) - 4);
            let quotient = dividend / u32::from(divisor);
            let remainder = dividend % u32::from(divisor);
            if quotient > 0xFFFF {
                self.set_flag(V, true);
            } else {
                self.d[r] = remainder << 16 | quotient;
                self.set_flag(V, false);
                self.set_nz(Size::Word, quotient);
            }
        }
        self.prefetch(bus)
    }

    /// ABCD/SBCD Dy,Dx ou -(Ay),-(Ax).
    fn op_bcd<B: Bus>(&mut self, bus: &mut B, op: u16, add: bool) -> Step<()> {
        let rx = reg_hi(op);
        let ry = usize::from(op & 7);
        let memory = op & 8 != 0;
        let (src, dst, dst_loc) = if memory {
            let mut s = self.ea(bus, 4, ry as u16, Size::Byte, true)?;
            let src = self.read_loc(bus, &mut s, Size::Byte)?;
            let mut d = self.ea(bus, 4, rx as u16, Size::Byte, false)?;
            let dst = self.read_loc(bus, &mut d, Size::Byte)?;
            (src, dst, d)
        } else {
            self.idle(2);
            (self.d[ry] & 0xFF, self.d[rx] & 0xFF, Loc::D(rx))
        };
        let x = self.flag(X);
        let r = if add {
            arith::abcd(src as u8, dst as u8, x)
        } else {
            arith::sbcd(src as u8, dst as u8, x)
        };
        self.set_bcd(r);
        self.prefetch_then_write(bus, dst_loc, Size::Byte, u32::from(r.result))
    }

    /// AND/OR: <ea>,Dn ou Dn,<ea>.
    fn op_logic<B: Bus>(&mut self, bus: &mut B, op: u16, f: fn(u32, u32) -> u32) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        let Some(size) = Size::from_bits(op >> 6) else {
            return self.illegal(bus, ILLEGAL);
        };
        let r = reg_hi(op);
        if op & 0x0100 == 0 {
            if !valid(mode, reg, Class::Data) {
                return self.illegal(bus, ILLEGAL);
            }
            let mut loc = self.ea(bus, mode, reg, size, true)?;
            let src = self.read_loc(bus, &mut loc, size)?;
            let result = f(src, self.d[r]) & size.mask();
            self.set_logic(size, result);
            if size == Size::Long {
                self.idle(if mode == 0 || (mode == 7 && reg == 4) {
                    4
                } else {
                    2
                });
            }
            self.write_loc(bus, &mut Loc::D(r), size, result)?;
            return self.prefetch(bus);
        }
        if !valid(mode, reg, Class::MemoryAlterable) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, size, true)?;
        let dst = self.read_loc(bus, &mut loc, size)?;
        let result = f(self.d[r], dst) & size.mask();
        self.set_logic(size, result);
        self.prefetch_then_write(bus, loc, size, result)
    }

    // ---- Grupos 9/D: SUB/ADD, SUBA/ADDA, SUBX/ADDX ------------------------

    fn group_add_sub<B: Bus>(&mut self, bus: &mut B, op: u16, add: bool) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        let r = reg_hi(op);
        let opmode = (op >> 6) & 7;
        if opmode == 3 || opmode == 7 {
            // ADDA/SUBA
            let size = if opmode == 3 { Size::Word } else { Size::Long };
            if !valid(mode, reg, Class::All) {
                return self.illegal(bus, ILLEGAL);
            }
            let mut loc = self.ea(bus, mode, reg, size, true)?;
            let src = sign_extend(size, self.read_loc(bus, &mut loc, size)?);
            self.a[r] = if add {
                self.a[r].wrapping_add(src)
            } else {
                self.a[r].wrapping_sub(src)
            };
            self.idle(
                if size == Size::Word || mode <= 1 || (mode == 7 && reg == 4) {
                    4
                } else {
                    2
                },
            );
            return self.prefetch(bus);
        }
        let Some(size) = Size::from_bits(opmode) else {
            return self.illegal(bus, ILLEGAL);
        };
        if opmode >= 4 && mode <= 1 {
            // ADDX/SUBX
            let ry = usize::from(reg);
            let (src, dst, dst_loc) = if mode == 1 {
                let mut s = self.ea_with(bus, 4, reg, size, true, Access::LowWordFirst)?;
                let src = self.read_loc(bus, &mut s, size)?;
                let mut d = self.ea_with(bus, 4, r as u16, size, false, Access::LowWordFirst)?;
                let dst = self.read_loc(bus, &mut d, size)?;
                (src, dst, d)
            } else {
                if size == Size::Long {
                    self.idle(4);
                }
                (self.d[ry], self.d[r], Loc::D(r))
            };
            let x = self.flag(X);
            let f = if add {
                arith::add(size, src, dst, x)
            } else {
                arith::sub(size, src, dst, x)
            };
            self.set_arith_extended(size, f);
            return self.prefetch_then_write(bus, dst_loc, size, f.result);
        }
        if opmode < 4 {
            // <ea>,Dn
            if !valid(mode, reg, Class::All) || (mode == 1 && size == Size::Byte) {
                return self.illegal(bus, ILLEGAL);
            }
            let mut loc = self.ea(bus, mode, reg, size, true)?;
            let src = self.read_loc(bus, &mut loc, size)?;
            let f = if add {
                arith::add(size, src, self.d[r], false)
            } else {
                arith::sub(size, src, self.d[r], false)
            };
            self.set_arith(size, f, true);
            if size == Size::Long {
                self.idle(if mode <= 1 || (mode == 7 && reg == 4) {
                    4
                } else {
                    2
                });
            }
            self.write_loc(bus, &mut Loc::D(r), size, f.result)?;
            return self.prefetch(bus);
        }
        // Dn,<ea>
        if !valid(mode, reg, Class::MemoryAlterable) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, size, true)?;
        let dst = self.read_loc(bus, &mut loc, size)?;
        let f = if add {
            arith::add(size, self.d[r], dst, false)
        } else {
            arith::sub(size, self.d[r], dst, false)
        };
        self.set_arith(size, f, true);
        self.prefetch_then_write(bus, loc, size, f.result)
    }

    // ---- Grupo B: CMP, CMPA, CMPM, EOR ------------------------------------

    fn group_b<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        let r = reg_hi(op);
        let opmode = (op >> 6) & 7;
        if opmode == 3 || opmode == 7 {
            let size = if opmode == 3 { Size::Word } else { Size::Long };
            if !valid(mode, reg, Class::All) {
                return self.illegal(bus, ILLEGAL);
            }
            let mut loc = self.ea(bus, mode, reg, size, true)?;
            let src = sign_extend(size, self.read_loc(bus, &mut loc, size)?);
            let f = arith::sub(Size::Long, src, self.a[r], false);
            self.set_arith(Size::Long, f, false);
            self.idle(2);
            return self.prefetch(bus);
        }
        let Some(size) = Size::from_bits(opmode) else {
            return self.illegal(bus, ILLEGAL);
        };
        if opmode < 4 {
            if !valid(mode, reg, Class::All) || (mode == 1 && size == Size::Byte) {
                return self.illegal(bus, ILLEGAL);
            }
            let mut loc = self.ea(bus, mode, reg, size, true)?;
            let src = self.read_loc(bus, &mut loc, size)?;
            let f = arith::sub(size, src, self.d[r], false);
            self.set_arith(size, f, false);
            if size == Size::Long {
                self.idle(2);
            }
            return self.prefetch(bus);
        }
        if mode == 1 {
            // CMPM (Ay)+,(Ax)+
            let mut s = self.ea(bus, 3, reg, size, true)?;
            let src = self.read_loc(bus, &mut s, size)?;
            let mut d = self.ea(bus, 3, r as u16, size, true)?;
            let dst = self.read_loc(bus, &mut d, size)?;
            let f = arith::sub(size, src, dst, false);
            self.set_arith(size, f, false);
            return self.prefetch(bus);
        }
        // EOR Dn,<ea>
        if !valid(mode, reg, Class::DataAlterable) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, size, true)?;
        let dst = self.read_loc(bus, &mut loc, size)?;
        let result = (self.d[r] ^ dst) & size.mask();
        self.set_logic(size, result);
        if mode == 0 && size == Size::Long {
            self.idle(4);
        }
        self.prefetch_then_write(bus, loc, size, result)
    }

    // ---- Grupo C: AND, MULU/MULS, ABCD, EXG -------------------------------

    fn group_c<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        match op & 0x01C0 {
            0x00C0 => return self.op_mul(bus, op, false),
            0x01C0 => return self.op_mul(bus, op, true),
            _ => {}
        }
        if op & 0x01F0 == 0x0100 {
            return self.op_bcd(bus, op, true);
        }
        let rx = reg_hi(op);
        let ry = usize::from(op & 7);
        match op & 0x01F8 {
            0x0140 => {
                self.d.swap(rx, ry);
                self.idle(2);
                return self.prefetch(bus);
            }
            0x0148 => {
                self.a.swap(rx, ry);
                self.idle(2);
                return self.prefetch(bus);
            }
            0x0188 => {
                std::mem::swap(&mut self.d[rx], &mut self.a[ry]);
                self.idle(2);
                return self.prefetch(bus);
            }
            _ => {}
        }
        self.op_logic(bus, op, |a, b| a & b)
    }

    fn op_mul<B: Bus>(&mut self, bus: &mut B, op: u16, signed: bool) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        if !valid(mode, reg, Class::Data) {
            return self.illegal(bus, ILLEGAL);
        }
        let mut loc = self.ea(bus, mode, reg, Size::Word, true)?;
        let src = self.read_loc(bus, &mut loc, Size::Word)? as u16;
        let r = reg_hi(op);
        let result = if signed {
            self.idle(arith::muls_cycles(src) - 4);
            (i32::from(src as i16) * i32::from(self.d[r] as u16 as i16)) as u32
        } else {
            self.idle(arith::mulu_cycles(src) - 4);
            u32::from(src) * (self.d[r] & 0xFFFF)
        };
        self.d[r] = result;
        self.set_logic(Size::Long, result);
        self.prefetch(bus)
    }

    // ---- Grupo E: deslocamentos e rotações --------------------------------

    fn group_e<B: Bus>(&mut self, bus: &mut B, op: u16) -> Step<()> {
        let (mode, reg) = mode_reg(op);
        let left = op & 0x0100 != 0;
        let Some(size) = Size::from_bits(op >> 6) else {
            // Em memória: palavra, deslocamento de 1.
            if op & 0x0800 != 0 || !valid(mode, reg, Class::MemoryAlterable) {
                return self.illegal(bus, ILLEGAL);
            }
            let kind = (op >> 9) & 3;
            let mut loc = self.ea(bus, mode, reg, Size::Word, true)?;
            let v = self.read_loc(bus, &mut loc, Size::Word)?;
            let r = self.shift(kind, left, Size::Word, v, 1);
            return self.prefetch_then_write(bus, loc, Size::Word, r);
        };
        let count = if op & 0x20 != 0 {
            self.d[reg_hi(op)] % 64
        } else {
            match (op >> 9) & 7 {
                0 => 8,
                n => u32::from(n),
            }
        };
        let kind = (op >> 3) & 3;
        let r = usize::from(reg);
        let result = self.shift(kind, left, size, self.d[r], count);
        self.idle(if size == Size::Long { 4 } else { 2 } + 2 * count);
        self.write_loc(bus, &mut Loc::D(r), size, result)?;
        self.prefetch(bus)
    }

    /// Desloca/rotaciona `count` vezes e ajusta os flags.
    /// `kind`: 0 = AS, 1 = LS, 2 = ROX, 3 = RO.
    fn shift(&mut self, kind: u16, left: bool, size: Size, value: u32, count: u32) -> u32 {
        let mask = size.mask();
        let msb = size.msb();
        let mut v = value & mask;
        let mut carry = false;
        let mut overflow = false;
        let mut x = self.flag(X);
        for _ in 0..count {
            let out = if left { v & msb != 0 } else { v & 1 != 0 };
            v = match (kind, left) {
                (0, true) | (1, true) => (v << 1) & mask,
                (0, false) => (v >> 1) | (v & msb),
                (1, false) => v >> 1,
                (2, true) => ((v << 1) | u32::from(x)) & mask,
                (2, false) => (v >> 1) | if x { msb } else { 0 },
                (_, true) => ((v << 1) | u32::from(out)) & mask,
                (_, false) => (v >> 1) | if out { msb } else { 0 },
            };
            if kind == 0 && left && (v & msb != 0) != out {
                overflow = true;
            }
            carry = out;
            if kind != 3 {
                x = out;
            }
        }
        let width = size.bytes() * 8;
        if kind == 0 && !left && count > width {
            // Medido: além da largura, o ASR deixa C e X zerados.
            carry = false;
            x = false;
        }
        self.set_nz(size, v);
        self.set_flag(V, overflow);
        match kind {
            // ROX com contagem 0: C recebe X.
            2 => {
                self.set_flag(C, if count == 0 { self.flag(X) } else { carry });
                self.set_flag(X, x);
            }
            3 => self.set_flag(C, count != 0 && carry),
            _ => {
                self.set_flag(C, count != 0 && carry);
                if count != 0 {
                    self.set_flag(X, x);
                }
            }
        }
        v
    }
}
