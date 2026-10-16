//! Modos de endereçamento.

use super::{Bus, Cpu, Size, Step};

/// Onde está o operando, já com as extensões consumidas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Loc {
    D(usize),
    A(usize),
    Mem {
        addr: u32,
        /// (An)+ pendente: o incremento só vale depois de um acesso bem
        /// sucedido (num erro de endereço, An fica intacto).
        post: Option<(usize, u32)>,
        /// -(An) em long: o 68000 acessa primeiro a palavra baixa; num erro
        /// de endereço An fica decrementado de só 2.
        predec_long: Option<usize>,
    },
    Imm(u32),
}

impl Loc {
    fn mem(addr: u32) -> Self {
        Loc::Mem {
            addr,
            post: None,
            predec_long: None,
        }
    }
}

/// Ordem dos acessos de memória, que só aparece num erro de endereço.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// (An)+ incrementa antes do acesso; -(An) long acessa a palavra alta primeiro.
    Normal,
    /// -(An) long acessa a palavra baixa primeiro (ADDX/SUBX).
    LowWordFirst,
    /// Destino de MOVE: (An)+ só incrementa após o acesso e -(An) long
    /// acessa a palavra baixa primeiro.
    MoveDestination,
}

/// Classes de modo válidas para cada instrução (manual da Motorola).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    /// Todos os modos.
    All,
    /// Todos menos An.
    Data,
    /// Dados e alterável: Dn e memória exceto PC-relativo e imediato.
    DataAlterable,
    /// Alterável: Dn, An e memória exceto PC-relativo e imediato.
    Alterable,
    /// Memória alterável.
    MemoryAlterable,
    /// Controle: (An), d16(An), d8(An,Xn), abs, PC-relativo.
    Control,
    /// Controle alterável (sem PC-relativo).
    ControlAlterable,
    /// Dados sem imediato (CMPI e BTST com destino em memória no 68000).
    DataNoImmediate,
}

/// O modo (`mode`, `reg`) existe na classe?
pub(crate) fn valid(mode: u16, reg: u16, class: Class) -> bool {
    let kind = match mode {
        0 => 0,
        1 => 1,
        2..=6 => 2,
        _ => match reg {
            0 | 1 => 3, // absoluto
            2 | 3 => 4, // PC-relativo
            4 => 5,     // imediato
            _ => return false,
        },
    };
    let control = matches!(mode, 2 | 5 | 6) || matches!(kind, 3 | 4);
    match class {
        Class::All => true,
        Class::Data => kind != 1,
        Class::DataAlterable => matches!(kind, 0 | 2 | 3),
        Class::Alterable => matches!(kind, 0..=3),
        Class::MemoryAlterable => matches!(kind, 2 | 3),
        Class::Control => control,
        Class::ControlAlterable => control && kind != 4,
        Class::DataNoImmediate => matches!(kind, 0 | 2 | 3 | 4),
    }
}

impl Cpu {
    /// Calcula o endereço efetivo e consome as extensões.
    ///
    /// `predec_idle`: -(An) custa 2 ciclos internos na maioria das
    /// instruções, mas não no destino de MOVE (nem no segundo operando de
    /// ADDX/SUBX/ABCD/SBCD).
    pub(crate) fn ea<B: Bus>(
        &mut self,
        bus: &mut B,
        mode: u16,
        reg: u16,
        size: Size,
        predec_idle: bool,
    ) -> Step<Loc> {
        self.ea_with(bus, mode, reg, size, predec_idle, Access::Normal)
    }

    /// Como [`Cpu::ea`], com a ordem de acesso de MOVE ou ADDX/SUBX.
    pub(crate) fn ea_with<B: Bus>(
        &mut self,
        bus: &mut B,
        mode: u16,
        reg: u16,
        size: Size,
        predec_idle: bool,
        access: Access,
    ) -> Step<Loc> {
        let r = usize::from(reg);
        Ok(match mode {
            0 => Loc::D(r),
            1 => Loc::A(r),
            2 => Loc::mem(self.a[r]),
            3 if access == Access::MoveDestination => Loc::Mem {
                addr: self.a[r],
                post: Some((r, self.step_size(size, r))),
                predec_long: None,
            },
            3 => {
                let addr = self.a[r];
                self.a[r] = addr.wrapping_add(self.step_size(size, r));
                Loc::mem(addr)
            }
            4 => {
                if predec_idle {
                    self.idle(2);
                }
                let addr = self.a[r].wrapping_sub(self.step_size(size, r));
                self.a[r] = addr;
                Loc::Mem {
                    addr,
                    post: None,
                    predec_long: (size == Size::Long && access != Access::Normal).then_some(r),
                }
            }
            5 => {
                let disp = self.fetch_ext(bus)? as i16;
                Loc::mem(self.a[r].wrapping_add_signed(i32::from(disp)))
            }
            6 => {
                let ext = self.fetch_ext(bus)?;
                self.idle(2);
                Loc::mem(self.a[r].wrapping_add(self.index(ext)))
            }
            _ => match reg {
                0 => Loc::mem(self.fetch_ext(bus)? as i16 as i32 as u32),
                1 => Loc::mem(self.fetch_ext_long(bus)?),
                2 => {
                    let base = self.pc;
                    let disp = self.fetch_ext(bus)? as i16;
                    Loc::mem(base.wrapping_add_signed(i32::from(disp)))
                }
                3 => {
                    let base = self.pc;
                    let ext = self.fetch_ext(bus)?;
                    self.idle(2);
                    Loc::mem(base.wrapping_add(self.index(ext)))
                }
                _ => Loc::Imm(match size {
                    Size::Byte => u32::from(self.fetch_ext(bus)? & 0xFF),
                    Size::Word => u32::from(self.fetch_ext(bus)?),
                    Size::Long => self.fetch_ext_long(bus)?,
                }),
            },
        })
    }

    /// Endereço de um modo de controle, sem ler o operando (LEA, PEA, JMP,
    /// JSR, MOVEM). Com `refill_last = false`, a última extensão sai de IRC
    /// sem buscar a próxima palavra (o desvio vai esvaziar a fila).
    /// Os ciclos internos ficam por conta de quem chama.
    pub(crate) fn control_addr<B: Bus>(
        &mut self,
        bus: &mut B,
        mode: u16,
        reg: u16,
        refill_last: bool,
    ) -> Step<u32> {
        let r = usize::from(reg);
        let ext = |cpu: &mut Self, bus: &mut B| -> Step<u16> {
            if refill_last {
                cpu.fetch_ext(bus)
            } else {
                Ok(cpu.take_ext())
            }
        };
        Ok(match (mode, reg) {
            (2, _) => self.a[r],
            (5, _) => {
                let disp = ext(self, bus)? as i16;
                self.a[r].wrapping_add_signed(i32::from(disp))
            }
            (6, _) => {
                let e = ext(self, bus)?;
                self.a[r].wrapping_add(self.index(e))
            }
            (7, 0) => ext(self, bus)? as i16 as i32 as u32,
            (7, 1) => {
                let hi = self.fetch_ext(bus)?;
                let lo = ext(self, bus)?;
                u32::from(hi) << 16 | u32::from(lo)
            }
            (7, 2) => {
                let base = self.pc;
                let disp = ext(self, bus)? as i16;
                base.wrapping_add_signed(i32::from(disp))
            }
            _ => {
                let base = self.pc;
                let e = ext(self, bus)?;
                base.wrapping_add(self.index(e))
            }
        })
    }

    /// Deslocamento de 8 bits mais o registrador de índice da extensão breve.
    pub(crate) fn index(&self, ext: u16) -> u32 {
        let reg = usize::from((ext >> 12) & 7);
        let value = if ext & 0x8000 != 0 {
            self.a[reg]
        } else {
            self.d[reg]
        };
        let value = if ext & 0x0800 != 0 {
            value
        } else {
            value as u16 as i16 as i32 as u32
        };
        value.wrapping_add(ext as u8 as i8 as i32 as u32)
    }

    /// (An)+ / -(An) em byte com A7 andam 2, para manter a pilha alinhada.
    fn step_size(&self, size: Size, reg: usize) -> u32 {
        if size == Size::Byte && reg == 7 {
            2
        } else {
            size.bytes()
        }
    }

    /// Lê o operando; um (An)+ pendente é aplicado aqui (uma vez só).
    pub(crate) fn read_loc<B: Bus>(&mut self, bus: &mut B, loc: &mut Loc, size: Size) -> Step<u32> {
        Ok(match *loc {
            Loc::D(r) => self.d[r] & size.mask(),
            Loc::A(r) => self.a[r] & size.mask(),
            Loc::Mem {
                addr,
                predec_long: Some(r),
                ..
            } if size == Size::Long => {
                let lo = self.predec_access(r, addr, |cpu| {
                    cpu.read_word(bus, addr.wrapping_add(2), false)
                })?;
                let hi = self.read_word(bus, addr, false)?;
                u32::from(hi) << 16 | u32::from(lo)
            }
            Loc::Mem { addr, .. } => {
                let v = self.read_mem(bus, size, addr, false)?;
                self.apply_post(loc);
                v
            }
            Loc::Imm(v) => v,
        })
    }

    fn apply_post(&mut self, loc: &mut Loc) {
        if let Loc::Mem { post, .. } = loc
            && let Some((r, n)) = post.take()
        {
            self.a[r] = self.a[r].wrapping_add(n);
        }
    }

    /// Primeiro acesso (palavra baixa) de um -(An) long: se falhar, An volta 2.
    fn predec_access<T>(
        &mut self,
        reg: usize,
        addr: u32,
        access: impl FnOnce(&mut Self) -> Step<T>,
    ) -> Step<T> {
        access(self).inspect_err(|_| self.a[reg] = addr.wrapping_add(2))
    }

    /// Escreve preservando os bits altos de Dn; em An grava 32 bits (quem
    /// chama estende o sinal quando necessário).
    pub(crate) fn write_loc<B: Bus>(
        &mut self,
        bus: &mut B,
        loc: &mut Loc,
        size: Size,
        value: u32,
    ) -> Step<()> {
        match *loc {
            Loc::D(r) => {
                let mask = size.mask();
                self.d[r] = (self.d[r] & !mask) | (value & mask);
            }
            Loc::A(r) => self.a[r] = value,
            Loc::Mem {
                addr,
                predec_long: Some(r),
                ..
            } if size == Size::Long => {
                self.predec_access(r, addr, |cpu| {
                    cpu.write_word(bus, addr.wrapping_add(2), value as u16)
                })?;
                self.write_word(bus, addr, (value >> 16) as u16)?;
            }
            Loc::Mem { addr, .. } => {
                self.write_mem(bus, size, addr, value)?;
                self.apply_post(loc);
            }
            // Imediato nunca é destino (as classes de modo impedem).
            Loc::Imm(_) => {}
        }
        Ok(())
    }
}
