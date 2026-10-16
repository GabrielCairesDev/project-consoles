//! Interpretador do Motorola 68000.
//!
//! Modela a fila de prefetch real (IR + IRC): `pc` aponta para a palavra que
//! está em IRC, e cada palavra de extensão consumida dispara a busca da
//! seguinte, como no hardware. Isso deixa o estado da fila — e o tempo —
//! iguais aos do chip, o que a suíte SingleStepTests confere.
//!
//! Tempo: cada acesso de 16 bits ao barramento custa 4 ciclos e é somado
//! automaticamente; as instruções só acrescentam os ciclos internos
//! (`idle`). Erros de endereço (acesso de palavra em endereço ímpar)
//! interrompem a instrução via `Result` e viram a exceção do grupo 0.

mod arith;
mod ea;
mod exec;
#[cfg(test)]
mod tests;

use chips::state::{Reader, StateError, Writer};

/// Barramento visto pelo 68000 (endereços de 24 bits).
pub trait Bus {
    fn read_byte(&mut self, addr: u32) -> u8;
    /// `addr` é sempre par.
    fn read_word(&mut self, addr: u32) -> u16;
    fn write_byte(&mut self, addr: u32, value: u8);
    /// `addr` é sempre par.
    fn write_word(&mut self, addr: u32, value: u16);
    /// Nível de interrupção pedido: 0 = nenhum, 1–7.
    fn interrupt_level(&self) -> u8 {
        0
    }
    /// A CPU aceitou a interrupção `level` (o dispositivo pode baixar o pedido).
    fn acknowledge_interrupt(&mut self, _level: u8) {}
    /// Sinal RESET emitido pela instrução RESET.
    fn reset_devices(&mut self) {}
}

/// Bits do registrador de status.
pub mod sr {
    pub const T: u16 = 0x8000;
    pub const S: u16 = 0x2000;
    pub const INT_MASK: u16 = 0x0700;
    pub const X: u16 = 0x10;
    pub const N: u16 = 0x08;
    pub const Z: u16 = 0x04;
    pub const V: u16 = 0x02;
    pub const C: u16 = 0x01;
    /// Bits que existem no 68000; os demais são sempre lidos como 0.
    pub const IMPLEMENTED: u16 = 0xA71F;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    pub(crate) fn mask(self) -> u32 {
        match self {
            Size::Byte => 0xFF,
            Size::Word => 0xFFFF,
            Size::Long => 0xFFFF_FFFF,
        }
    }

    pub(crate) fn msb(self) -> u32 {
        match self {
            Size::Byte => 0x80,
            Size::Word => 0x8000,
            Size::Long => 0x8000_0000,
        }
    }

    pub(crate) fn bytes(self) -> u32 {
        match self {
            Size::Byte => 1,
            Size::Word => 2,
            Size::Long => 4,
        }
    }

    /// Campo de tamanho padrão (bits 7–6): 00 byte, 01 word, 10 long.
    pub(crate) fn from_bits(bits: u16) -> Option<Size> {
        match bits & 3 {
            0 => Some(Size::Byte),
            1 => Some(Size::Word),
            2 => Some(Size::Long),
            _ => None,
        }
    }
}

/// Acesso de palavra em endereço ímpar: aborta a instrução.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AddressError {
    pub addr: u32,
    pub write: bool,
    /// Busca de instrução/espaço de programa (bit I/N e código de função).
    pub program: bool,
    /// PC a empilhar quando difere do padrão (falha ao desviar).
    pub pc: Option<u32>,
}

pub(crate) type Step<T> = Result<T, AddressError>;

/// Registradores visíveis, para testes e depuração.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Registers {
    pub d: [u32; 8],
    pub a: [u32; 7],
    pub usp: u32,
    pub ssp: u32,
    pub sr: u16,
    /// Endereço da próxima instrução (a que está em `prefetch[0]`).
    pub pc: u32,
    /// Fila de prefetch: IR (próximo opcode) e IRC (palavra seguinte).
    pub prefetch: [u16; 2],
}

#[derive(Debug, Clone)]
pub struct Cpu {
    pub(crate) d: [u32; 8],
    /// `a[7]` é o ponteiro de pilha ativo (USP ou SSP conforme o modo).
    pub(crate) a: [u32; 8],
    /// O ponteiro de pilha do outro modo.
    pub(crate) inactive_sp: u32,
    pub(crate) sr: u16,
    /// Endereço da palavra em IRC.
    pub(crate) pc: u32,
    pub(crate) ir: u16,
    pub(crate) irc: u16,
    /// Ciclos da instrução em andamento.
    pub(crate) cycles: u32,
    /// Endereço e opcode da instrução em andamento (o quadro de erro de
    /// endereço usa o opcode original, mesmo depois de um prefetch).
    pub(crate) instr_pc: u32,
    opcode: u16,
    stopped: bool,
    /// Falha dupla (erro de endereço durante uma exceção): CPU parada.
    halted: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            d: [0; 8],
            a: [0; 8],
            inactive_sp: 0,
            sr: sr::S | sr::INT_MASK,
            pc: 0,
            ir: 0,
            irc: 0,
            cycles: 0,
            instr_pc: 0,
            opcode: 0,
            stopped: false,
            halted: false,
        }
    }

    /// Reset do hardware: SSP e PC lidos dos vetores 0 e 1.
    pub fn reset<B: Bus>(&mut self, bus: &mut B) {
        *self = Self::new();
        let ssp = u32::from(bus.read_word(0)) << 16 | u32::from(bus.read_word(2));
        let pc = u32::from(bus.read_word(4)) << 16 | u32::from(bus.read_word(6));
        self.a[7] = ssp;
        if self.jump(bus, pc).is_err() {
            self.halted = true;
        }
    }

    pub fn registers(&self) -> Registers {
        let (usp, ssp) = if self.supervisor() {
            (self.inactive_sp, self.a[7])
        } else {
            (self.a[7], self.inactive_sp)
        };
        let mut a = [0; 7];
        a.copy_from_slice(&self.a[..7]);
        Registers {
            d: self.d,
            a,
            usp,
            ssp,
            sr: self.sr,
            pc: self.pc.wrapping_sub(2),
            prefetch: [self.ir, self.irc],
        }
    }

    pub fn set_registers(&mut self, r: &Registers) {
        self.d = r.d;
        self.a[..7].copy_from_slice(&r.a);
        self.sr = r.sr & sr::IMPLEMENTED;
        if self.supervisor() {
            self.a[7] = r.ssp;
            self.inactive_sp = r.usp;
        } else {
            self.a[7] = r.usp;
            self.inactive_sp = r.ssp;
        }
        self.pc = r.pc.wrapping_add(2);
        self.ir = r.prefetch[0];
        self.irc = r.prefetch[1];
        self.stopped = false;
        self.halted = false;
    }

    pub fn halted(&self) -> bool {
        self.halted
    }

    /// Executa uma instrução (ou atende uma interrupção) e devolve os ciclos.
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u32 {
        self.cycles = 0;
        if self.halted {
            return 4;
        }
        let level = bus.interrupt_level();
        // O nível 7 deveria ser por borda (NMI); o Mega Drive não o usa.
        if level > self.interrupt_mask() {
            self.stopped = false;
            self.interrupt(bus, level);
            return self.cycles;
        }
        if self.stopped {
            return 4;
        }
        self.instr_pc = self.pc.wrapping_sub(2);
        self.opcode = self.ir;
        let trace = self.sr & sr::T != 0;
        match self.execute(bus) {
            Ok(()) => {
                if trace {
                    let pc = self.pc.wrapping_sub(2);
                    self.exception(bus, 9, pc, 4);
                }
            }
            Err(fault) => self.address_error(bus, fault),
        }
        self.cycles
    }

    // ---- Estado ---------------------------------------------------------

    pub(crate) fn supervisor(&self) -> bool {
        self.sr & sr::S != 0
    }

    fn interrupt_mask(&self) -> u8 {
        ((self.sr & sr::INT_MASK) >> 8) as u8
    }

    /// Grava SR trocando a pilha ativa se o modo mudar.
    pub(crate) fn set_sr(&mut self, value: u16) {
        let value = value & sr::IMPLEMENTED;
        if (value ^ self.sr) & sr::S != 0 {
            std::mem::swap(&mut self.a[7], &mut self.inactive_sp);
        }
        self.sr = value;
    }

    pub(crate) fn set_ccr(&mut self, value: u16) {
        self.sr = (self.sr & 0xFF00) | (value & 0x1F);
    }

    pub(crate) fn flag(&self, bit: u16) -> bool {
        self.sr & bit != 0
    }

    pub(crate) fn set_flag(&mut self, bit: u16, on: bool) {
        if on {
            self.sr |= bit;
        } else {
            self.sr &= !bit;
        }
    }

    pub(crate) fn idle(&mut self, cycles: u32) {
        self.cycles += cycles;
    }

    // ---- Barramento -----------------------------------------------------

    pub(crate) fn read_byte<B: Bus>(&mut self, bus: &mut B, addr: u32) -> u8 {
        self.cycles += 4;
        bus.read_byte(addr & 0x00FF_FFFF)
    }

    pub(crate) fn read_word<B: Bus>(&mut self, bus: &mut B, addr: u32, program: bool) -> Step<u16> {
        if addr & 1 != 0 {
            return Err(AddressError {
                addr,
                write: false,
                program,
                pc: None,
            });
        }
        self.cycles += 4;
        Ok(bus.read_word(addr & 0x00FF_FFFF))
    }

    pub(crate) fn write_byte<B: Bus>(&mut self, bus: &mut B, addr: u32, value: u8) {
        self.cycles += 4;
        bus.write_byte(addr & 0x00FF_FFFF, value);
    }

    pub(crate) fn write_word<B: Bus>(&mut self, bus: &mut B, addr: u32, value: u16) -> Step<()> {
        if addr & 1 != 0 {
            return Err(AddressError {
                addr,
                write: true,
                program: false,
                pc: None,
            });
        }
        self.cycles += 4;
        bus.write_word(addr & 0x00FF_FFFF, value);
        Ok(())
    }

    pub(crate) fn read_mem<B: Bus>(
        &mut self,
        bus: &mut B,
        size: Size,
        addr: u32,
        program: bool,
    ) -> Step<u32> {
        match size {
            Size::Byte => Ok(u32::from(self.read_byte(bus, addr))),
            Size::Word => Ok(u32::from(self.read_word(bus, addr, program)?)),
            Size::Long => {
                let hi = self.read_word(bus, addr, program)?;
                let lo = self.read_word(bus, addr.wrapping_add(2), program)?;
                Ok(u32::from(hi) << 16 | u32::from(lo))
            }
        }
    }

    pub(crate) fn write_mem<B: Bus>(
        &mut self,
        bus: &mut B,
        size: Size,
        addr: u32,
        value: u32,
    ) -> Step<()> {
        match size {
            Size::Byte => {
                self.write_byte(bus, addr, value as u8);
                Ok(())
            }
            Size::Word => self.write_word(bus, addr, value as u16),
            Size::Long => {
                self.write_word(bus, addr, (value >> 16) as u16)?;
                self.write_word(bus, addr.wrapping_add(2), value as u16)
            }
        }
    }

    /// Consome a palavra de extensão em IRC e busca a seguinte.
    pub(crate) fn fetch_ext<B: Bus>(&mut self, bus: &mut B) -> Step<u16> {
        let value = self.irc;
        self.pc = self.pc.wrapping_add(2);
        self.irc = self.read_word(bus, self.pc, true)?;
        Ok(value)
    }

    pub(crate) fn fetch_ext_long<B: Bus>(&mut self, bus: &mut B) -> Step<u32> {
        let hi = self.fetch_ext(bus)?;
        let lo = self.fetch_ext(bus)?;
        Ok(u32::from(hi) << 16 | u32::from(lo))
    }

    /// Consome IRC sem buscar a próxima (antes de um desvio que esvazia a fila).
    pub(crate) fn take_ext(&mut self) -> u16 {
        self.pc = self.pc.wrapping_add(2);
        self.irc
    }

    /// Fim de instrução: a palavra em IRC vira o próximo opcode.
    pub(crate) fn prefetch<B: Bus>(&mut self, bus: &mut B) -> Step<()> {
        self.ir = self.irc;
        self.pc = self.pc.wrapping_add(2);
        self.irc = self.read_word(bus, self.pc, true)?;
        Ok(())
    }

    /// Desvio: recarrega a fila a partir de `target`.
    pub(crate) fn jump<B: Bus>(&mut self, bus: &mut B, target: u32) -> Step<()> {
        self.ir = self
            .read_word(bus, target, true)
            .map_err(|e| AddressError {
                pc: Some(target.wrapping_sub(4)),
                ..e
            })?;
        self.irc = self.read_word(bus, target.wrapping_add(2), true)?;
        self.pc = target.wrapping_add(2);
        Ok(())
    }

    pub(crate) fn push_long<B: Bus>(&mut self, bus: &mut B, value: u32) -> Step<()> {
        let sp = self.a[7].wrapping_sub(4);
        self.a[7] = sp;
        self.write_word(bus, sp.wrapping_add(2), value as u16)?;
        self.write_word(bus, sp, (value >> 16) as u16)
    }

    pub(crate) fn push_word<B: Bus>(&mut self, bus: &mut B, value: u16) -> Step<()> {
        let sp = self.a[7].wrapping_sub(2);
        self.a[7] = sp;
        self.write_word(bus, sp, value)
    }

    pub(crate) fn pop_long<B: Bus>(&mut self, bus: &mut B) -> Step<u32> {
        let v = self.read_mem(bus, Size::Long, self.a[7], false)?;
        self.a[7] = self.a[7].wrapping_add(4);
        Ok(v)
    }

    pub(crate) fn pop_word<B: Bus>(&mut self, bus: &mut B) -> Step<u16> {
        let v = self.read_word(bus, self.a[7], false)?;
        self.a[7] = self.a[7].wrapping_add(2);
        Ok(v)
    }

    // ---- Exceções -------------------------------------------------------

    /// Exceções dos grupos 1 e 2: empilha PC e SR e salta para o vetor.
    pub(crate) fn exception<B: Bus>(
        &mut self,
        bus: &mut B,
        vector: u32,
        return_pc: u32,
        idle: u32,
    ) {
        let old_sr = self.sr;
        self.set_sr((self.sr | sr::S) & !sr::T);
        self.idle(idle);
        let result = (|| {
            self.push_long(bus, return_pc)?;
            self.push_word(bus, old_sr)?;
            let target = self.read_mem(bus, Size::Long, vector * 4, false)?;
            self.jump(bus, target)
        })();
        if result.is_err() {
            self.halted = true;
        }
    }

    pub(crate) fn raise<B: Bus>(
        &mut self,
        bus: &mut B,
        vector: u32,
        return_pc: u32,
        idle: u32,
    ) -> Step<()> {
        self.exception(bus, vector, return_pc, idle);
        Ok(())
    }

    fn interrupt<B: Bus>(&mut self, bus: &mut B, level: u8) {
        let return_pc = self.pc.wrapping_sub(2);
        let old_sr = self.sr;
        self.set_sr(((self.sr | sr::S) & !(sr::T | sr::INT_MASK)) | (u16::from(level) << 8));
        bus.acknowledge_interrupt(level);
        self.idle(16);
        let vector = 24 + u32::from(level);
        let result = (|| {
            self.push_long(bus, return_pc)?;
            self.push_word(bus, old_sr)?;
            let target = self.read_mem(bus, Size::Long, vector * 4, false)?;
            self.jump(bus, target)
        })();
        if result.is_err() {
            self.halted = true;
        }
    }

    /// Exceção do grupo 0: quadro de 7 palavras com o acesso que falhou.
    fn address_error<B: Bus>(&mut self, bus: &mut B, fault: AddressError) {
        let old_sr = self.sr;
        self.set_sr((self.sr | sr::S) & !sr::T);
        let function_code = match (old_sr & sr::S != 0, fault.program) {
            (false, false) => 1,
            (false, true) => 2,
            (true, false) => 5,
            (true, true) => 6,
        };
        let status = (self.opcode & 0xFFE0)
            | if fault.write { 0 } else { 0x10 }
            | if fault.program { 0x08 } else { 0 }
            | function_code;
        let return_pc = fault.pc.unwrap_or(self.pc.wrapping_sub(2));
        self.idle(6);
        let result = (|| {
            self.push_long(bus, return_pc)?;
            self.push_word(bus, old_sr)?;
            self.push_word(bus, self.opcode)?;
            self.push_long(bus, fault.addr)?;
            self.push_word(bus, status)?;
            let target = self.read_mem(bus, Size::Long, 3 * 4, false)?;
            self.jump(bus, target)
        })();
        if result.is_err() {
            self.halted = true;
        }
    }

    // ---- Estado salvo ---------------------------------------------------

    pub fn save_state(&self, w: &mut Writer) {
        for v in self.d.iter().chain(self.a.iter()) {
            w.u32(*v);
        }
        w.u32(self.inactive_sp);
        w.u16(self.sr);
        w.u32(self.pc);
        w.u16(self.ir);
        w.u16(self.irc);
        w.bool(self.stopped);
        w.bool(self.halted);
    }

    pub fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        for v in self.d.iter_mut().chain(self.a.iter_mut()) {
            *v = r.u32()?;
        }
        self.inactive_sp = r.u32()?;
        self.sr = r.u16()? & sr::IMPLEMENTED;
        self.pc = r.u32()?;
        self.ir = r.u16()?;
        self.irc = r.u16()?;
        self.stopped = r.bool()?;
        self.halted = r.bool()?;
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        self.stopped = true;
    }
}
