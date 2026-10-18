//! Número de série do disco: o nome do executável que o BIOS carrega, na
//! linha `BOOT` do `SYSTEM.CNF` na raiz do sistema de arquivos ISO 9660.
//! É o que identifica o jogo nos DATs Redump (ex.: `SCUS-94900`).

use super::{Disc, FIRST_SECTOR, SECTOR_SIZE};

/// Começo dos dados num setor modo 2 forma 1, depois do sincronismo, do
/// cabeçalho e do sub-cabeçalho.
const USER_DATA: usize = 24;
const USER_SIZE: usize = 2048;
/// Setor lógico do descritor de volume primário.
const PRIMARY_VOLUME: u32 = 16;
/// Posição do registro do diretório raiz no descritor.
const ROOT_RECORD: usize = 156;
/// Maior diretório raiz percorrido, em setores: um disco corrompido não faz
/// a busca ler o disco inteiro.
const MAX_ROOT_SECTORS: u32 = 16;
/// Tamanho mínimo de um registro de diretório (sem o nome).
const RECORD_HEADER: usize = 33;

impl Disc {
    /// Número de série do jogo (ex.: `"SCUS-94900"`), ou `None` quando o
    /// disco não tem `SYSTEM.CNF` (o BIOS então carrega `PSX.EXE`) ou o
    /// executável não segue o padrão `XXXX_NNN.NN`.
    pub fn serial(&self) -> Option<String> {
        let (root, root_size) = self.root_directory()?;
        let (cnf, cnf_size) = self.find_file(root, root_size, b"SYSTEM.CNF")?;
        let data = self.user_data(cnf)?;
        boot_serial(&data[..(cnf_size as usize).min(USER_SIZE)])
    }

    /// Tamanho em bytes do primeiro arquivo da imagem; nos dumps Redump, o
    /// da trilha 1.
    pub fn first_file_size(&self) -> u64 {
        self.files
            .first()
            .map_or(0, |(_, sectors)| sectors * SECTOR_SIZE as u64)
    }

    /// Dados do setor lógico `lba` da trilha 1.
    fn user_data(&self, lba: u32) -> Option<[u8; USER_SIZE]> {
        let mut sector = [0; SECTOR_SIZE];
        self.read(FIRST_SECTOR.checked_add(lba)?, &mut sector)
            .ok()?;
        let mut data = [0; USER_SIZE];
        data.copy_from_slice(&sector[USER_DATA..USER_DATA + USER_SIZE]);
        Some(data)
    }

    fn root_directory(&self) -> Option<(u32, u32)> {
        let volume = self.user_data(PRIMARY_VOLUME)?;
        if volume[0] != 1 || &volume[1..6] != b"CD001" {
            return None;
        }
        extent(&volume[ROOT_RECORD..])
    }

    /// Posição e tamanho do arquivo `name` (sem a versão `;1`) no diretório.
    fn find_file(&self, directory: u32, size: u32, name: &[u8]) -> Option<(u32, u32)> {
        let sectors = size.div_ceil(USER_SIZE as u32).min(MAX_ROOT_SECTORS);
        for i in 0..sectors {
            let data = self.user_data(directory.checked_add(i)?)?;
            let mut at = 0;
            // Registros não cruzam setores; um tamanho zero é o preenchimento
            // até o fim do setor.
            while let Some(&len) = data.get(at)
                && len != 0
            {
                let record = data.get(at..at + usize::from(len))?;
                let name_len = usize::from(*record.get(RECORD_HEADER - 1)?);
                let file = record.get(RECORD_HEADER..RECORD_HEADER + name_len)?;
                let file = file.split(|&b| b == b';').next().unwrap_or(file);
                let is_directory = record[25] & 2 != 0;
                if !is_directory && file.eq_ignore_ascii_case(name) {
                    return extent(record);
                }
                at += usize::from(len);
            }
        }
        None
    }
}

/// Setor e tamanho de um registro de diretório (campos little-endian).
fn extent(record: &[u8]) -> Option<(u32, u32)> {
    let lba = u32::from_le_bytes(record.get(2..6)?.try_into().ok()?);
    let size = u32::from_le_bytes(record.get(10..14)?.try_into().ok()?);
    Some((lba, size))
}

/// `BOOT = cdrom:\SCUS_949.00;1` vira `SCUS-94900`.
fn boot_serial(cnf: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(cnf);
    let path = text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        key.trim().eq_ignore_ascii_case("BOOT").then_some(value)
    })?;
    let file = path.rsplit(['\\', '/', ':']).next()?;
    let file = file.split(';').next()?;
    let file = file.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    // `XXXX_NNN.NN`: o prefixo da região e o número, partido pelo ponto
    // do formato 8.3.
    let b = file.as_bytes();
    let valid = b.len() == 11
        && b[..4].iter().all(u8::is_ascii_alphabetic)
        && matches!(b[4], b'_' | b'-')
        && b[5..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'.'
        && b[9..].iter().all(u8::is_ascii_digit);
    valid.then(|| {
        format!(
            "{}-{}{}",
            file[..4].to_ascii_uppercase(),
            &file[5..8],
            &file[9..]
        )
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::disc_with;
    use super::*;

    /// Registro de diretório com `name`, no setor `lba`, de `size` bytes.
    fn record(name: &[u8], lba: u32, size: u32, directory: bool) -> Vec<u8> {
        let len = RECORD_HEADER + name.len() + (name.len() + 1) % 2;
        let mut r = vec![0; len];
        r[0] = len as u8;
        r[2..6].copy_from_slice(&lba.to_le_bytes());
        r[10..14].copy_from_slice(&size.to_le_bytes());
        r[25] = if directory { 2 } else { 0 };
        r[32] = name.len() as u8;
        r[RECORD_HEADER..RECORD_HEADER + name.len()].copy_from_slice(name);
        r
    }

    /// Disco ISO 9660 com os arquivos dados na raiz (setor 18); cada arquivo
    /// ocupa um setor a partir do 20.
    fn iso(files: &[(&[u8], &[u8])]) -> Disc {
        let mut sectors = vec![[0; SECTOR_SIZE]; 20 + files.len()];
        let mut put = |lba: usize, data: &[u8]| {
            sectors[lba][USER_DATA..USER_DATA + data.len()].copy_from_slice(data);
        };
        let mut volume = vec![1];
        volume.extend(b"CD001");
        volume.resize(ROOT_RECORD, 0);
        volume.extend(record(b"\0", 18, USER_SIZE as u32, true));
        put(16, &volume);
        let mut root = record(b"\0", 18, USER_SIZE as u32, true);
        root.extend(record(b"\x01", 18, USER_SIZE as u32, true));
        for (i, (name, data)) in files.iter().enumerate() {
            root.extend(record(name, 20 + i as u32, data.len() as u32, false));
            put(20 + i, data);
        }
        put(18, &root);
        disc_with(&sectors)
    }

    #[test]
    fn serial_comes_from_the_boot_line_of_system_cnf() {
        let disc = iso(&[
            (b"PSX.EXE;1", b"executavel"),
            (
                b"SYSTEM.CNF;1",
                b"BOOT = cdrom:\\SCUS_949.00;1\r\nTCB = 4\r\nSTACK = 801FFFF0\r\n",
            ),
        ]);
        assert_eq!(disc.serial().as_deref(), Some("SCUS-94900"));
        assert_eq!(disc.first_file_size(), 22 * SECTOR_SIZE as u64);
    }

    #[test]
    fn discs_without_a_serial_are_not_guessed() {
        assert_eq!(iso(&[(b"PSX.EXE;1", b"executavel")]).serial(), None);
        let directory = iso(&[]);
        assert_eq!(directory.serial(), None);
        assert_eq!(disc_with(&[[0; SECTOR_SIZE]; 20]).serial(), None, "sem ISO");
        assert_eq!(disc_with(&[[0; SECTOR_SIZE]; 3]).serial(), None, "curto");
    }

    #[test]
    fn boot_line_variants() {
        let serial = |text: &str| boot_serial(text.as_bytes());
        assert_eq!(
            serial("BOOT=cdrom:SLUS_000.67;1").as_deref(),
            Some("SLUS-00067")
        );
        assert_eq!(
            serial("boot = cdrom0:\\EXE\\slps_012.34;1\0\0").as_deref(),
            Some("SLPS-01234")
        );
        assert_eq!(
            serial("TCB = 4\nBOOT = cdrom:\\SLES_123.45").as_deref(),
            Some("SLES-12345")
        );
        assert_eq!(serial("BOOT = cdrom:\\MAIN.EXE;1"), None);
        assert_eq!(serial("BOOT = cdrom:\\SLUS_0001.2;1"), None);
        assert_eq!(serial("TCB = 4"), None);
    }
}
