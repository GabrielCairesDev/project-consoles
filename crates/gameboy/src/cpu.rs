//! CPU SM83 (o "LR35902" do Game Boy).
//!
//! Cada leitura e escrita passa pelo [`Bus`], que conta um M-ciclo; os
//! ciclos internos das instruções são `bus.idle()`. Começa no estado em que
//! a boot ROM do DMG deixa os registradores.

use chips::state::{Reader, StateError, Writer};

use crate::bus::Bus;

const Z: u8 = 0x80;
const N: u8 = 0x40;
const H: u8 = 0x20;
const C: u8 = 0x10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpu {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    ime: bool,
    /// EI liga o IME só depois da instrução seguinte.
    ime_pending: bool,
    halted: bool,
    /// HALT com IME desligado e interrupção pendente: o próximo byte é lido
    /// duas vezes (o PC não avança na busca seguinte).
    halt_bug: bool,
    /// Opcode inexistente: a CPU trava até ser desligada.
    locked: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            a: 0x01,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
            ime: false,
            ime_pending: false,
            halted: false,
            halt_bug: false,
            locked: false,
        }
    }
}

impl Cpu {
    fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }

    fn set_hl(&mut self, v: u16) {
        [self.h, self.l] = v.to_be_bytes();
    }

    fn flag(&self, mask: u8) -> bool {
        self.f & mask != 0
    }

    fn set_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.f = (u8::from(z) << 7) | (u8::from(n) << 6) | (u8::from(h) << 5) | (u8::from(c) << 4);
    }

    /// Par de registradores: 0 BC, 1 DE, 2 HL, 3 SP.
    fn r16(&self, index: u8) -> u16 {
        match index {
            0 => u16::from_be_bytes([self.b, self.c]),
            1 => u16::from_be_bytes([self.d, self.e]),
            2 => self.hl(),
            _ => self.sp,
        }
    }

    fn set_r16(&mut self, index: u8, v: u16) {
        let [hi, lo] = v.to_be_bytes();
        match index {
            0 => (self.b, self.c) = (hi, lo),
            1 => (self.d, self.e) = (hi, lo),
            2 => (self.h, self.l) = (hi, lo),
            _ => self.sp = v,
        }
    }

    /// Registrador de 8 bits: 0 B, 1 C, 2 D, 3 E, 4 H, 5 L, 6 (HL), 7 A.
    fn r8(&mut self, bus: &mut Bus, index: u8) -> u8 {
        match index {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read(self.hl()),
            _ => self.a,
        }
    }

    fn set_r8(&mut self, bus: &mut Bus, index: u8, v: u8) {
        match index {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => self.h = v,
            5 => self.l = v,
            6 => bus.write(self.hl(), v),
            _ => self.a = v,
        }
    }

    fn fetch(&mut self, bus: &mut Bus) -> u8 {
        let v = bus.read(self.pc);
        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
        v
    }

    fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_be_bytes([hi, lo])
    }

    fn push(&mut self, bus: &mut Bus, v: u16) {
        let [hi, lo] = v.to_be_bytes();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, hi);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, lo);
    }

    fn pop(&mut self, bus: &mut Bus) -> u16 {
        let lo = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        u16::from_be_bytes([hi, lo])
    }

    /// Condição: 0 NZ, 1 Z, 2 NC, 3 C.
    fn condition(&self, index: u8) -> bool {
        match index {
            0 => !self.flag(Z),
            1 => self.flag(Z),
            2 => !self.flag(C),
            _ => self.flag(C),
        }
    }

    /// Executa uma instrução (ou atende uma interrupção, ou espera no HALT).
    pub fn step(&mut self, bus: &mut Bus) {
        if self.locked {
            bus.idle();
            return;
        }
        if self.halted {
            if bus.pending() == 0 {
                bus.idle();
                return;
            }
            self.halted = false;
        }
        if self.ime && bus.pending() != 0 {
            self.dispatch(bus);
            return;
        }
        if self.ime_pending {
            self.ime_pending = false;
            self.ime = true;
        }
        let op = self.fetch(bus);
        self.execute(bus, op);
    }

    /// Atende a interrupção pendente de maior prioridade (5 M-ciclos: duas
    /// esperas, o PC na pilha e o salto).
    fn dispatch(&mut self, bus: &mut Bus) {
        self.ime = false;
        bus.idle();
        bus.idle();
        let [hi, lo] = self.pc.to_be_bytes();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, hi);
        // A escolha acontece depois do byte alto: se ele sobrescreveu o IE
        // e cancelou a interrupção, o salto vai para 0x0000.
        let pending = bus.pending();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, lo);
        if pending == 0 {
            self.pc = 0;
        } else {
            let bit = pending.trailing_zeros() as u16;
            bus.interrupt_flag &= !(1 << bit);
            self.pc = 0x40 + bit * 8;
        }
        bus.idle();
    }

    fn alu(&mut self, op: u8, v: u8) {
        let a = self.a;
        let carry = u8::from(self.flag(C));
        match op {
            0 | 1 => {
                let c = if op == 1 { carry } else { 0 };
                let r = u16::from(a) + u16::from(v) + u16::from(c);
                self.a = r as u8;
                self.set_flags(
                    self.a == 0,
                    false,
                    (a & 0xF) + (v & 0xF) + c > 0xF,
                    r > 0xFF,
                );
            }
            2 | 3 | 7 => {
                let c = if op == 3 { carry } else { 0 };
                let r = i16::from(a) - i16::from(v) - i16::from(c);
                let result = r as u8;
                self.set_flags(result == 0, true, (a & 0xF) < (v & 0xF) + c, r < 0);
                if op != 7 {
                    self.a = result;
                }
            }
            4 => {
                self.a &= v;
                self.set_flags(self.a == 0, false, true, false);
            }
            5 => {
                self.a ^= v;
                self.set_flags(self.a == 0, false, false, false);
            }
            _ => {
                self.a |= v;
                self.set_flags(self.a == 0, false, false, false);
            }
        }
    }

    fn inc8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_add(1);
        self.set_flags(r == 0, false, v & 0xF == 0xF, self.flag(C));
        r
    }

    fn dec8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_sub(1);
        self.set_flags(r == 0, true, v & 0xF == 0, self.flag(C));
        r
    }

    /// SP + deslocamento com sinal; flags H e C do byte baixo (ADD SP / LD HL,SP+e).
    fn sp_offset(&mut self, offset: u8) -> u16 {
        let sp = self.sp;
        let e = u16::from(offset);
        self.set_flags(
            false,
            false,
            (sp & 0xF) + (e & 0xF) > 0xF,
            (sp & 0xFF) + e > 0xFF,
        );
        sp.wrapping_add(i16::from(offset as i8) as u16)
    }

    fn daa(&mut self) {
        let mut a = self.a;
        let mut carry = self.flag(C);
        if self.flag(N) {
            if carry {
                a = a.wrapping_sub(0x60);
            }
            if self.flag(H) {
                a = a.wrapping_sub(0x06);
            }
        } else {
            if carry || a > 0x99 {
                a = a.wrapping_add(0x60);
                carry = true;
            }
            if self.flag(H) || a & 0x0F > 0x09 {
                a = a.wrapping_add(0x06);
            }
        }
        self.a = a;
        self.set_flags(a == 0, self.flag(N), false, carry);
    }

    fn jump_relative(&mut self, bus: &mut Bus, offset: u8) {
        bus.idle();
        self.pc = self.pc.wrapping_add(i16::from(offset as i8) as u16);
    }

    fn execute(&mut self, bus: &mut Bus, op: u8) {
        match op {
            0x00 => {}
            0x01 | 0x11 | 0x21 | 0x31 => {
                let v = self.fetch16(bus);
                self.set_r16(op >> 4, v);
            }
            0x02 | 0x12 => bus.write(self.r16(op >> 4), self.a),
            0x22 => {
                bus.write(self.hl(), self.a);
                self.set_hl(self.hl().wrapping_add(1));
            }
            0x32 => {
                bus.write(self.hl(), self.a);
                self.set_hl(self.hl().wrapping_sub(1));
            }
            0x0A | 0x1A => self.a = bus.read(self.r16(op >> 4)),
            0x2A => {
                self.a = bus.read(self.hl());
                self.set_hl(self.hl().wrapping_add(1));
            }
            0x3A => {
                self.a = bus.read(self.hl());
                self.set_hl(self.hl().wrapping_sub(1));
            }
            0x03 | 0x13 | 0x23 | 0x33 => {
                bus.idle();
                let r = op >> 4;
                self.set_r16(r, self.r16(r).wrapping_add(1));
            }
            0x0B | 0x1B | 0x2B | 0x3B => {
                bus.idle();
                let r = op >> 4;
                self.set_r16(r, self.r16(r).wrapping_sub(1));
            }
            _ if op & 0xC7 == 0x04 => {
                let r = (op >> 3) & 7;
                let v = self.r8(bus, r);
                let v = self.inc8(v);
                self.set_r8(bus, r, v);
            }
            _ if op & 0xC7 == 0x05 => {
                let r = (op >> 3) & 7;
                let v = self.r8(bus, r);
                let v = self.dec8(v);
                self.set_r8(bus, r, v);
            }
            _ if op & 0xC7 == 0x06 => {
                let v = self.fetch(bus);
                self.set_r8(bus, (op >> 3) & 7, v);
            }
            0x07 => {
                self.a = self.a.rotate_left(1);
                self.set_flags(false, false, false, self.a & 1 != 0);
            }
            0x0F => {
                self.set_flags(false, false, false, self.a & 1 != 0);
                self.a = self.a.rotate_right(1);
            }
            0x17 => {
                let carry = u8::from(self.flag(C));
                self.set_flags(false, false, false, self.a & 0x80 != 0);
                self.a = (self.a << 1) | carry;
            }
            0x1F => {
                let carry = u8::from(self.flag(C));
                self.set_flags(false, false, false, self.a & 1 != 0);
                self.a = (self.a >> 1) | (carry << 7);
            }
            0x08 => {
                let addr = self.fetch16(bus);
                let [hi, lo] = self.sp.to_be_bytes();
                bus.write(addr, lo);
                bus.write(addr.wrapping_add(1), hi);
            }
            0x09 | 0x19 | 0x29 | 0x39 => {
                bus.idle();
                let hl = self.hl();
                let v = self.r16(op >> 4);
                let (r, carry) = hl.overflowing_add(v);
                self.set_flags(
                    self.flag(Z),
                    false,
                    (hl & 0xFFF) + (v & 0xFFF) > 0xFFF,
                    carry,
                );
                self.set_hl(r);
            }
            0x10 => {
                // STOP: no DMG sem troca de velocidade, só zera o DIV e
                // pula o byte seguinte. O modo de baixo consumo não é emulado.
                self.fetch(bus);
                bus.timer.reset_divider();
            }
            0x18 => {
                let e = self.fetch(bus);
                self.jump_relative(bus, e);
            }
            0x20 | 0x28 | 0x30 | 0x38 => {
                let e = self.fetch(bus);
                if self.condition((op >> 3) & 3) {
                    self.jump_relative(bus, e);
                }
            }
            0x27 => self.daa(),
            0x2F => {
                self.a = !self.a;
                self.f |= N | H;
            }
            0x37 => self.f = (self.f & Z) | C,
            0x3F => self.f = (self.f & (Z | C)) ^ C,
            0x76 => {
                if !self.ime && bus.pending() != 0 {
                    self.halt_bug = true;
                } else {
                    self.halted = true;
                }
            }
            0x40..=0x7F => {
                let v = self.r8(bus, op & 7);
                self.set_r8(bus, (op >> 3) & 7, v);
            }
            0x80..=0xBF => {
                let v = self.r8(bus, op & 7);
                self.alu((op >> 3) & 7, v);
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 => {
                bus.idle();
                if self.condition((op >> 3) & 3) {
                    self.pc = self.pop(bus);
                    bus.idle();
                }
            }
            0xC1 | 0xD1 | 0xE1 => {
                let v = self.pop(bus);
                self.set_r16((op >> 4) & 3, v);
            }
            0xF1 => {
                let v = self.pop(bus);
                let [a, f] = v.to_be_bytes();
                self.a = a;
                self.f = f & 0xF0;
            }
            0xC2 | 0xCA | 0xD2 | 0xDA => {
                let addr = self.fetch16(bus);
                if self.condition((op >> 3) & 3) {
                    bus.idle();
                    self.pc = addr;
                }
            }
            0xC3 => {
                let addr = self.fetch16(bus);
                bus.idle();
                self.pc = addr;
            }
            0xC4 | 0xCC | 0xD4 | 0xDC => {
                let addr = self.fetch16(bus);
                if self.condition((op >> 3) & 3) {
                    bus.idle();
                    self.push(bus, self.pc);
                    self.pc = addr;
                }
            }
            0xC5 | 0xD5 | 0xE5 => {
                bus.idle();
                self.push(bus, self.r16((op >> 4) & 3));
            }
            0xF5 => {
                bus.idle();
                self.push(bus, u16::from_be_bytes([self.a, self.f]));
            }
            _ if op & 0xC7 == 0xC6 => {
                let v = self.fetch(bus);
                self.alu((op >> 3) & 7, v);
            }
            _ if op & 0xC7 == 0xC7 => {
                bus.idle();
                self.push(bus, self.pc);
                self.pc = u16::from(op & 0x38);
            }
            0xC9 | 0xD9 => {
                self.pc = self.pop(bus);
                bus.idle();
                if op == 0xD9 {
                    self.ime = true;
                }
            }
            0xCB => {
                let cb = self.fetch(bus);
                self.execute_cb(bus, cb);
            }
            0xCD => {
                let addr = self.fetch16(bus);
                bus.idle();
                self.push(bus, self.pc);
                self.pc = addr;
            }
            0xE0 => {
                let n = self.fetch(bus);
                bus.write(0xFF00 | u16::from(n), self.a);
            }
            0xF0 => {
                let n = self.fetch(bus);
                self.a = bus.read(0xFF00 | u16::from(n));
            }
            0xE2 => bus.write(0xFF00 | u16::from(self.c), self.a),
            0xF2 => self.a = bus.read(0xFF00 | u16::from(self.c)),
            0xE8 => {
                let e = self.fetch(bus);
                self.sp = self.sp_offset(e);
                bus.idle();
                bus.idle();
            }
            0xE9 => self.pc = self.hl(),
            0xEA => {
                let addr = self.fetch16(bus);
                bus.write(addr, self.a);
            }
            0xFA => {
                let addr = self.fetch16(bus);
                self.a = bus.read(addr);
            }
            0xF3 => {
                self.ime = false;
                self.ime_pending = false;
            }
            0xFB => self.ime_pending = true,
            0xF8 => {
                let e = self.fetch(bus);
                let v = self.sp_offset(e);
                self.set_hl(v);
                bus.idle();
            }
            0xF9 => {
                bus.idle();
                self.sp = self.hl();
            }
            // D3, DB, DD, E3, E4, EB, EC, ED, F4, FC, FD: inexistentes.
            _ => self.locked = true,
        }
    }

    fn execute_cb(&mut self, bus: &mut Bus, op: u8) {
        let r = op & 7;
        let v = self.r8(bus, r);
        let bit = (op >> 3) & 7;
        match op >> 6 {
            0 => {
                let carry_in = u8::from(self.flag(C));
                let (result, carry) = match bit {
                    0 => (v.rotate_left(1), v & 0x80 != 0),
                    1 => (v.rotate_right(1), v & 1 != 0),
                    2 => ((v << 1) | carry_in, v & 0x80 != 0),
                    3 => ((v >> 1) | (carry_in << 7), v & 1 != 0),
                    4 => (v << 1, v & 0x80 != 0),
                    5 => (((v as i8) >> 1) as u8, v & 1 != 0),
                    6 => (v.rotate_left(4), false),
                    _ => (v >> 1, v & 1 != 0),
                };
                self.set_flags(result == 0, false, false, carry);
                self.set_r8(bus, r, result);
            }
            1 => self.set_flags(v & (1 << bit) == 0, false, true, self.flag(C)),
            2 => self.set_r8(bus, r, v & !(1 << bit)),
            _ => self.set_r8(bus, r, v | (1 << bit)),
        }
    }

    pub fn save_state(&self, w: &mut Writer) {
        for v in [
            self.a, self.f, self.b, self.c, self.d, self.e, self.h, self.l,
        ] {
            w.u8(v);
        }
        w.u16(self.sp);
        w.u16(self.pc);
        for v in [
            self.ime,
            self.ime_pending,
            self.halted,
            self.halt_bug,
            self.locked,
        ] {
            w.bool(v);
        }
    }

    pub fn load_state(&mut self, r: &mut Reader) -> Result<(), StateError> {
        *self = Self {
            a: r.u8()?,
            f: r.u8()? & 0xF0,
            b: r.u8()?,
            c: r.u8()?,
            d: r.u8()?,
            e: r.u8()?,
            h: r.u8()?,
            l: r.u8()?,
            sp: r.u16()?,
            pc: r.u16()?,
            ime: r.bool()?,
            ime_pending: r.bool()?,
            halted: r.bool()?,
            halt_bug: r.bool()?,
            locked: r.bool()?,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests;
