//! Interpretador do Zilog Z80.
//!
//! Implementa o conjunto completo de instruções (incluindo as não
//! documentadas usadas por jogos de Master System), com flags exatas — bits
//! 3 e 5, registrador interno `Q` (SCF/CCF) e `MEMPTR` (BIT n,(HL)) — e
//! contagem de ciclos por instrução. A CPU não conhece o hardware: memória,
//! portas e a linha de interrupção chegam pelo trait [`Bus`].
//!
//! Limitação conhecida: nas repetições intermediárias de LDIR/CPIR/INIR/OTIR
//! os bits 3/5 não seguem o comportamento do PC descoberto em 2018; o valor
//! final (última iteração) é exato.

mod exec;

/// Flags do registrador F.
pub mod flag {
    pub const C: u8 = 0x01;
    pub const N: u8 = 0x02;
    pub const PV: u8 = 0x04;
    pub const X: u8 = 0x08;
    pub const H: u8 = 0x10;
    pub const Y: u8 = 0x20;
    pub const Z: u8 = 0x40;
    pub const S: u8 = 0x80;
}

use flag::*;

/// Tudo o que a CPU enxerga fora dela.
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);
    fn input(&mut self, port: u16) -> u8;
    fn output(&mut self, port: u16, value: u8);
    /// Nível atual da linha /INT (interrupção mascarável).
    fn int_line(&self) -> bool;
}

/// Estado da CPU. Os registradores são públicos para depuração e testes.
#[derive(Debug, Clone)]
pub struct Z80 {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub af_alt: u16,
    pub bc_alt: u16,
    pub de_alt: u16,
    pub hl_alt: u16,
    pub ix: u16,
    pub iy: u16,
    pub sp: u16,
    pub pc: u16,
    pub i: u8,
    pub r: u8,
    pub iff1: bool,
    pub iff2: bool,
    pub im: u8,
    pub halted: bool,
    /// MEMPTR: registrador interno que vaza nos bits 3/5 de BIT n,(HL).
    pub(crate) wz: u16,
    /// Valor de F se a última instrução alterou flags; senão 0 (SCF/CCF).
    q: u8,
    flags_written: bool,
    /// EI só libera interrupções depois da instrução seguinte.
    ei_delay: bool,
    nmi_pending: bool,
}

impl Default for Z80 {
    fn default() -> Self {
        Self::new()
    }
}

impl Z80 {
    /// CPU no estado de reset.
    pub fn new() -> Self {
        Self {
            a: 0xFF,
            f: 0xFF,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            af_alt: 0xFFFF,
            bc_alt: 0,
            de_alt: 0,
            hl_alt: 0,
            ix: 0xFFFF,
            iy: 0xFFFF,
            sp: 0xFFFF,
            pc: 0,
            i: 0,
            r: 0,
            iff1: false,
            iff2: false,
            im: 0,
            halted: false,
            wz: 0,
            q: 0,
            flags_written: false,
            ei_delay: false,
            nmi_pending: false,
        }
    }

    /// Sinaliza uma NMI (borda); é atendida antes da próxima instrução.
    pub fn request_nmi(&mut self) {
        self.nmi_pending = true;
    }

    /// Executa uma instrução (ou atende uma interrupção) e devolve os
    /// T-states gastos.
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u32 {
        if self.nmi_pending {
            self.nmi_pending = false;
            self.halted = false;
            self.inc_r();
            self.iff1 = false;
            self.push(bus, self.pc);
            self.pc = 0x0066;
            self.wz = self.pc;
            self.q = 0;
            return 11;
        }
        if self.iff1 && !self.ei_delay && bus.int_line() {
            return self.accept_int(bus);
        }
        self.ei_delay = false;
        if self.halted {
            self.inc_r();
            self.q = 0;
            return 4;
        }
        self.flags_written = false;
        let cycles = self.execute(bus);
        self.q = if self.flags_written { self.f } else { 0 };
        cycles
    }

    fn accept_int<B: Bus>(&mut self, bus: &mut B) -> u32 {
        self.halted = false;
        self.inc_r();
        self.iff1 = false;
        self.iff2 = false;
        self.q = 0;
        self.push(bus, self.pc);
        if self.im == 2 {
            // No Master System o barramento de dados flutua em 0xFF.
            let vector = u16::from_be_bytes([self.i, 0xFF]);
            self.pc = self.read16(bus, vector);
            self.wz = self.pc;
            19
        } else {
            // IM 0 com o barramento em 0xFF executa RST 38h, igual a IM 1.
            self.pc = 0x0038;
            self.wz = self.pc;
            13
        }
    }

    // ---- Registradores --------------------------------------------------

    pub fn af(&self) -> u16 {
        u16::from_be_bytes([self.a, self.f])
    }
    pub fn bc(&self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }
    pub fn de(&self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }
    pub fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }
    pub fn set_af(&mut self, v: u16) {
        [self.a, self.f] = v.to_be_bytes();
    }
    pub fn set_bc(&mut self, v: u16) {
        [self.b, self.c] = v.to_be_bytes();
    }
    pub fn set_de(&mut self, v: u16) {
        [self.d, self.e] = v.to_be_bytes();
    }
    pub fn set_hl(&mut self, v: u16) {
        [self.h, self.l] = v.to_be_bytes();
    }

    fn set_f(&mut self, v: u8) {
        self.f = v;
        self.flags_written = true;
    }

    fn inc_r(&mut self) {
        self.r = (self.r & 0x80) | (self.r.wrapping_add(1) & 0x7F);
    }

    // ---- Memória --------------------------------------------------------

    fn fetch<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn fetch16<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }

    fn read16<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u16 {
        let lo = bus.read(addr);
        let hi = bus.read(addr.wrapping_add(1));
        u16::from_le_bytes([lo, hi])
    }

    fn write16<B: Bus>(&mut self, bus: &mut B, addr: u16, v: u16) {
        let [lo, hi] = v.to_le_bytes();
        bus.write(addr, lo);
        bus.write(addr.wrapping_add(1), hi);
    }

    fn push<B: Bus>(&mut self, bus: &mut B, v: u16) {
        let [lo, hi] = v.to_le_bytes();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, hi);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, lo);
    }

    fn pop<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let v = self.read16(bus, self.sp);
        self.sp = self.sp.wrapping_add(2);
        v
    }

    // ---- ULA ------------------------------------------------------------

    fn condition(&self, cc: u8) -> bool {
        match cc {
            0 => self.f & Z == 0,
            1 => self.f & Z != 0,
            2 => self.f & C == 0,
            3 => self.f & C != 0,
            4 => self.f & PV == 0,
            5 => self.f & PV != 0,
            6 => self.f & S == 0,
            _ => self.f & S != 0,
        }
    }

    fn add8(&mut self, v: u8, carry: bool) {
        let a = self.a;
        let wide = u16::from(a) + u16::from(v) + u16::from(carry);
        let r = wide as u8;
        let mut f = sz_xy(r) | ((a ^ v ^ r) & H);
        if (a ^ r) & (v ^ r) & 0x80 != 0 {
            f |= PV;
        }
        if wide > 0xFF {
            f |= C;
        }
        self.a = r;
        self.set_f(f);
    }

    /// Subtração com flags; devolve o resultado sem gravar em A.
    fn sub8_flags(&mut self, v: u8, carry: bool) -> u8 {
        let a = self.a;
        let wide = u16::from(a)
            .wrapping_sub(u16::from(v))
            .wrapping_sub(u16::from(carry));
        let r = wide as u8;
        let mut f = sz_xy(r) | ((a ^ v ^ r) & H) | N;
        if (a ^ v) & (a ^ r) & 0x80 != 0 {
            f |= PV;
        }
        if wide > 0xFF {
            f |= C;
        }
        self.set_f(f);
        r
    }

    fn alu(&mut self, op: u8, v: u8) {
        match op {
            0 => self.add8(v, false),
            1 => self.add8(v, self.f & C != 0),
            2 => self.a = self.sub8_flags(v, false),
            3 => self.a = self.sub8_flags(v, self.f & C != 0),
            4 => {
                self.a &= v;
                self.set_f(szp_xy(self.a) | H);
            }
            5 => {
                self.a ^= v;
                self.set_f(szp_xy(self.a));
            }
            6 => {
                self.a |= v;
                self.set_f(szp_xy(self.a));
            }
            _ => {
                // CP: bits 3/5 vêm do operando, não do resultado.
                self.sub8_flags(v, false);
                self.f = (self.f & !(X | Y)) | (v & (X | Y));
            }
        }
    }

    fn inc8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_add(1);
        let mut f = (self.f & C) | sz_xy(r);
        if v & 0x0F == 0x0F {
            f |= H;
        }
        if v == 0x7F {
            f |= PV;
        }
        self.set_f(f);
        r
    }

    fn dec8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_sub(1);
        let mut f = (self.f & C) | sz_xy(r) | N;
        if v & 0x0F == 0 {
            f |= H;
        }
        if v == 0x80 {
            f |= PV;
        }
        self.set_f(f);
        r
    }

    fn add16(&mut self, a: u16, b: u16) -> u16 {
        self.wz = a.wrapping_add(1);
        let wide = u32::from(a) + u32::from(b);
        let r = wide as u16;
        let hi = (r >> 8) as u8;
        let mut f = (self.f & (S | Z | PV)) | (hi & (X | Y));
        if (a ^ b ^ r) & 0x1000 != 0 {
            f |= H;
        }
        if wide > 0xFFFF {
            f |= C;
        }
        self.set_f(f);
        r
    }

    fn adc16(&mut self, b: u16) {
        let a = self.hl();
        self.wz = a.wrapping_add(1);
        let wide = u32::from(a) + u32::from(b) + u32::from(self.f & C);
        let r = wide as u16;
        let mut f = flags16(r);
        if (a ^ b ^ r) & 0x1000 != 0 {
            f |= H;
        }
        if (a ^ r) & (b ^ r) & 0x8000 != 0 {
            f |= PV;
        }
        if wide > 0xFFFF {
            f |= C;
        }
        self.set_hl(r);
        self.set_f(f);
    }

    fn sbc16(&mut self, b: u16) {
        let a = self.hl();
        self.wz = a.wrapping_add(1);
        let wide = u32::from(a)
            .wrapping_sub(u32::from(b))
            .wrapping_sub(u32::from(self.f & C));
        let r = wide as u16;
        let mut f = flags16(r) | N;
        if (a ^ b ^ r) & 0x1000 != 0 {
            f |= H;
        }
        if (a ^ b) & (a ^ r) & 0x8000 != 0 {
            f |= PV;
        }
        if wide > 0xFFFF {
            f |= C;
        }
        self.set_hl(r);
        self.set_f(f);
    }

    /// Rotações e deslocamentos do prefixo CB (`op` = campo y).
    fn rot(&mut self, op: u8, v: u8) -> u8 {
        let carry_in = self.f & C;
        let (r, carry) = match op {
            0 => (v.rotate_left(1), v >> 7),
            1 => (v.rotate_right(1), v & 1),
            2 => ((v << 1) | carry_in, v >> 7),
            3 => ((v >> 1) | (carry_in << 7), v & 1),
            4 => (v << 1, v >> 7),
            5 => ((v >> 1) | (v & 0x80), v & 1),
            6 => ((v << 1) | 1, v >> 7),
            _ => (v >> 1, v & 1),
        };
        self.set_f(szp_xy(r) | carry);
        r
    }

    /// RLCA/RRCA/RLA/RRA: preservam S, Z e P/V.
    fn rot_a(&mut self, op: u8) {
        let v = self.a;
        let carry_in = self.f & C;
        let (r, carry) = match op {
            0 => (v.rotate_left(1), v >> 7),
            1 => (v.rotate_right(1), v & 1),
            2 => ((v << 1) | carry_in, v >> 7),
            _ => ((v >> 1) | (carry_in << 7), v & 1),
        };
        self.a = r;
        self.set_f((self.f & (S | Z | PV)) | (r & (X | Y)) | carry);
    }

    fn bit(&mut self, n: u8, v: u8, xy_source: u8) {
        let set = v & (1 << n);
        let mut f = (self.f & C) | H | (xy_source & (X | Y));
        if set == 0 {
            f |= Z | PV;
        }
        if n == 7 && set != 0 {
            f |= S;
        }
        self.set_f(f);
    }

    fn daa(&mut self) {
        let a = self.a;
        let lo = a & 0x0F;
        let subtract = self.f & N != 0;
        let mut carry = self.f & C != 0;
        let mut diff = 0u8;
        if self.f & H != 0 || lo > 9 {
            diff |= 0x06;
        }
        if carry || a > 0x99 {
            diff |= 0x60;
            carry = true;
        }
        let r = if subtract {
            a.wrapping_sub(diff)
        } else {
            a.wrapping_add(diff)
        };
        let half = if subtract {
            self.f & H != 0 && lo < 6
        } else {
            lo > 9
        };
        let mut f = szp_xy(r) | (self.f & N);
        if half {
            f |= H;
        }
        if carry {
            f |= C;
        }
        self.a = r;
        self.set_f(f);
    }
}

impl Z80 {
    pub fn save_state(&self, w: &mut crate::state::Writer) {
        for v in [
            self.a, self.f, self.b, self.c, self.d, self.e, self.h, self.l,
        ] {
            w.u8(v);
        }
        for v in [
            self.af_alt,
            self.bc_alt,
            self.de_alt,
            self.hl_alt,
            self.ix,
            self.iy,
            self.sp,
            self.pc,
            self.wz,
        ] {
            w.u16(v);
        }
        w.u8(self.i);
        w.u8(self.r);
        w.u8(self.im);
        w.u8(self.q);
        for v in [
            self.iff1,
            self.iff2,
            self.halted,
            self.ei_delay,
            self.nmi_pending,
        ] {
            w.bool(v);
        }
    }

    pub fn load_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        for reg in [
            &mut self.a,
            &mut self.f,
            &mut self.b,
            &mut self.c,
            &mut self.d,
            &mut self.e,
            &mut self.h,
            &mut self.l,
        ] {
            *reg = r.u8()?;
        }
        for reg in [
            &mut self.af_alt,
            &mut self.bc_alt,
            &mut self.de_alt,
            &mut self.hl_alt,
            &mut self.ix,
            &mut self.iy,
            &mut self.sp,
            &mut self.pc,
            &mut self.wz,
        ] {
            *reg = r.u16()?;
        }
        self.i = r.u8()?;
        self.r = r.u8()?;
        self.im = r.u8()?;
        if self.im > 2 {
            return Err(crate::state::StateError::Invalid("modo de interrupção"));
        }
        self.q = r.u8()?;
        for flag in [
            &mut self.iff1,
            &mut self.iff2,
            &mut self.halted,
            &mut self.ei_delay,
            &mut self.nmi_pending,
        ] {
            *flag = r.bool()?;
        }
        self.flags_written = false;
        Ok(())
    }
}

/// S, Z e bits 3/5 de um resultado de 8 bits.
fn sz_xy(r: u8) -> u8 {
    let mut f = r & (S | X | Y);
    if r == 0 {
        f |= Z;
    }
    f
}

/// S, Z, paridade e bits 3/5.
fn szp_xy(r: u8) -> u8 {
    let mut f = sz_xy(r);
    if r.count_ones().is_multiple_of(2) {
        f |= PV;
    }
    f
}

/// S, Z e bits 3/5 de um resultado de 16 bits (ADC/SBC HL).
fn flags16(r: u16) -> u8 {
    let hi = (r >> 8) as u8;
    let mut f = hi & (S | X | Y);
    if r == 0 {
        f |= Z;
    }
    f
}

#[cfg(test)]
mod tests;
