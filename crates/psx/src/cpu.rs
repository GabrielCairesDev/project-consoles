//! CPU R3000A (MIPS I) do PlayStation, com o COP0 e a ligação com o GTE
//! (COP2).
//!
//! Modelo:
//! - **Delay slots:** a instrução depois de um desvio sempre executa (`pc` e
//!   `next_pc`). Um load só fica visível duas instruções depois: a seguinte
//!   ainda lê o valor antigo (`load` e `next_load`, como no hardware, em que
//!   a escrita da instrução seguinte no mesmo registrador vence).
//! - **Exceções:** pelo COP0 (SR, CAUSE, EPC, BadVaddr), com a pilha de
//!   modos de 3 níveis do SR e o vetor em 0x80000080 (ou 0xBFC00180 com BEV).
//! - **Tempo:** 1 ciclo por instrução, mais o custo de buscar a instrução
//!   (cache de instruções simulada só pelas tags, para o tempo; o código é
//!   sempre lido da memória) e de cada acesso de dados. MULT e DIV têm
//!   latência: MFHI/MFLO esperam o resultado.

use crate::bus::Bus;
use crate::gte::Gte;

const RESET_VECTOR: u32 = 0xBFC0_0000;
const SR_IEC: u32 = 1;
const SR_ISOLATE_CACHE: u32 = 1 << 16;
const SR_BEV: u32 = 1 << 22;
const CAUSE_BD: u32 = 1 << 31;

/// Códigos de exceção (campo ExcCode do CAUSE).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exception {
    Interrupt = 0x0,
    LoadAddress = 0x4,
    StoreAddress = 0x5,
    BusFetch = 0x6,
    Syscall = 0x8,
    Break = 0x9,
    ReservedInstruction = 0xA,
    CoprocessorUnusable = 0xB,
    Overflow = 0xC,
}

#[derive(Clone, Default)]
struct Cop0 {
    /// Registradores de breakpoint (3, 5, 6, 7, 9, 11): só guardados.
    bpc: u32,
    bda: u32,
    jumpdest: u32,
    dcic: u32,
    bdam: u32,
    bpcm: u32,
    bad_vaddr: u32,
    sr: u32,
    /// CAUSE sem o bit da interrupção de hardware (vem do barramento).
    cause: u32,
    epc: u32,
}

const PRID: u32 = 0x0000_0002;

#[derive(Clone)]
pub(crate) struct Cpu {
    regs: [u32; 32],
    hi: u32,
    lo: u32,
    /// Endereço da instrução a executar.
    pc: u32,
    /// Endereço da seguinte (o destino, depois de um desvio).
    next_pc: u32,
    /// Endereço da instrução em execução (para o EPC).
    current_pc: u32,
    /// A instrução em execução é um desvio: a próxima está no delay slot.
    branch: bool,
    /// A instrução em execução está num delay slot.
    delay_slot: bool,
    /// Load em andamento: aplicado ao fim da instrução atual.
    load: (usize, u32),
    /// Load feito pela instrução atual: aplicado ao fim da seguinte.
    next_load: (usize, u32),
    /// Ciclo em que o resultado de MULT/DIV fica pronto.
    muldiv_ready: u64,
    /// Ciclo em que o comando do GTE em andamento termina.
    gte_ready: u64,
    cop0: Cop0,
    pub(crate) gte: Gte,
    /// Tags do cache de instruções (4 KB, 256 linhas de 16 bytes): endereço
    /// da linha nos bits 31–12 e um bit de validade por palavra.
    icache: Box<[u32; 256]>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            regs: [0; 32],
            hi: 0,
            lo: 0,
            pc: RESET_VECTOR,
            next_pc: RESET_VECTOR.wrapping_add(4),
            current_pc: RESET_VECTOR,
            branch: false,
            delay_slot: false,
            load: (0, 0),
            next_load: (0, 0),
            muldiv_ready: 0,
            gte_ready: 0,
            cop0: Cop0::default(),
            gte: Gte::default(),
            icache: Box::new([0; 256]),
        }
    }
}

#[inline]
fn rs(i: u32) -> usize {
    ((i >> 21) & 31) as usize
}
#[inline]
fn rt(i: u32) -> usize {
    ((i >> 16) & 31) as usize
}
#[inline]
fn rd(i: u32) -> usize {
    ((i >> 11) & 31) as usize
}
#[inline]
fn shamt(i: u32) -> u32 {
    (i >> 6) & 31
}
#[inline]
fn imm(i: u32) -> u32 {
    i & 0xFFFF
}
/// Imediato com extensão de sinal.
#[inline]
fn simm(i: u32) -> u32 {
    i as u16 as i16 as i32 as u32
}

impl Cpu {
    /// Endereço da próxima instrução.
    pub(crate) fn pc(&self) -> u32 {
        self.pc
    }

    pub(crate) fn reg(&self, r: usize) -> u32 {
        self.regs[r]
    }

    /// Escreve um registrador fora da execução (carga de PS-EXE).
    pub(crate) fn set_register(&mut self, r: usize, value: u32) {
        if r != 0 {
            self.regs[r] = value;
        }
    }

    /// Desvia para `pc` fora da execução, descartando delay slot e loads.
    pub(crate) fn jump_to(&mut self, pc: u32) {
        self.pc = pc;
        self.next_pc = pc.wrapping_add(4);
        self.branch = false;
        self.load = (0, 0);
        self.next_load = (0, 0);
    }

    /// Executa uma instrução (ou entra numa interrupção pendente).
    pub(crate) fn step(&mut self, bus: &mut Bus) {
        self.current_pc = self.pc;
        self.delay_slot = self.branch;
        self.branch = false;

        if self.interrupt_pending(bus) {
            // O hardware executa uma instrução do GTE que já estava no
            // pipeline; o handler do BIOS conta com isso e pula a
            // instrução ao voltar.
            if self.pc & 3 == 0 {
                let next = bus.peek_code(self.pc);
                if next >> 25 == 0x25 {
                    self.gte_ready = bus.now + self.gte.command(next);
                    bus.gpu.remember_vertices(self.gte.projected());
                }
            }
            self.exception(Exception::Interrupt);
            self.finish_step();
            return;
        }

        if self.pc & 3 != 0 {
            self.cop0.bad_vaddr = self.pc;
            self.exception(Exception::LoadAddress);
            self.finish_step();
            return;
        }

        let Some(instruction) = self.fetch(bus) else {
            self.exception(Exception::BusFetch);
            self.finish_step();
            return;
        };
        self.pc = self.next_pc;
        self.next_pc = self.pc.wrapping_add(4);
        bus.now += 1;
        self.execute(bus, instruction);
        self.finish_step();
    }

    fn finish_step(&mut self) {
        let (r, value) = self.load;
        self.regs[r] = value;
        self.regs[0] = 0;
        self.load = self.next_load;
        self.next_load = (0, 0);
    }

    fn interrupt_pending(&self, bus: &Bus) -> bool {
        self.cop0.sr & SR_IEC != 0 && self.cop0.sr & self.cause(bus) & 0xFF00 != 0
    }

    fn cause(&self, bus: &Bus) -> u32 {
        self.cop0.cause | (u32::from(bus.irq.pending()) << 10)
    }

    fn fetch(&mut self, bus: &mut Bus) -> Option<u32> {
        let pc = self.pc;
        if pc < 0xA000_0000 && bus.icache_enabled() {
            let line = ((pc >> 4) & 0xFF) as usize;
            let word = (pc >> 2) & 3;
            let tag = pc & 0xFFFF_F000;
            let entry = self.icache[line];
            let same_line = entry & 0xFFFF_F000 == tag;
            if !same_line || entry & (1 << word) == 0 {
                // Preenche da palavra pedida até o fim da linha.
                let fill = (0xF << word) & 0xF;
                let valid = if same_line { entry & 0xF } else { 0 };
                self.icache[line] = tag | valid | fill;
                bus.now += bus.fetch_ticks(pc) + u64::from(3 - word);
            }
        } else {
            bus.now += bus.fetch_ticks(pc);
        }
        bus.fetch(pc)
    }

    fn set_reg(&mut self, r: usize, value: u32) {
        self.regs[r] = value;
        self.regs[0] = 0;
        // A escrita da própria instrução vence o load em andamento.
        if self.load.0 == r {
            self.load = (0, 0);
        }
    }

    fn delayed_load(&mut self, r: usize, value: u32) {
        if r == 0 {
            return;
        }
        if self.load.0 == r {
            self.load = (0, 0);
        }
        self.next_load = (r, value);
    }

    /// Valor de `r` visto por LWL/LWR, que se juntam ao load em andamento.
    fn load_bypass(&self, r: usize) -> u32 {
        if self.load.0 == r && r != 0 {
            self.load.1
        } else {
            self.regs[r]
        }
    }

    fn exception(&mut self, exception: Exception) {
        let handler = if self.cop0.sr & SR_BEV != 0 {
            0xBFC0_0180
        } else {
            0x8000_0080
        };
        let mode = self.cop0.sr & 0x3F;
        self.cop0.sr = (self.cop0.sr & !0x3F) | ((mode << 2) & 0x3F);
        self.cop0.cause = (self.cop0.cause & !(0x7C | CAUSE_BD)) | ((exception as u32) << 2);
        if self.delay_slot {
            self.cop0.epc = self.current_pc.wrapping_sub(4);
            self.cop0.cause |= CAUSE_BD;
        } else {
            self.cop0.epc = self.current_pc;
        }
        self.pc = handler;
        self.next_pc = handler.wrapping_add(4);
        self.branch = false;
        // A instrução que falhou não grava nada.
        self.next_load = (0, 0);
    }

    /// O coprocessador `cop` está habilitado no SR (o COP0 sempre está em
    /// modo kernel); senão gera a exceção "coprocessador indisponível".
    fn require_coprocessor(&mut self, cop: u32) -> bool {
        let kernel = cop == 0 && self.cop0.sr & 2 == 0;
        if kernel || self.cop0.sr & (1 << (28 + cop)) != 0 {
            return true;
        }
        self.coprocessor_unusable(cop);
        false
    }

    fn coprocessor_unusable(&mut self, cop: u32) {
        self.cop0.cause = (self.cop0.cause & !(3 << 28)) | (cop << 28);
        self.exception(Exception::CoprocessorUnusable);
    }

    fn execute(&mut self, bus: &mut Bus, i: u32) {
        match i >> 26 {
            0x00 => self.special(bus, i),
            0x01 => self.bcondz(i),
            0x02 => {
                self.branch = true;
                self.next_pc = (self.pc & 0xF000_0000) | ((i & 0x03FF_FFFF) << 2);
            }
            0x03 => {
                self.set_reg(31, self.next_pc);
                self.branch = true;
                self.next_pc = (self.pc & 0xF000_0000) | ((i & 0x03FF_FFFF) << 2);
            }
            0x04 => self.branch_if(self.regs[rs(i)] == self.regs[rt(i)], i),
            0x05 => self.branch_if(self.regs[rs(i)] != self.regs[rt(i)], i),
            0x06 => self.branch_if(self.regs[rs(i)] as i32 <= 0, i),
            0x07 => self.branch_if(self.regs[rs(i)] as i32 > 0, i),
            0x08 => match (self.regs[rs(i)] as i32).checked_add(simm(i) as i32) {
                Some(v) => self.set_reg(rt(i), v as u32),
                None => self.exception(Exception::Overflow),
            },
            0x09 => self.set_reg(rt(i), self.regs[rs(i)].wrapping_add(simm(i))),
            0x0A => self.set_reg(rt(i), u32::from((self.regs[rs(i)] as i32) < simm(i) as i32)),
            0x0B => self.set_reg(rt(i), u32::from(self.regs[rs(i)] < simm(i))),
            0x0C => self.set_reg(rt(i), self.regs[rs(i)] & imm(i)),
            0x0D => self.set_reg(rt(i), self.regs[rs(i)] | imm(i)),
            0x0E => self.set_reg(rt(i), self.regs[rs(i)] ^ imm(i)),
            0x0F => self.set_reg(rt(i), imm(i) << 16),
            0x10 => {
                if self.require_coprocessor(0) {
                    self.cop0_op(bus, i);
                }
            }
            0x12 => self.cop2_op(bus, i),
            // O PS1 não tem COP1 nem COP3: habilitados, viram NOP.
            0x11 | 0x13 => {
                self.require_coprocessor((i >> 26) & 3);
            }
            0x20 => {
                let addr = self.address(i);
                let v = bus.read8(addr) as u8 as i8 as i32 as u32;
                self.delayed_load(rt(i), v);
            }
            0x21 => {
                let addr = self.address(i);
                if self.check_load_alignment(addr, 1) {
                    let v = bus.read16(addr) as u16 as i16 as i32 as u32;
                    self.delayed_load(rt(i), v);
                }
            }
            0x22 => {
                let addr = self.address(i);
                let word = bus.read32(addr & !3);
                let current = self.load_bypass(rt(i));
                let v = match addr & 3 {
                    0 => (current & 0x00FF_FFFF) | (word << 24),
                    1 => (current & 0x0000_FFFF) | (word << 16),
                    2 => (current & 0x0000_00FF) | (word << 8),
                    _ => word,
                };
                self.delayed_load(rt(i), v);
            }
            0x23 => {
                let addr = self.address(i);
                if self.check_load_alignment(addr, 3) {
                    let v = bus.read32(addr);
                    self.delayed_load(rt(i), v);
                }
            }
            0x24 => {
                let addr = self.address(i);
                let v = bus.read8(addr) & 0xFF;
                self.delayed_load(rt(i), v);
            }
            0x25 => {
                let addr = self.address(i);
                if self.check_load_alignment(addr, 1) {
                    let v = bus.read16(addr) & 0xFFFF;
                    self.delayed_load(rt(i), v);
                }
            }
            0x26 => {
                let addr = self.address(i);
                let word = bus.read32(addr & !3);
                let current = self.load_bypass(rt(i));
                let v = match addr & 3 {
                    0 => word,
                    1 => (current & 0xFF00_0000) | (word >> 8),
                    2 => (current & 0xFFFF_0000) | (word >> 16),
                    _ => (current & 0xFFFF_FF00) | (word >> 24),
                };
                self.delayed_load(rt(i), v);
            }
            0x28 => {
                let addr = self.address(i);
                self.store(bus, addr, self.regs[rt(i)], 1);
            }
            0x29 => {
                let addr = self.address(i);
                if self.check_store_alignment(addr, 1) {
                    self.store(bus, addr, self.regs[rt(i)], 2);
                }
            }
            0x2A => {
                let addr = self.address(i);
                let aligned = addr & !3;
                let memory = bus.peek32(aligned);
                let value = self.regs[rt(i)];
                let v = match addr & 3 {
                    0 => (memory & 0xFFFF_FF00) | (value >> 24),
                    1 => (memory & 0xFFFF_0000) | (value >> 16),
                    2 => (memory & 0xFF00_0000) | (value >> 8),
                    _ => value,
                };
                self.store(bus, aligned, v, 4);
            }
            0x2B => {
                let addr = self.address(i);
                if self.check_store_alignment(addr, 3) {
                    self.store(bus, addr, self.regs[rt(i)], 4);
                }
            }
            0x2E => {
                let addr = self.address(i);
                let aligned = addr & !3;
                let memory = bus.peek32(aligned);
                let value = self.regs[rt(i)];
                let v = match addr & 3 {
                    0 => value,
                    1 => (memory & 0x0000_00FF) | (value << 8),
                    2 => (memory & 0x0000_FFFF) | (value << 16),
                    _ => (memory & 0x00FF_FFFF) | (value << 24),
                };
                self.store(bus, aligned, v, 4);
            }
            0x32 => {
                if !self.require_coprocessor(2) {
                    return;
                }
                let addr = self.address(i);
                if self.check_load_alignment(addr, 3) {
                    let v = bus.read32(addr);
                    self.gte.write_data(rt(i), v);
                }
            }
            0x3A => {
                if !self.require_coprocessor(2) {
                    return;
                }
                let addr = self.address(i);
                if self.check_store_alignment(addr, 3) {
                    self.wait_gte(bus);
                    let v = self.gte.read_data(rt(i));
                    self.store(bus, addr, v, 4);
                }
            }
            // LWCn/SWCn sem coprocessor que os aceite: NOP se habilitado.
            // Para o COP0 vale o bit do SR mesmo em modo kernel.
            op @ (0x30 | 0x31 | 0x33 | 0x38 | 0x39 | 0x3B) => {
                let cop = op & 3;
                if self.cop0.sr & (1 << (28 + cop)) == 0 {
                    self.coprocessor_unusable(cop);
                }
            }
            _ => self.exception(Exception::ReservedInstruction),
        }
    }

    fn special(&mut self, bus: &mut Bus, i: u32) {
        let (s, t, d) = (rs(i), rt(i), rd(i));
        let (a, b) = (self.regs[s], self.regs[t]);
        match i & 0x3F {
            0x00 => self.set_reg(d, b << shamt(i)),
            0x02 => self.set_reg(d, b >> shamt(i)),
            0x03 => self.set_reg(d, ((b as i32) >> shamt(i)) as u32),
            0x04 => self.set_reg(d, b << (a & 31)),
            0x06 => self.set_reg(d, b >> (a & 31)),
            0x07 => self.set_reg(d, ((b as i32) >> (a & 31)) as u32),
            0x08 => {
                self.branch = true;
                self.next_pc = a;
            }
            0x09 => {
                self.set_reg(d, self.next_pc);
                self.branch = true;
                self.next_pc = a;
            }
            0x0C => self.exception(Exception::Syscall),
            0x0D => self.exception(Exception::Break),
            0x10 => {
                self.wait_muldiv(bus);
                self.set_reg(d, self.hi);
            }
            0x11 => self.hi = a,
            0x12 => {
                self.wait_muldiv(bus);
                self.set_reg(d, self.lo);
            }
            0x13 => self.lo = a,
            0x18 => {
                let product = i64::from(a as i32) * i64::from(b as i32);
                self.hi = (product >> 32) as u32;
                self.lo = product as u32;
                let magnitude = if (a as i32) < 0 { !a } else { a };
                self.muldiv_ready = bus.now + mult_latency(magnitude);
            }
            0x19 => {
                let product = u64::from(a) * u64::from(b);
                self.hi = (product >> 32) as u32;
                self.lo = product as u32;
                self.muldiv_ready = bus.now + mult_latency(a);
            }
            0x1A => {
                let (n, m) = (a as i32, b as i32);
                if m == 0 {
                    self.hi = a;
                    self.lo = if n >= 0 { 0xFFFF_FFFF } else { 1 };
                } else if n == i32::MIN && m == -1 {
                    self.hi = 0;
                    self.lo = 0x8000_0000;
                } else {
                    self.hi = (n % m) as u32;
                    self.lo = (n / m) as u32;
                }
                self.muldiv_ready = bus.now + DIV_LATENCY;
            }
            0x1B => {
                if b == 0 {
                    self.hi = a;
                    self.lo = 0xFFFF_FFFF;
                } else {
                    self.hi = a % b;
                    self.lo = a / b;
                }
                self.muldiv_ready = bus.now + DIV_LATENCY;
            }
            0x20 => match (a as i32).checked_add(b as i32) {
                Some(v) => self.set_reg(d, v as u32),
                None => self.exception(Exception::Overflow),
            },
            0x21 => self.set_reg(d, a.wrapping_add(b)),
            0x22 => match (a as i32).checked_sub(b as i32) {
                Some(v) => self.set_reg(d, v as u32),
                None => self.exception(Exception::Overflow),
            },
            0x23 => self.set_reg(d, a.wrapping_sub(b)),
            0x24 => self.set_reg(d, a & b),
            0x25 => self.set_reg(d, a | b),
            0x26 => self.set_reg(d, a ^ b),
            0x27 => self.set_reg(d, !(a | b)),
            0x2A => self.set_reg(d, u32::from((a as i32) < (b as i32))),
            0x2B => self.set_reg(d, u32::from(a < b)),
            _ => self.exception(Exception::ReservedInstruction),
        }
    }

    /// BLTZ, BGEZ, BLTZAL e BGEZAL. Como no hardware, só os bits 0 e 4 de
    /// `rt` contam: o bit 0 escolhe "≥ 0", e o link vale quando os bits
    /// 4–1 são 1000 (os outros códigos viram BLTZ/BGEZ sem link).
    fn bcondz(&mut self, i: u32) {
        let code = rt(i);
        let value = self.regs[rs(i)] as i32;
        let taken = if code & 1 != 0 { value >= 0 } else { value < 0 };
        if code & 0x1E == 0x10 {
            self.set_reg(31, self.next_pc);
        }
        self.branch_if(taken, i);
    }

    fn branch_if(&mut self, taken: bool, i: u32) {
        self.branch = true;
        if taken {
            // `pc` já aponta para o delay slot.
            self.next_pc = self.pc.wrapping_add(simm(i) << 2);
        }
    }

    fn address(&self, i: u32) -> u32 {
        self.regs[rs(i)].wrapping_add(simm(i))
    }

    fn check_load_alignment(&mut self, addr: u32, mask: u32) -> bool {
        if addr & mask != 0 {
            self.cop0.bad_vaddr = addr;
            self.exception(Exception::LoadAddress);
            return false;
        }
        true
    }

    fn check_store_alignment(&mut self, addr: u32, mask: u32) -> bool {
        if addr & mask != 0 {
            self.cop0.bad_vaddr = addr;
            self.exception(Exception::StoreAddress);
            return false;
        }
        true
    }

    fn store(&mut self, bus: &mut Bus, addr: u32, value: u32, width: u32) {
        if self.cop0.sr & SR_ISOLATE_CACHE != 0 {
            // Cache isolado: a escrita vai para o cache, não para a memória.
            // O BIOS usa isso para invalidar o cache de instruções.
            self.icache[((addr >> 4) & 0xFF) as usize] = 0;
            return;
        }
        match width {
            1 => bus.write8(addr, value),
            2 => bus.write16(addr, value),
            _ => bus.write32(addr, value),
        }
    }

    fn wait_gte(&self, bus: &mut Bus) {
        if bus.now < self.gte_ready {
            bus.now = self.gte_ready;
        }
    }

    fn wait_muldiv(&self, bus: &mut Bus) {
        if bus.now < self.muldiv_ready {
            bus.now = self.muldiv_ready;
        }
    }

    fn cop0_op(&mut self, bus: &Bus, i: u32) {
        match rs(i) {
            0x00 => {
                let c = &self.cop0;
                let v = match rd(i) {
                    3 => c.bpc,
                    5 => c.bda,
                    6 => c.jumpdest,
                    7 => c.dcic,
                    8 => c.bad_vaddr,
                    9 => c.bdam,
                    11 => c.bpcm,
                    12 => c.sr,
                    13 => self.cause(bus),
                    14 => c.epc,
                    15 => PRID,
                    _ => 0,
                };
                self.delayed_load(rt(i), v);
            }
            0x04 => {
                let v = self.regs[rt(i)];
                let c = &mut self.cop0;
                match rd(i) {
                    3 => c.bpc = v,
                    5 => c.bda = v,
                    6 => c.jumpdest = v,
                    7 => c.dcic = v,
                    9 => c.bdam = v,
                    11 => c.bpcm = v,
                    12 => c.sr = v,
                    // Só as interrupções por software (bits 8 e 9).
                    13 => c.cause = (c.cause & !0x300) | (v & 0x300),
                    _ => {}
                }
            }
            0x10..=0x1F if i & 0x3F == 0x10 => {
                let sr = self.cop0.sr;
                self.cop0.sr = (sr & !0xF) | ((sr >> 2) & 0xF);
            }
            // Outras operações do COP0 não fazem nada (sem exceção, como no
            // hardware).
            _ => {}
        }
    }

    fn cop2_op(&mut self, bus: &mut Bus, i: u32) {
        if !self.require_coprocessor(2) {
            return;
        }
        // O GTE executa um comando por vez; ler resultados também espera.
        self.wait_gte(bus);
        if i & (1 << 25) != 0 {
            self.gte_ready = bus.now + self.gte.command(i);
            bus.gpu.remember_vertices(self.gte.projected());
            return;
        }
        match rs(i) {
            0x00 => {
                let v = self.gte.read_data(rd(i));
                self.delayed_load(rt(i), v);
            }
            0x02 => {
                let v = self.gte.read_control(rd(i));
                self.delayed_load(rt(i), v);
            }
            0x04 => self.gte.write_data(rd(i), self.regs[rt(i)]),
            0x06 => self.gte.write_control(rd(i), self.regs[rt(i)]),
            _ => {}
        }
    }
}

const DIV_LATENCY: u64 = 36;

/// Latência do MULT pelo tamanho do operando (`rs`, em magnitude).
fn mult_latency(magnitude: u32) -> u64 {
    if magnitude < 0x800 {
        6
    } else if magnitude < 0x10_0000 {
        9
    } else {
        13
    }
}

#[cfg(test)]
mod tests;
