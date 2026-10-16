//! Barramento do Master System: mapa de memória, mapper Sega e portas.

use crate::vdp::Vdp;
use crate::z80::Bus;
use crate::{CYCLES_PER_LINE, Joypad};
use chips::psg::Psg;
use chips::state::{Reader, StateError, Writer};

pub(crate) const BANK_SIZE: usize = 0x4000;
const RAM_SIZE: usize = 0x2000;
const CART_RAM_SIZE: usize = 0x8000;
/// O primeiro 1 KB da ROM é fixo, para os vetores de interrupção.
const FIXED_ROM: u16 = 0x0400;

#[derive(Debug, Clone)]
pub(crate) struct SmsBus {
    rom: Vec<u8>,
    bank_count: usize,
    ram: Box<[u8; RAM_SIZE]>,
    cart_ram: Box<[u8; CART_RAM_SIZE]>,
    /// O jogo já gravou na RAM do cartucho (ou ela veio de um save). O
    /// cabeçalho do Master System não diz se o cartucho tem bateria; usar a
    /// RAM é o sinal de que há save a guardar.
    cart_ram_used: bool,
    /// Registradores do mapper em 0xFFFC–0xFFFF.
    mapper: [u8; 4],
    io_control: u8,
    pub vdp: Vdp,
    pub psg: Psg,
    pub joypad: Joypad,
    /// Ciclos já executados na linha atual (para o contador H).
    pub line_cycles: u32,
}

impl SmsBus {
    /// `rom` já deve ter tamanho múltiplo de [`BANK_SIZE`] e não ser vazia.
    pub fn new(rom: Vec<u8>, psg: Psg) -> Self {
        let bank_count = rom.len() / BANK_SIZE;
        Self {
            rom,
            bank_count,
            ram: Box::new([0; RAM_SIZE]),
            cart_ram: Box::new([0; CART_RAM_SIZE]),
            cart_ram_used: false,
            mapper: [0, 0, 1, 2],
            io_control: 0xFF,
            vdp: Vdp::new(),
            psg,
            joypad: Joypad::default(),
            line_cycles: 0,
        }
    }

    fn rom_byte(&self, bank_reg: u8, offset: u16) -> u8 {
        let bank = usize::from(bank_reg) % self.bank_count;
        self.rom[bank * BANK_SIZE + usize::from(offset)]
    }

    fn cart_ram_enabled(&self) -> bool {
        self.mapper[0] & 0x08 != 0
    }

    fn cart_ram_index(&self, addr: u16) -> usize {
        let bank = usize::from((self.mapper[0] >> 2) & 1);
        bank * BANK_SIZE + usize::from(addr & 0x3FFF)
    }

    /// Porta A (0xDC): direcional e botões do jogador 1, ativos em 0.
    fn port_a(&self) -> u8 {
        let p = &self.joypad;
        let mut v = 0xFF;
        for (pressed, bit) in [
            (p.up, 0),
            (p.down, 1),
            (p.left, 2),
            (p.right, 3),
            (p.button1, 4),
            (p.button2, 5),
        ] {
            if pressed {
                v &= !(1 << bit);
            }
        }
        v
    }

    /// Porta B (0xDD): jogador 2 (ausente), reset e linhas TH.
    ///
    /// Num console de exportação, TH configurado como saída é lido de volta
    /// com o nível escrito — é assim que os jogos detectam a região.
    fn port_b(&self) -> u8 {
        let mut v = 0x3F;
        let th_a = if self.io_control & 0x02 == 0 {
            self.io_control & 0x20 != 0
        } else {
            true
        };
        let th_b = if self.io_control & 0x08 == 0 {
            self.io_control & 0x80 != 0
        } else {
            true
        };
        if th_a {
            v |= 0x40;
        }
        if th_b {
            v |= 0x80;
        }
        v
    }

    /// Contador H aproximado: 342 pixels por linha, lido em unidades de 2.
    fn h_counter(&self) -> u8 {
        let pixel = self.line_cycles.min(CYCLES_PER_LINE - 1) * 342 / CYCLES_PER_LINE;
        (pixel / 2) as u8
    }
}

impl Bus for SmsBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..FIXED_ROM => self.rom[usize::from(addr)],
            FIXED_ROM..=0x3FFF => self.rom_byte(self.mapper[1], addr),
            0x4000..=0x7FFF => self.rom_byte(self.mapper[2], addr - 0x4000),
            0x8000..=0xBFFF => {
                if self.cart_ram_enabled() {
                    self.cart_ram[self.cart_ram_index(addr)]
                } else {
                    self.rom_byte(self.mapper[3], addr - 0x8000)
                }
            }
            _ => self.ram[usize::from(addr) % RAM_SIZE],
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0xBFFF if self.cart_ram_enabled() => {
                let i = self.cart_ram_index(addr);
                self.cart_ram[i] = value;
                self.cart_ram_used = true;
            }
            0xC000..=0xFFFF => {
                self.ram[usize::from(addr) % RAM_SIZE] = value;
                if addr >= 0xFFFC {
                    self.mapper[usize::from(addr - 0xFFFC)] = value;
                }
            }
            // ROM: escritas são ignoradas.
            _ => {}
        }
    }

    fn input(&mut self, port: u16) -> u8 {
        let port = port as u8;
        match port & 0xC1 {
            0x40 => self.vdp.v_counter(),
            0x41 => self.h_counter(),
            0x80 => self.vdp.read_data(),
            0x81 => self.vdp.read_status(),
            0xC0 => self.port_a(),
            0xC1 => self.port_b(),
            // 0x00–0x3F: barramento aberto.
            _ => 0xFF,
        }
    }

    fn output(&mut self, port: u16, value: u8) {
        let port = port as u8;
        match port & 0xC1 {
            // 0x3E (controle de memória/BIOS) não se aplica: não há BIOS.
            0x01 => self.io_control = value,
            0x40 | 0x41 => self.psg.write(value),
            0x80 => self.vdp.write_data(value),
            0x81 => self.vdp.write_control(value),
            _ => {}
        }
    }

    fn int_line(&self) -> bool {
        self.vdp.irq()
    }
}

impl SmsBus {
    /// RAM do cartucho, depois que o jogo a usou.
    pub(crate) fn cart_ram(&self) -> Option<&[u8]> {
        self.cart_ram_used.then_some(self.cart_ram.as_slice())
    }

    /// Carrega a RAM do cartucho a partir de um save. Aceita 8, 16 ou 32 KB
    /// (outros emuladores gravam só os bancos usados); o resto fica zerado.
    pub(crate) fn load_cart_ram(&mut self, bytes: &[u8]) -> bool {
        if !matches!(bytes.len(), 0x2000 | 0x4000 | CART_RAM_SIZE) {
            return false;
        }
        let (head, tail) = self.cart_ram.split_at_mut(bytes.len());
        head.copy_from_slice(bytes);
        tail.fill(0);
        self.cart_ram_used = true;
        true
    }

    /// A ROM não entra no estado: quem carrega já tem a mesma ROM (o hash é
    /// conferido em [`crate::Console::load_state`]).
    pub(crate) fn save_state(&self, w: &mut Writer) {
        w.slice(self.ram.as_slice());
        w.slice(self.cart_ram.as_slice());
        w.slice(&self.mapper);
        w.u8(self.io_control);
        self.vdp.save_state(w);
        self.psg.save_state(w);
    }

    pub(crate) fn load_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        r.fill(self.ram.as_mut_slice())?;
        r.fill(self.cart_ram.as_mut_slice())?;
        // O estado não guarda o sinal; RAM com conteúdo já foi usada.
        self.cart_ram_used |= self.cart_ram.iter().any(|&b| b != 0);
        r.fill(&mut self.mapper)?;
        self.io_control = r.u8()?;
        self.vdp.load_state(r)?;
        self.psg.load_state(r)?;
        self.line_cycles = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus_with_banks(banks: usize) -> SmsBus {
        let mut rom = vec![0; banks * BANK_SIZE];
        for (i, bank) in rom.chunks_mut(BANK_SIZE).enumerate() {
            bank.fill(i as u8);
        }
        SmsBus::new(rom, Psg::new(3_579_545, 44_100))
    }

    #[test]
    fn default_mapping_is_banks_0_1_2() {
        let mut bus = bus_with_banks(4);
        assert_eq!(bus.read(0x0000), 0);
        assert_eq!(bus.read(0x4000), 1);
        assert_eq!(bus.read(0x8000), 2);
    }

    #[test]
    fn mapper_registers_switch_banks_and_wrap() {
        let mut bus = bus_with_banks(4);
        bus.write(0xFFFF, 3);
        bus.write(0xFFFE, 6); // 6 % 4 = 2
        bus.write(0xFFFD, 1);
        assert_eq!(bus.read(0x8000), 3);
        assert_eq!(bus.read(0x4000), 2);
        assert_eq!(bus.read(0x0400), 1);
        // O primeiro 1 KB não muda.
        assert_eq!(bus.read(0x03FF), 0);
    }

    #[test]
    fn ram_is_mirrored() {
        let mut bus = bus_with_banks(1);
        bus.write(0xC010, 0x42);
        assert_eq!(bus.read(0xE010), 0x42);
        bus.write(0xFFFC, 0x00);
        assert_eq!(bus.read(0xDFFC), 0x00);
    }

    #[test]
    fn cartridge_ram_replaces_slot_2() {
        let mut bus = bus_with_banks(4);
        bus.write(0xFFFC, 0x08);
        bus.write(0x8000, 0x99);
        assert_eq!(bus.read(0x8000), 0x99);
        bus.write(0xFFFC, 0x0C); // segundo banco de RAM
        assert_eq!(bus.read(0x8000), 0x00);
        bus.write(0xFFFC, 0x00);
        assert_eq!(bus.read(0x8000), 2);
    }

    #[test]
    fn cartridge_ram_is_a_save_only_after_use() {
        let mut bus = bus_with_banks(4);
        assert!(
            bus.cart_ram().is_none(),
            "jogo que não usa a RAM não tem save"
        );
        bus.write(0x8000, 0x99); // RAM desligada: escrita na ROM, ignorada
        assert!(bus.cart_ram().is_none());
        bus.write(0xFFFC, 0x08);
        bus.write(0x8001, 0x42);
        let ram = bus.cart_ram().expect("RAM usada");
        assert_eq!(ram.len(), CART_RAM_SIZE);
        assert_eq!(ram[1], 0x42);
    }

    #[test]
    fn cartridge_ram_loads_from_saves_of_known_sizes() {
        let mut bus = bus_with_banks(4);
        for wrong in [0, 0x1000, 0x2001, 0x10000] {
            assert!(!bus.load_cart_ram(&vec![1; wrong]), "{wrong}");
        }
        assert!(bus.cart_ram().is_none(), "recusado: nada muda");

        assert!(bus.load_cart_ram(&[0x77; CART_RAM_SIZE]));
        assert!(bus.load_cart_ram(&[0x55; 0x2000]), "8 KB de outro emulador");
        let ram = bus.cart_ram().expect("carregada");
        assert_eq!(ram.len(), CART_RAM_SIZE);
        assert!(ram[..0x2000].iter().all(|&b| b == 0x55));
        assert!(ram[0x2000..].iter().all(|&b| b == 0), "o resto é zerado");
    }

    #[test]
    fn rom_writes_are_ignored() {
        let mut bus = bus_with_banks(2);
        bus.write(0x0100, 0x55);
        assert_eq!(bus.read(0x0100), 0);
    }

    #[test]
    fn joypad_bits_are_active_low() {
        let mut bus = bus_with_banks(1);
        assert_eq!(bus.input(0xDC), 0xFF);
        bus.joypad.up = true;
        bus.joypad.button1 = true;
        assert_eq!(bus.input(0xDC), !(0x01 | 0x10));
    }

    #[test]
    fn th_lines_read_back_output_level() {
        let mut bus = bus_with_banks(1);
        // TH-A e TH-B como saída, nível baixo.
        bus.output(0x3F, 0x05);
        assert_eq!(bus.input(0xDD) & 0xC0, 0x00);
        // Saída em nível alto.
        bus.output(0x3F, 0xF5);
        assert_eq!(bus.input(0xDD) & 0xC0, 0xC0);
    }
}
