//! Memory card de 128 KB: 1024 setores de 128 bytes, gravado no formato
//! bruto (`.mcr`), o mesmo dos outros emuladores.

/// Tamanho do cartão.
pub const MEMORY_CARD_SIZE: usize = 128 * 1024;
const SECTOR_SIZE: usize = 128;
const SECTORS: u16 = (MEMORY_CARD_SIZE / SECTOR_SIZE) as u16;
/// FLAG bit 3: nenhuma escrita desde que o cartão foi conectado. O BIOS
/// usa isso para saber que o cartão pode ter sido trocado.
const FLAG_FRESH: u8 = 0x08;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryCardError {
    /// O arquivo não tem 128 KB.
    WrongSize(usize),
    /// Não começa com o cabeçalho "MC" de um cartão formatado.
    NotFormatted,
}

impl std::fmt::Display for MemoryCardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongSize(size) => write!(
                f,
                "o arquivo tem {size} bytes; um memory card tem {MEMORY_CARD_SIZE}"
            ),
            Self::NotFormatted => f.write_str("o arquivo não é um memory card formatado"),
        }
    }
}

impl std::error::Error for MemoryCardError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Read,
    Write,
    Id,
    Unknown,
}

#[derive(Clone)]
pub(super) struct MemoryCard {
    data: Box<[u8; MEMORY_CARD_SIZE]>,
    flag: u8,
    /// Algum setor foi gravado desde que o cartão foi conectado.
    written: bool,
    /// Byte da sequência depois do endereço.
    step: usize,
    command: Command,
    sector: u16,
    checksum: u8,
    /// Último byte recebido: a escrita o devolve como eco.
    previous: u8,
    buffer: [u8; SECTOR_SIZE],
}

impl Default for MemoryCard {
    fn default() -> Self {
        Self {
            data: formatted(),
            flag: FLAG_FRESH,
            written: false,
            step: 0,
            command: Command::Unknown,
            sector: 0,
            checksum: 0,
            previous: 0,
            buffer: [0; SECTOR_SIZE],
        }
    }
}

/// Cartão vazio já formatado, como o BIOS o deixaria.
fn formatted() -> Box<[u8; MEMORY_CARD_SIZE]> {
    let mut data = Box::new([0u8; MEMORY_CARD_SIZE]);
    let mut frame = |index: usize, fill: &dyn Fn(&mut [u8])| {
        let frame = &mut data[index * SECTOR_SIZE..(index + 1) * SECTOR_SIZE];
        fill(frame);
        frame[SECTOR_SIZE - 1] = frame[..SECTOR_SIZE - 1].iter().fold(0, |x, b| x ^ b);
    };
    let header = |f: &mut [u8]| f[..2].copy_from_slice(b"MC");
    frame(0, &header);
    // Diretório: 15 blocos livres, sem próximo bloco.
    for i in 1..16 {
        frame(i, &|f| {
            f[0] = 0xA0;
            f[8..10].fill(0xFF);
        });
    }
    // Lista de setores defeituosos, vazia.
    for i in 16..36 {
        frame(i, &|f| {
            f[..4].fill(0xFF);
            f[8..10].fill(0xFF);
        });
    }
    // Quadro de teste de escrita: cópia do cabeçalho.
    frame(63, &header);
    data
}

/// Confere se `bytes` é um memory card: 128 KB começando com o cabeçalho
/// "MC" de um cartão formatado.
pub fn check_memory_card(bytes: &[u8]) -> Result<&[u8; MEMORY_CARD_SIZE], MemoryCardError> {
    let data: &[u8; MEMORY_CARD_SIZE] = bytes
        .try_into()
        .map_err(|_| MemoryCardError::WrongSize(bytes.len()))?;
    if &data[..2] != b"MC" {
        return Err(MemoryCardError::NotFormatted);
    }
    Ok(data)
}

impl MemoryCard {
    pub(super) fn data(&self) -> &[u8] {
        &self.data[..]
    }

    pub(super) fn written(&self) -> bool {
        self.written
    }

    /// Troca o conteúdo, como trocar o cartão: o BIOS vê o FLAG de novo.
    pub(super) fn load(&mut self, bytes: &[u8]) -> Result<(), MemoryCardError> {
        let data = check_memory_card(bytes)?;
        self.data.copy_from_slice(data);
        self.flag = FLAG_FRESH;
        self.written = false;
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        self.step = 0;
    }

    /// Troca um byte; devolve a resposta e se o cartão confirma (/ACK).
    pub(super) fn transfer(&mut self, tx: u8) -> (u8, bool) {
        let step = self.step;
        self.step += 1;
        let reply = if step == 0 {
            self.command = match tx {
                b'R' => Command::Read,
                b'W' => Command::Write,
                b'S' => Command::Id,
                _ => Command::Unknown,
            };
            (self.flag, self.command != Command::Unknown)
        } else {
            match self.command {
                Command::Read => self.read(step, tx),
                Command::Write => self.write(step, tx),
                Command::Id => id(step),
                Command::Unknown => (0xFF, false),
            }
        };
        self.previous = tx;
        reply
    }

    fn sector_valid(&self) -> bool {
        self.sector < SECTORS
    }

    fn sector_bytes(&mut self) -> &mut [u8] {
        let start = usize::from(self.sector) * SECTOR_SIZE;
        &mut self.data[start..start + SECTOR_SIZE]
    }

    /// Recebe o número do setor (passos 3 e 4).
    fn address(&mut self, step: usize, tx: u8) -> u8 {
        if step == 3 {
            self.sector = u16::from(tx) << 8;
            0x00
        } else {
            self.sector |= u16::from(tx);
            self.checksum = (self.sector >> 8) as u8 ^ tx;
            self.previous
        }
    }

    fn read(&mut self, step: usize, tx: u8) -> (u8, bool) {
        const DATA: usize = 9;
        let [msb, lsb] = self.sector.to_be_bytes();
        match step {
            1 => (0x5A, true),
            2 => (0x5D, true),
            3 | 4 => (self.address(step, tx), true),
            5 => (0x5C, true),
            6 => (0x5D, true),
            // Setor inexistente: responde FFFF e encerra.
            7 if !self.sector_valid() => (0xFF, true),
            8 if !self.sector_valid() => (0xFF, false),
            7 => (msb, true),
            8 => (lsb, true),
            DATA..=136 => {
                let byte = self.sector_bytes()[step - DATA];
                self.checksum ^= byte;
                (byte, true)
            }
            137 => (self.checksum, true),
            138 => (b'G', false),
            _ => (0xFF, false),
        }
    }

    fn write(&mut self, step: usize, tx: u8) -> (u8, bool) {
        const DATA: usize = 5;
        match step {
            1 => (0x5A, true),
            2 => (0x5D, true),
            3 | 4 => (self.address(step, tx), true),
            DATA..=132 => {
                self.buffer[step - DATA] = tx;
                self.checksum ^= tx;
                (self.previous, true)
            }
            133 => {
                // Guarda a diferença: 0 quando o checksum confere.
                self.checksum ^= tx;
                (self.previous, true)
            }
            134 => (0x5C, true),
            135 => (0x5D, true),
            136 => {
                let status = if !self.sector_valid() {
                    0xFF
                } else if self.checksum != 0 {
                    b'N'
                } else {
                    let buffer = self.buffer;
                    self.sector_bytes().copy_from_slice(&buffer);
                    self.flag &= !FLAG_FRESH;
                    self.written = true;
                    b'G'
                };
                (status, false)
            }
            _ => (0xFF, false),
        }
    }
}

/// Identificação do cartão: 1024 setores de 128 bytes.
fn id(step: usize) -> (u8, bool) {
    match step {
        1 => (0x5A, true),
        2 => (0x5D, true),
        3 => (0x5C, true),
        4 => (0x5D, true),
        5 => (0x04, true),
        6 | 7 => (0x00, true),
        8 => (0x80, false),
        _ => (0xFF, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(card: &mut MemoryCard, bytes: &[u8]) -> Vec<(u8, bool)> {
        card.reset();
        bytes.iter().map(|&b| card.transfer(b)).collect()
    }

    fn write_frame(sector: u16, data: &[u8; 128], checksum_fix: u8) -> Vec<u8> {
        let [msb, lsb] = sector.to_be_bytes();
        let checksum = data.iter().fold(msb ^ lsb, |x, b| x ^ b) ^ checksum_fix;
        let mut bytes = vec![b'W', 0, 0, msb, lsb];
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&[checksum, 0, 0, 0]);
        bytes
    }

    #[test]
    fn write_then_read_round_trip() {
        let mut card = MemoryCard::default();
        assert!(!card.written());
        let data: [u8; 128] = std::array::from_fn(|i| (i as u8).wrapping_mul(3));
        let replies = run(&mut card, &write_frame(0x0123, &data, 0));
        assert!(card.written(), "gravou um setor");
        assert_eq!(replies[0], (FLAG_FRESH, true));
        assert_eq!(replies[5].0, 0x23, "eco do byte anterior");
        assert_eq!(replies.last(), Some(&(b'G', false)));
        assert!(replies[..replies.len() - 1].iter().all(|r| r.1));

        let mut read = vec![b'R', 0, 0, 0x01, 0x23];
        read.resize(139, 0);
        let replies = run(&mut card, &read);
        assert_eq!(replies[0].0, 0, "FLAG limpo pela escrita");
        assert_eq!(
            &replies[5..9],
            &[(0x5C, true), (0x5D, true), (1, true), (0x23, true)]
        );
        let got: Vec<u8> = replies[9..137].iter().map(|r| r.0).collect();
        assert_eq!(got, data);
        let checksum = data.iter().fold(0x01 ^ 0x23, |x, b| x ^ b);
        assert_eq!(&replies[137..], &[(checksum, true), (b'G', false)]);
    }

    #[test]
    fn bad_checksum_or_sector_is_refused() {
        let mut card = MemoryCard::default();
        let before = card.data().to_vec();
        let replies = run(&mut card, &write_frame(40, &[7; 128], 1));
        assert_eq!(replies.last(), Some(&(b'N', false)));
        let replies = run(&mut card, &write_frame(0x400, &[7; 128], 0));
        assert_eq!(replies.last(), Some(&(0xFF, false)));
        assert_eq!(card.data(), &before[..], "nada gravado");
        assert_eq!(card.flag, FLAG_FRESH);
        assert!(!card.written());

        let mut read = vec![b'R', 0, 0, 0x04, 0x00];
        read.resize(9, 0);
        let replies = run(&mut card, &read);
        assert_eq!(&replies[7..], &[(0xFF, true), (0xFF, false)]);
    }

    #[test]
    fn id_command() {
        let mut card = MemoryCard::default();
        let replies: Vec<u8> = run(&mut card, &[b'S', 0, 0, 0, 0, 0, 0, 0, 0])
            .iter()
            .map(|r| r.0)
            .collect();
        assert_eq!(
            replies,
            [FLAG_FRESH, 0x5A, 0x5D, 0x5C, 0x5D, 0x04, 0, 0, 0x80]
        );
    }

    #[test]
    fn new_card_is_formatted_with_valid_checksums() {
        let card = MemoryCard::default();
        assert_eq!(&card.data()[..2], b"MC");
        for frame in card.data()[..64 * 128].chunks(128) {
            assert_eq!(frame.iter().fold(0, |x, b| x ^ b), 0);
        }
        assert_eq!(card.data()[128], 0xA0, "bloco 1 livre");
    }

    #[test]
    fn load_validates_size_and_header() {
        let mut card = MemoryCard::default();
        let mut image = card.data().to_vec();
        image[0x2000] = 0x55;
        assert_eq!(
            card.load(&image[..1000]),
            Err(MemoryCardError::WrongSize(1000))
        );
        let mut blank = image.clone();
        blank[..2].fill(0);
        assert_eq!(card.load(&blank), Err(MemoryCardError::NotFormatted));
        assert_eq!(card.data()[0x2000], 0, "recusado sem mudar nada");
        assert!(check_memory_card(&blank).is_err());
        assert!(check_memory_card(&image).is_ok());
        assert_eq!(card.load(&image), Ok(()));
        assert_eq!(card.data(), &image[..]);
        assert!(!card.written(), "trocar o cartão não é gravar");
    }
}
