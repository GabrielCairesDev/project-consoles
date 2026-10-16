//! Decodificação e execução das instruções.
//!
//! Segue a decomposição clássica do opcode em campos `x` (bits 7-6), `y`
//! (5-3), `z` (2-0), `p` (5-4) e `q` (3). Os prefixos DD/FD trocam HL por
//! IX/IY: H e L viram IXH/IXL, e (HL) vira (IX+d) — nesse caso, H e L
//! continuam sendo os registradores reais.

use super::flag::*;
use super::{Bus, Z80, szp_xy};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Index {
    Hl,
    Ix,
    Iy,
}

/// Ciclos extras de `(IX+d)` em relação a `(HL)`: busca de `d` e soma.
const DISP_CYCLES: u32 = 8;

impl Z80 {
    pub(super) fn execute<B: Bus>(&mut self, bus: &mut B) -> u32 {
        self.inc_r();
        let op = self.fetch(bus);
        match op {
            0xCB => self.exec_cb(bus),
            0xED => self.exec_ed(bus),
            0xDD => self.exec_indexed(bus, Index::Ix),
            0xFD => self.exec_indexed(bus, Index::Iy),
            _ => self.exec_main(bus, op, Index::Hl),
        }
    }

    fn exec_indexed<B: Bus>(&mut self, bus: &mut B, idx: Index) -> u32 {
        let op = bus.read(self.pc);
        // Outro prefixo logo depois: este age como NOP e o seguinte começa
        // uma nova instrução.
        if matches!(op, 0xDD | 0xFD | 0xED) {
            return 4;
        }
        self.inc_r();
        self.pc = self.pc.wrapping_add(1);
        if op == 0xCB {
            return self.exec_indexed_cb(bus, idx);
        }
        4 + self.exec_main(bus, op, idx)
    }

    // ---- Acesso a operandos ---------------------------------------------

    fn index_reg(&self, idx: Index) -> u16 {
        match idx {
            Index::Hl => self.hl(),
            Index::Ix => self.ix,
            Index::Iy => self.iy,
        }
    }

    fn set_index_reg(&mut self, idx: Index, v: u16) {
        match idx {
            Index::Hl => self.set_hl(v),
            Index::Ix => self.ix = v,
            Index::Iy => self.iy = v,
        }
    }

    /// Endereço de `(HL)` ou `(IX+d)`; no caso indexado busca `d`.
    fn mem_operand<B: Bus>(&mut self, bus: &mut B, idx: Index) -> u16 {
        match idx {
            Index::Hl => self.hl(),
            _ => {
                let d = self.fetch(bus) as i8;
                let addr = self.index_reg(idx).wrapping_add_signed(i16::from(d));
                self.wz = addr;
                addr
            }
        }
    }

    /// Registrador de 8 bits pelo índice do opcode (6 = (HL) é tratado à parte).
    fn reg(&self, r: u8, idx: Index) -> u8 {
        match r {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => (self.index_reg(idx) >> 8) as u8,
            5 => self.index_reg(idx) as u8,
            _ => self.a,
        }
    }

    fn set_reg(&mut self, r: u8, idx: Index, v: u8) {
        match r {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => {
                let low = self.index_reg(idx) & 0x00FF;
                self.set_index_reg(idx, (u16::from(v) << 8) | low);
            }
            5 => {
                let high = self.index_reg(idx) & 0xFF00;
                self.set_index_reg(idx, high | u16::from(v));
            }
            _ => self.a = v,
        }
    }

    /// Pares BC, DE, HL/IX/IY, SP.
    fn rp(&self, p: u8, idx: Index) -> u16 {
        match p {
            0 => self.bc(),
            1 => self.de(),
            2 => self.index_reg(idx),
            _ => self.sp,
        }
    }

    fn set_rp(&mut self, p: u8, idx: Index, v: u16) {
        match p {
            0 => self.set_bc(v),
            1 => self.set_de(v),
            2 => self.set_index_reg(idx, v),
            _ => self.sp = v,
        }
    }

    /// Pares de PUSH/POP: BC, DE, HL/IX/IY, AF.
    fn rp2(&self, p: u8, idx: Index) -> u16 {
        if p == 3 { self.af() } else { self.rp(p, idx) }
    }

    fn set_rp2(&mut self, p: u8, idx: Index, v: u16) {
        if p == 3 {
            // POP AF não conta como instrução que altera flags (Q = 0).
            self.set_af(v);
        } else {
            self.set_rp(p, idx, v);
        }
    }

    fn jump_relative<B: Bus>(&mut self, bus: &mut B, taken: bool) -> bool {
        let d = self.fetch(bus) as i8;
        if taken {
            self.pc = self.pc.wrapping_add_signed(i16::from(d));
            self.wz = self.pc;
        }
        taken
    }

    // ---- Sem prefixo / DD / FD --------------------------------------------

    fn exec_main<B: Bus>(&mut self, bus: &mut B, op: u8, idx: Index) -> u32 {
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        let p = y >> 1;
        let q = y & 1;
        let disp = if idx == Index::Hl { 0 } else { DISP_CYCLES };

        match x {
            0 => match z {
                0 => match y {
                    0 => 4,
                    1 => {
                        let af = self.af();
                        self.set_af(self.af_alt);
                        self.af_alt = af;
                        4
                    }
                    2 => {
                        self.b = self.b.wrapping_sub(1);
                        if self.jump_relative(bus, self.b != 0) {
                            13
                        } else {
                            8
                        }
                    }
                    3 => {
                        self.jump_relative(bus, true);
                        12
                    }
                    _ => {
                        if self.jump_relative(bus, self.condition(y - 4)) {
                            12
                        } else {
                            7
                        }
                    }
                },
                1 => {
                    if q == 0 {
                        let nn = self.fetch16(bus);
                        self.set_rp(p, idx, nn);
                        10
                    } else {
                        let r = self.add16(self.index_reg(idx), self.rp(p, idx));
                        self.set_index_reg(idx, r);
                        11
                    }
                }
                2 => self.exec_indirect_load(bus, p, q, idx),
                3 => {
                    let v = self.rp(p, idx);
                    let r = if q == 0 {
                        v.wrapping_add(1)
                    } else {
                        v.wrapping_sub(1)
                    };
                    self.set_rp(p, idx, r);
                    6
                }
                4 | 5 => {
                    let inc = z == 4;
                    if y == 6 {
                        let addr = self.mem_operand(bus, idx);
                        let v = bus.read(addr);
                        let r = if inc { self.inc8(v) } else { self.dec8(v) };
                        bus.write(addr, r);
                        11 + disp
                    } else {
                        let v = self.reg(y, idx);
                        let r = if inc { self.inc8(v) } else { self.dec8(v) };
                        self.set_reg(y, idx, r);
                        4
                    }
                }
                6 => {
                    if y == 6 {
                        let addr = self.mem_operand(bus, idx);
                        let n = self.fetch(bus);
                        bus.write(addr, n);
                        // Em LD (IX+d),n a busca de n se sobrepõe à soma.
                        if idx == Index::Hl { 10 } else { 15 }
                    } else {
                        let n = self.fetch(bus);
                        self.set_reg(y, idx, n);
                        7
                    }
                }
                _ => {
                    match y {
                        0..=3 => self.rot_a(y),
                        4 => self.daa(),
                        5 => {
                            self.a = !self.a;
                            let f = (self.f & (S | Z | PV | C)) | H | N | (self.a & (X | Y));
                            self.set_f(f);
                        }
                        6 => {
                            let xy = ((self.q ^ self.f) | self.a) & (X | Y);
                            self.set_f((self.f & (S | Z | PV)) | xy | C);
                        }
                        _ => {
                            let xy = ((self.q ^ self.f) | self.a) & (X | Y);
                            let half = if self.f & C != 0 { H } else { 0 };
                            let carry = (self.f & C) ^ C;
                            self.set_f((self.f & (S | Z | PV)) | xy | half | carry);
                        }
                    }
                    4
                }
            },
            1 => {
                if y == 6 && z == 6 {
                    self.halted = true;
                    4
                } else if y == 6 {
                    let addr = self.mem_operand(bus, idx);
                    bus.write(addr, self.reg(z, Index::Hl));
                    7 + disp
                } else if z == 6 {
                    let addr = self.mem_operand(bus, idx);
                    let v = bus.read(addr);
                    self.set_reg(y, Index::Hl, v);
                    7 + disp
                } else {
                    let v = self.reg(z, idx);
                    self.set_reg(y, idx, v);
                    4
                }
            }
            2 => {
                if z == 6 {
                    let addr = self.mem_operand(bus, idx);
                    let v = bus.read(addr);
                    self.alu(y, v);
                    7 + disp
                } else {
                    let v = self.reg(z, idx);
                    self.alu(y, v);
                    4
                }
            }
            _ => self.exec_x3(bus, y, z, p, q, idx),
        }
    }

    fn exec_indirect_load<B: Bus>(&mut self, bus: &mut B, p: u8, q: u8, idx: Index) -> u32 {
        match (q, p) {
            (0, 0 | 1) => {
                let addr = if p == 0 { self.bc() } else { self.de() };
                bus.write(addr, self.a);
                self.wz = u16::from_be_bytes([self.a, addr.wrapping_add(1) as u8]);
                7
            }
            (0, 2) => {
                let nn = self.fetch16(bus);
                let v = self.index_reg(idx);
                self.write16(bus, nn, v);
                self.wz = nn.wrapping_add(1);
                16
            }
            (0, _) => {
                let nn = self.fetch16(bus);
                bus.write(nn, self.a);
                self.wz = u16::from_be_bytes([self.a, nn.wrapping_add(1) as u8]);
                13
            }
            (_, 0 | 1) => {
                let addr = if p == 0 { self.bc() } else { self.de() };
                self.a = bus.read(addr);
                self.wz = addr.wrapping_add(1);
                7
            }
            (_, 2) => {
                let nn = self.fetch16(bus);
                let v = self.read16(bus, nn);
                self.set_index_reg(idx, v);
                self.wz = nn.wrapping_add(1);
                16
            }
            _ => {
                let nn = self.fetch16(bus);
                self.a = bus.read(nn);
                self.wz = nn.wrapping_add(1);
                13
            }
        }
    }

    fn exec_x3<B: Bus>(&mut self, bus: &mut B, y: u8, z: u8, p: u8, q: u8, idx: Index) -> u32 {
        match z {
            0 => {
                if self.condition(y) {
                    self.pc = self.pop(bus);
                    self.wz = self.pc;
                    11
                } else {
                    5
                }
            }
            1 => {
                if q == 0 {
                    let v = self.pop(bus);
                    self.set_rp2(p, idx, v);
                    10
                } else {
                    match p {
                        0 => {
                            self.pc = self.pop(bus);
                            self.wz = self.pc;
                            10
                        }
                        1 => {
                            let (bc, de, hl) = (self.bc(), self.de(), self.hl());
                            self.set_bc(self.bc_alt);
                            self.set_de(self.de_alt);
                            self.set_hl(self.hl_alt);
                            self.bc_alt = bc;
                            self.de_alt = de;
                            self.hl_alt = hl;
                            4
                        }
                        2 => {
                            self.pc = self.index_reg(idx);
                            4
                        }
                        _ => {
                            self.sp = self.index_reg(idx);
                            6
                        }
                    }
                }
            }
            2 => {
                let nn = self.fetch16(bus);
                self.wz = nn;
                if self.condition(y) {
                    self.pc = nn;
                }
                10
            }
            3 => match y {
                0 => {
                    self.pc = self.fetch16(bus);
                    self.wz = self.pc;
                    10
                }
                2 => {
                    let n = self.fetch(bus);
                    let port = u16::from_be_bytes([self.a, n]);
                    bus.output(port, self.a);
                    self.wz = u16::from_be_bytes([self.a, n.wrapping_add(1)]);
                    11
                }
                3 => {
                    let n = self.fetch(bus);
                    let port = u16::from_be_bytes([self.a, n]);
                    self.a = bus.input(port);
                    self.wz = port.wrapping_add(1);
                    11
                }
                4 => {
                    let mem = self.read16(bus, self.sp);
                    let v = self.index_reg(idx);
                    self.write16(bus, self.sp, v);
                    self.set_index_reg(idx, mem);
                    self.wz = mem;
                    19
                }
                5 => {
                    // EX DE,HL não é afetado por DD/FD.
                    let de = self.de();
                    self.set_de(self.hl());
                    self.set_hl(de);
                    4
                }
                6 => {
                    self.iff1 = false;
                    self.iff2 = false;
                    4
                }
                7 => {
                    self.iff1 = true;
                    self.iff2 = true;
                    self.ei_delay = true;
                    4
                }
                // y = 1 é o prefixo CB, tratado antes de chegar aqui.
                _ => 4,
            },
            4 => {
                let nn = self.fetch16(bus);
                self.wz = nn;
                if self.condition(y) {
                    self.push(bus, self.pc);
                    self.pc = nn;
                    17
                } else {
                    10
                }
            }
            5 => {
                if q == 0 {
                    let v = self.rp2(p, idx);
                    self.push(bus, v);
                    11
                } else {
                    // p = 0: CALL nn. Os demais são prefixos, tratados antes.
                    let nn = self.fetch16(bus);
                    self.wz = nn;
                    self.push(bus, self.pc);
                    self.pc = nn;
                    17
                }
            }
            6 => {
                let n = self.fetch(bus);
                self.alu(y, n);
                7
            }
            _ => {
                self.push(bus, self.pc);
                self.pc = u16::from(y) * 8;
                self.wz = self.pc;
                11
            }
        }
    }

    // ---- CB ---------------------------------------------------------------

    fn exec_cb<B: Bus>(&mut self, bus: &mut B) -> u32 {
        self.inc_r();
        let op = self.fetch(bus);
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;

        if z == 6 {
            let addr = self.hl();
            let v = bus.read(addr);
            match x {
                0 => {
                    let r = self.rot(y, v);
                    bus.write(addr, r);
                    15
                }
                1 => {
                    self.bit(y, v, (self.wz >> 8) as u8);
                    12
                }
                2 => {
                    bus.write(addr, v & !(1 << y));
                    15
                }
                _ => {
                    bus.write(addr, v | (1 << y));
                    15
                }
            }
        } else {
            let v = self.reg(z, Index::Hl);
            match x {
                0 => {
                    let r = self.rot(y, v);
                    self.set_reg(z, Index::Hl, r);
                }
                1 => self.bit(y, v, v),
                2 => self.set_reg(z, Index::Hl, v & !(1 << y)),
                _ => self.set_reg(z, Index::Hl, v | (1 << y)),
            }
            8
        }
    }

    /// DD CB d op / FD CB d op. O resultado vai para a memória e, quando
    /// `z != 6`, também é copiado para o registrador (não documentado).
    fn exec_indexed_cb<B: Bus>(&mut self, bus: &mut B, idx: Index) -> u32 {
        let addr = self.mem_operand(bus, idx);
        let op = self.fetch(bus);
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        let v = bus.read(addr);

        let r = match x {
            0 => self.rot(y, v),
            1 => {
                self.bit(y, v, (addr >> 8) as u8);
                return 20;
            }
            2 => v & !(1 << y),
            _ => v | (1 << y),
        };
        bus.write(addr, r);
        if z != 6 {
            self.set_reg(z, Index::Hl, r);
        }
        23
    }

    // ---- ED ---------------------------------------------------------------

    fn exec_ed<B: Bus>(&mut self, bus: &mut B) -> u32 {
        self.inc_r();
        let op = self.fetch(bus);
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        let p = y >> 1;
        let q = y & 1;

        match x {
            1 => self.exec_ed_x1(bus, y, z, p, q),
            2 if z <= 3 && y >= 4 => self.exec_block(bus, y, z),
            // Opcodes ED inválidos se comportam como dois NOPs.
            _ => 8,
        }
    }

    fn exec_ed_x1<B: Bus>(&mut self, bus: &mut B, y: u8, z: u8, p: u8, q: u8) -> u32 {
        match z {
            0 => {
                let bc = self.bc();
                let v = bus.input(bc);
                self.wz = bc.wrapping_add(1);
                if y != 6 {
                    self.set_reg(y, Index::Hl, v);
                }
                self.set_f((self.f & C) | szp_xy(v));
                12
            }
            1 => {
                let bc = self.bc();
                let v = if y == 6 { 0 } else { self.reg(y, Index::Hl) };
                bus.output(bc, v);
                self.wz = bc.wrapping_add(1);
                12
            }
            2 => {
                let v = self.rp(p, Index::Hl);
                if q == 0 {
                    self.sbc16(v);
                } else {
                    self.adc16(v);
                }
                15
            }
            3 => {
                let nn = self.fetch16(bus);
                if q == 0 {
                    let v = self.rp(p, Index::Hl);
                    self.write16(bus, nn, v);
                } else {
                    let v = self.read16(bus, nn);
                    self.set_rp(p, Index::Hl, v);
                }
                self.wz = nn.wrapping_add(1);
                20
            }
            4 => {
                let v = self.a;
                self.a = 0;
                self.a = self.sub8_flags(v, false);
                8
            }
            5 => {
                // RETN e RETI: ambos restauram IFF1 a partir de IFF2.
                self.iff1 = self.iff2;
                self.pc = self.pop(bus);
                self.wz = self.pc;
                14
            }
            6 => {
                self.im = [0, 0, 1, 2][usize::from(y & 3)];
                8
            }
            _ => match y {
                0 => {
                    self.i = self.a;
                    9
                }
                1 => {
                    self.r = self.a;
                    9
                }
                2 | 3 => {
                    let v = if y == 2 { self.i } else { self.r };
                    self.a = v;
                    let mut f = (self.f & C) | super::sz_xy(v);
                    if self.iff2 {
                        f |= PV;
                    }
                    self.set_f(f);
                    9
                }
                4 | 5 => {
                    let addr = self.hl();
                    let v = bus.read(addr);
                    let (mem, a) = if y == 4 {
                        // RRD
                        ((self.a << 4) | (v >> 4), (self.a & 0xF0) | (v & 0x0F))
                    } else {
                        // RLD
                        ((v << 4) | (self.a & 0x0F), (self.a & 0xF0) | (v >> 4))
                    };
                    bus.write(addr, mem);
                    self.a = a;
                    self.wz = addr.wrapping_add(1);
                    self.set_f((self.f & C) | szp_xy(a));
                    18
                }
                _ => 8,
            },
        }
    }

    /// LDI/CPI/INI/OUTI e variantes (y: 4 = I, 5 = D, 6 = IR, 7 = DR).
    fn exec_block<B: Bus>(&mut self, bus: &mut B, y: u8, z: u8) -> u32 {
        let decrement = y & 1 == 1;
        let repeat = y >= 6;
        let step = |v: u16| {
            if decrement {
                v.wrapping_sub(1)
            } else {
                v.wrapping_add(1)
            }
        };

        let again = match z {
            0 => {
                let v = bus.read(self.hl());
                bus.write(self.de(), v);
                self.set_hl(step(self.hl()));
                self.set_de(step(self.de()));
                let bc = self.bc().wrapping_sub(1);
                self.set_bc(bc);
                let n = v.wrapping_add(self.a);
                let mut f = (self.f & (S | Z | C)) | (n & X) | ((n << 4) & Y);
                if bc != 0 {
                    f |= PV;
                }
                self.set_f(f);
                bc != 0
            }
            1 => {
                let v = bus.read(self.hl());
                let r = self.a.wrapping_sub(v);
                let half = (self.a ^ v ^ r) & H;
                self.set_hl(step(self.hl()));
                self.wz = step(self.wz);
                let bc = self.bc().wrapping_sub(1);
                self.set_bc(bc);
                let n = r.wrapping_sub(u8::from(half != 0));
                let mut f = (self.f & C) | (r & S) | half | N | (n & X) | ((n << 4) & Y);
                if r == 0 {
                    f |= Z;
                }
                if bc != 0 {
                    f |= PV;
                }
                self.set_f(f);
                bc != 0 && r != 0
            }
            2 => {
                let v = bus.input(self.bc());
                self.wz = step(self.bc());
                bus.write(self.hl(), v);
                self.b = self.b.wrapping_sub(1);
                self.set_hl(step(self.hl()));
                let c = if decrement {
                    self.c.wrapping_sub(1)
                } else {
                    self.c.wrapping_add(1)
                };
                self.io_block_flags(v, c);
                self.b != 0
            }
            _ => {
                let v = bus.read(self.hl());
                self.b = self.b.wrapping_sub(1);
                self.wz = step(self.bc());
                bus.output(self.bc(), v);
                self.set_hl(step(self.hl()));
                self.io_block_flags(v, self.l);
                self.b != 0
            }
        };

        if repeat && again {
            self.pc = self.pc.wrapping_sub(2);
            self.wz = self.pc.wrapping_add(1);
            21
        } else {
            16
        }
    }

    /// Flags de INI/IND/OUTI/OUTD (comportamento não documentado conhecido).
    fn io_block_flags(&mut self, v: u8, addend: u8) {
        let k = u16::from(v) + u16::from(addend);
        let mut f = super::sz_xy(self.b);
        if v & 0x80 != 0 {
            f |= N;
        }
        if k > 0xFF {
            f |= H | C;
        }
        if ((k as u8 & 7) ^ self.b).count_ones().is_multiple_of(2) {
            f |= PV;
        }
        self.set_f(f);
    }
}
