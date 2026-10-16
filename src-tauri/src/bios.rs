//! BIOS dos consoles que precisam dele (o PlayStation). O BIOS é do usuário:
//! o app não o distribui. Escolhido nas configurações do console, ele é
//! conferido e copiado para `<dados do app>/bios/<console>.bin`, então
//! continua valendo mesmo se o arquivo original mudar de lugar.

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use player::controls::System;

/// Maior arquivo lido ao instalar; o BIOS do PlayStation tem 512 KB.
const MAX_BIOS_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub enum BiosError {
    /// O arquivo não é um BIOS que o console aceita.
    Rejected(player::BiosError),
    TooLarge(u64),
    Io(io::Error),
}

impl fmt::Display for BiosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(e) => e.fmt(f),
            Self::TooLarge(size) => {
                write!(f, "o arquivo tem {size} bytes; grande demais para um BIOS")
            }
            Self::Io(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for BiosError {}

pub struct BiosStore {
    dir: PathBuf,
}

impl BiosStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn path_for(&self, system: System) -> PathBuf {
        self.dir.join(format!("{}.bin", system.id()))
    }

    /// O BIOS do console já foi escolhido.
    pub fn installed(&self, system: System) -> bool {
        self.path_for(system).is_file()
    }

    /// Confere `source` e o copia como BIOS do console. Recusado ou com
    /// falha, o BIOS anterior (se houver) fica como estava.
    pub fn install(&self, system: System, source: &Path) -> Result<(), BiosError> {
        let file = fs::File::open(source).map_err(BiosError::Io)?;
        let size = file.metadata().map_err(BiosError::Io)?.len();
        if size > MAX_BIOS_BYTES {
            return Err(BiosError::TooLarge(size));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BIOS_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(BiosError::Io)?;
        player::check_bios(system, &bytes).map_err(BiosError::Rejected)?;
        write_atomic(&self.path_for(system), &bytes).map_err(BiosError::Io)
    }
}

/// Arquivo temporário ao lado e rename por cima: nunca fica um BIOS pela
/// metade.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        // Melhor esforço: o erro que importa é o da gravação.
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS1: System = System::PlayStation;

    #[test]
    fn installs_a_copy_of_a_valid_bios() {
        let dir = tempfile::tempdir().expect("pasta");
        let store = BiosStore::new(dir.path().join("bios"));
        assert!(!store.installed(PS1));
        let source = dir.path().join("SCPH1001.BIN");
        fs::write(&source, vec![7; 512 * 1024]).expect("escrita");
        store.install(PS1, &source).expect("instala");
        fs::remove_file(&source).expect("o original pode sumir");
        assert!(store.installed(PS1));
        assert_eq!(
            fs::read(store.path_for(PS1)).expect("leitura"),
            vec![7; 512 * 1024]
        );
    }

    #[test]
    fn invalid_bios_keeps_the_installed_one() {
        let dir = tempfile::tempdir().expect("pasta");
        let store = BiosStore::new(dir.path().join("bios"));
        let good = dir.path().join("bom.bin");
        fs::write(&good, vec![1; 512 * 1024]).expect("escrita");
        store.install(PS1, &good).expect("instala");

        let small = dir.path().join("pequeno.bin");
        fs::write(&small, [0; 1000]).expect("escrita");
        assert!(matches!(
            store.install(PS1, &small),
            Err(BiosError::Rejected(_))
        ));
        let huge = dir.path().join("enorme.bin");
        fs::write(&huge, vec![0; MAX_BIOS_BYTES as usize + 1]).expect("escrita");
        assert!(matches!(
            store.install(PS1, &huge),
            Err(BiosError::TooLarge(_))
        ));
        assert!(matches!(
            store.install(PS1, &dir.path().join("nao-existe.bin")),
            Err(BiosError::Io(_))
        ));
        assert_eq!(
            fs::read(store.path_for(PS1)).expect("leitura"),
            vec![1; 512 * 1024]
        );
    }
}
