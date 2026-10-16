//! Porta dos controles e memory cards (SIO0, 0x1F801040–0x1F80104F).
//!
//! A CPU escolhe a porta e baixa o /JOY; cada byte enviado volta com a
//! resposta do dispositivo depois de 8 bits no baud programado. O primeiro
//! byte é o endereço: 0x01 fala com o controle, 0x81 com o memory card. O
//! dispositivo que quer o próximo byte pulsa o /ACK, que gera a IRQ 7.
//!
//! Na porta 1 há um controle digital e um memory card; a porta 2 fica
//! vazia. Não são modelados o nível do /ACK em JOY_STAT (bit 7) nem a
//! interrupção por bytes recebidos (JOY_CTRL bit 11), que o BIOS não usa.

mod memory_card;
mod pad;

pub use memory_card::{MEMORY_CARD_SIZE, MemoryCardError, check_memory_card};
pub use pad::Buttons;

use memory_card::MemoryCard;
use pad::Pad;

/// Do fim do byte ao /ACK (aproximado, como medido no hardware).
const PAD_ACK_DELAY: u64 = 338;
const CARD_ACK_DELAY: u64 = 170;

const CTRL_TX_ENABLE: u16 = 1 << 0;
const CTRL_SELECT: u16 = 1 << 1;
const CTRL_ACKNOWLEDGE: u16 = 1 << 4;
const CTRL_RESET: u16 = 1 << 6;
const CTRL_ACK_IRQ: u16 = 1 << 12;
const CTRL_PORT_2: u16 = 1 << 13;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Device {
    /// Ninguém respondeu ao endereço: a porta fica calada até soltar o /JOY.
    Nobody,
    Pad,
    Card,
}

#[derive(Clone, Copy)]
struct Transfer {
    end: u64,
    reply: u8,
    /// Ciclos entre o fim do byte e o /ACK, se o dispositivo confirma.
    ack_delay: Option<u64>,
}

#[derive(Clone, Default)]
pub(crate) struct Sio {
    mode: u16,
    control: u16,
    baud: u16,
    rx: Option<u8>,
    irq: bool,
    /// Dispositivo que respondeu ao endereço; `None` antes do primeiro byte.
    device: Option<Device>,
    transfer: Option<Transfer>,
    ack_at: Option<u64>,
    pad: Pad,
    card: MemoryCard,
}

impl Sio {
    pub(crate) fn set_buttons(&mut self, buttons: Buttons) {
        self.pad.buttons = buttons;
    }

    pub(crate) fn memory_card(&self) -> &[u8] {
        self.card.data()
    }

    pub(crate) fn memory_card_written(&self) -> bool {
        self.card.written()
    }

    pub(crate) fn load_memory_card(&mut self, bytes: &[u8]) -> Result<(), MemoryCardError> {
        self.card.load(bytes)
    }

    pub(crate) fn read(&mut self, offset: u32) -> u32 {
        match offset {
            // Sem byte recebido, o registrador lê 0xFF.
            0x0 => u32::from(self.rx.take().unwrap_or(0xFF)),
            0x4 => self.status(),
            0x8 => u32::from(self.mode),
            0xA => u32::from(self.control),
            0xE => u32::from(self.baud),
            _ => 0,
        }
    }

    /// JOY_STAT: TX pronto (bit 0), byte recebido (1), transmissão
    /// terminada (2) e IRQ (9).
    fn status(&self) -> u32 {
        1 | u32::from(self.rx.is_some()) << 1
            | u32::from(self.transfer.is_none()) << 2
            | u32::from(self.irq) << 9
    }

    pub(crate) fn write(&mut self, offset: u32, value: u32, now: u64) {
        match offset {
            0x0 => self.send(value as u8, now),
            0x8 => self.mode = value as u16,
            0xA => self.set_control(value as u16),
            0xE => self.baud = value as u16,
            _ => {}
        }
    }

    fn set_control(&mut self, value: u16) {
        if value & CTRL_RESET != 0 {
            self.mode = 0;
            self.control = 0;
            self.baud = 0;
            self.rx = None;
            self.irq = false;
            self.transfer = None;
            self.deselect();
            return;
        }
        if value & CTRL_ACKNOWLEDGE != 0 {
            self.irq = false;
        }
        let port_changed = (value ^ self.control) & CTRL_PORT_2 != 0;
        if value & CTRL_SELECT == 0 || port_changed {
            self.deselect();
        }
        self.control = value & !(CTRL_ACKNOWLEDGE | CTRL_RESET);
    }

    fn deselect(&mut self) {
        self.device = None;
        self.ack_at = None;
        self.pad.reset();
        self.card.reset();
    }

    /// Ciclos de um byte: 8 bits de `baud × fator` (JOY_MODE bits 0–1).
    fn byte_ticks(&self) -> u64 {
        let factor = match self.mode & 3 {
            2 => 16,
            3 => 64,
            _ => 1,
        };
        (u64::from(self.baud) * factor).max(1) * 8
    }

    fn send(&mut self, tx: u8, now: u64) {
        let (reply, ack) = self.exchange(tx);
        let delay = match self.device {
            Some(Device::Card) => CARD_ACK_DELAY,
            _ => PAD_ACK_DELAY,
        };
        self.transfer = Some(Transfer {
            end: now + self.byte_ticks(),
            reply,
            ack_delay: ack.then_some(delay),
        });
    }

    /// Resposta do dispositivo selecionado ao byte `tx`, e se ele confirma.
    fn exchange(&mut self, tx: u8) -> (u8, bool) {
        let port_1_selected = self.control & (CTRL_TX_ENABLE | CTRL_SELECT | CTRL_PORT_2)
            == CTRL_TX_ENABLE | CTRL_SELECT;
        if !port_1_selected {
            return (0xFF, false);
        }
        match self.device {
            None => {
                let device = match tx {
                    0x01 => Device::Pad,
                    0x81 => Device::Card,
                    _ => Device::Nobody,
                };
                self.device = Some(device);
                (0xFF, device != Device::Nobody)
            }
            Some(Device::Nobody) => (0xFF, false),
            Some(Device::Pad) => self.pad.transfer(tx),
            Some(Device::Card) => self.card.transfer(tx),
        }
    }

    /// Avança até `now`; devolve `true` quando a IRQ 7 sobe.
    pub(crate) fn sync(&mut self, now: u64) -> bool {
        if let Some(t) = self.transfer
            && t.end <= now
        {
            self.transfer = None;
            self.rx = Some(t.reply);
            self.ack_at = t.ack_delay.map(|delay| t.end + delay);
        }
        if let Some(at) = self.ack_at
            && at <= now
        {
            self.ack_at = None;
            if self.control & CTRL_ACK_IRQ != 0 && !self.irq {
                self.irq = true;
                return true;
            }
        }
        false
    }

    /// Próximo ciclo em que algo muda na porta.
    pub(crate) fn next_event(&self) -> Option<u64> {
        self.transfer.map(|t| t.end).or(self.ack_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Porta 1 selecionada, com IRQ no /ACK e baud 0x88 (1088 ciclos/byte).
    fn selected() -> Sio {
        let mut sio = Sio::default();
        sio.write(0xE, 0x88, 0);
        sio.write(0x8, 0x0D, 0);
        sio.write(
            0xA,
            u32::from(CTRL_TX_ENABLE | CTRL_SELECT | CTRL_ACK_IRQ),
            0,
        );
        sio
    }

    /// Envia um byte e roda até a resposta; devolve (resposta, IRQ).
    fn exchange(sio: &mut Sio, now: &mut u64, tx: u8) -> (u8, bool) {
        sio.write(0x0, u32::from(tx), *now);
        let mut irq = false;
        while let Some(at) = sio.next_event() {
            *now = at;
            irq |= sio.sync(at);
        }
        if irq {
            sio.write(0xA, u32::from(sio.control | CTRL_ACKNOWLEDGE), *now);
        }
        (sio.read(0x0) as u8, irq)
    }

    #[test]
    fn byte_takes_eight_baud_periods_then_acks() {
        let mut sio = selected();
        sio.write(0x0, 0x01, 100);
        assert_eq!(sio.status() & 0b110, 0, "em transmissão");
        assert_eq!(sio.next_event(), Some(100 + 1088));
        assert!(!sio.sync(1187));
        assert!(!sio.sync(1188));
        assert_eq!(sio.status() & 0b110, 0b110, "byte recebido, TX livre");
        assert_eq!(sio.next_event(), Some(1188 + PAD_ACK_DELAY));
        assert!(sio.sync(1188 + PAD_ACK_DELAY));
        assert_ne!(sio.status() & (1 << 9), 0);
        sio.write(0xA, u32::from(sio.control | CTRL_ACKNOWLEDGE), 2000);
        assert_eq!(sio.status() & (1 << 9), 0, "reconhecida");
        assert_eq!(sio.read(0x0), 0xFF);
        assert_eq!(sio.read(0x0), 0xFF, "FIFO vazia");
    }

    #[test]
    fn polls_the_pad() {
        let mut sio = selected();
        sio.set_buttons(Buttons {
            left: true,
            ..Buttons::default()
        });
        let mut now = 0;
        let replies: Vec<_> = [0x01, 0x42, 0, 0, 0]
            .map(|b| exchange(&mut sio, &mut now, b))
            .into();
        assert_eq!(
            replies,
            [
                (0xFF, true),
                (0x41, true),
                (0x5A, true),
                (0x7F, true),
                (0xFF, false)
            ]
        );
    }

    #[test]
    fn unknown_address_or_port_2_stays_silent() {
        let mut sio = selected();
        let mut now = 0;
        assert_eq!(exchange(&mut sio, &mut now, 0x02), (0xFF, false));
        assert_eq!(exchange(&mut sio, &mut now, 0x42), (0xFF, false));

        let mut sio = selected();
        let port_2 = CTRL_TX_ENABLE | CTRL_SELECT | CTRL_ACK_IRQ | CTRL_PORT_2;
        sio.write(0xA, u32::from(port_2), 0);
        assert_eq!(exchange(&mut sio, &mut now, 0x01), (0xFF, false));
    }

    #[test]
    fn deselecting_restarts_the_sequence() {
        let mut sio = selected();
        let mut now = 0;
        exchange(&mut sio, &mut now, 0x81);
        assert!(exchange(&mut sio, &mut now, b'S').1);
        sio.write(0xA, 0, now);
        sio.write(
            0xA,
            u32::from(CTRL_TX_ENABLE | CTRL_SELECT | CTRL_ACK_IRQ),
            now,
        );
        assert_eq!(exchange(&mut sio, &mut now, 0x01), (0xFF, true));
        assert_eq!(exchange(&mut sio, &mut now, 0x42), (0x41, true));
    }

    #[test]
    fn card_ack_is_faster_than_pad() {
        let mut sio = selected();
        sio.write(0x0, 0x81, 0);
        sio.sync(1088);
        assert_eq!(sio.next_event(), Some(1088 + CARD_ACK_DELAY));
    }
}
