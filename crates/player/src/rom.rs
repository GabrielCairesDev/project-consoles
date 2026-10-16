//! Leitura da ROM: arquivo direto ou a primeira ROM de dentro de um `.zip`.

use std::fmt;
use std::io::{self, Read, Seek};
use std::path::Path;

use crate::controls::System;

/// Maior ROM descompactada aceita; protege contra zip "bomba".
const MAX_ROM_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum RomReadError {
    Io(io::Error),
    Zip(zip::result::ZipError),
    NoRomInZip,
    TooLarge(u64),
}

impl fmt::Display for RomReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Zip(e) => write!(f, "zip inválido: {e}"),
            Self::NoRomInZip => f.write_str("o zip não contém uma ROM deste console"),
            Self::TooLarge(size) => write!(f, "a ROM tem {size} bytes; grande demais"),
        }
    }
}

impl std::error::Error for RomReadError {}

/// Extensões de ROM de cada console (sem o ponto, minúsculas).
fn extensions(system: System) -> &'static [&'static str] {
    match system {
        System::MasterSystem => &["sms"],
        System::Genesis => &["md", "gen", "bin", "smd"],
    }
}

fn lowercase_extension(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

pub fn read(path: &Path, system: System) -> Result<Vec<u8>, RomReadError> {
    let file = std::fs::File::open(path).map_err(RomReadError::Io)?;
    if lowercase_extension(path).as_deref() == Some("zip") {
        return from_zip(file, system);
    }
    let size = file.metadata().map_err(RomReadError::Io)?.len();
    if size > MAX_ROM_BYTES {
        return Err(RomReadError::TooLarge(size));
    }
    let mut rom = Vec::with_capacity(size as usize);
    io::BufReader::new(file)
        .read_to_end(&mut rom)
        .map_err(RomReadError::Io)?;
    Ok(rom)
}

/// A primeira entrada (na ordem do zip) com extensão de ROM do console.
fn from_zip(reader: impl Read + Seek, system: System) -> Result<Vec<u8>, RomReadError> {
    let mut archive = zip::ZipArchive::new(reader).map_err(RomReadError::Zip)?;
    let wanted = extensions(system);
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(RomReadError::Zip)?;
        let is_rom = entry.is_file()
            && entry.name().is_ok_and(|name| {
                lowercase_extension(Path::new(name.as_ref()))
                    .is_some_and(|ext| wanted.contains(&ext.as_str()))
            });
        if !is_rom {
            continue;
        }
        if entry.size() > MAX_ROM_BYTES {
            return Err(RomReadError::TooLarge(entry.size()));
        }
        let mut rom = Vec::with_capacity(entry.size() as usize);
        // O tamanho declarado pode mentir: o `take` limita o que é lido.
        entry
            .take(MAX_ROM_BYTES + 1)
            .read_to_end(&mut rom)
            .map_err(RomReadError::Io)?;
        if rom.len() as u64 > MAX_ROM_BYTES {
            return Err(RomReadError::TooLarge(rom.len() as u64));
        }
        return Ok(rom);
    }
    Err(RomReadError::NoRomInZip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn zip_with(files: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
        for (name, data) in files {
            writer.start_file(*name, options).expect("entrada zip");
            writer.write_all(data).expect("escrita zip");
        }
        writer.finish().expect("zip finalizado").into_inner()
    }

    #[test]
    fn reads_plain_file() {
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("jogo.sms");
        std::fs::write(&path, [1, 2, 3]).expect("escrita");
        assert_eq!(
            read(&path, System::MasterSystem).expect("leitura"),
            [1, 2, 3]
        );
    }

    #[test]
    fn picks_first_rom_of_the_console_from_deflated_zip() {
        let rom = vec![0x5A; 4096];
        let bytes = zip_with(
            &[
                ("leia-me.txt", b"x"),
                ("Jogo.MD", &rom),
                ("outro.bin", b"y"),
            ],
            zip::CompressionMethod::Deflated,
        );
        let dir = tempfile::tempdir().expect("pasta temporária");
        let path = dir.path().join("Jogo.ZIP");
        std::fs::write(&path, bytes).expect("escrita");
        assert_eq!(read(&path, System::Genesis).expect("leitura"), rom);
    }

    #[test]
    fn zip_without_rom_of_the_console_is_an_error() {
        let bytes = zip_with(&[("jogo.md", b"x")], zip::CompressionMethod::Stored);
        assert!(matches!(
            from_zip(Cursor::new(bytes), System::MasterSystem),
            Err(RomReadError::NoRomInZip)
        ));
        assert!(matches!(
            from_zip(Cursor::new(b"nao e zip".to_vec()), System::Genesis),
            Err(RomReadError::Zip(_))
        ));
    }
}
