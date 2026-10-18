//! Impressão digital de ROMs: tamanho e CRC32 dos bytes normalizados, no
//! mesmo formato dos dumps de referência do No-Intro.
//!
//! O arquivo é lido em blocos (memória constante), pois ROMs de 3DS chegam a
//! vários GB. Arquivos `.zip` usam o CRC e o tamanho do diretório central, sem
//! descompactar.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::systems::{self, RomFormat};

const CHUNK: usize = 1 << 16;
const SNES_COPIER_HEADER: u64 = 512;

/// Identidade de uma ROM para consulta no DAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub size: u64,
    pub crc32: u32,
}

/// Uma ROM lida do disco.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RomFile {
    pub fingerprint: Fingerprint,
    /// Extensão da ROM em minúsculas; num `.zip`, a do arquivo de dentro.
    pub extension: String,
    /// Identidade do disco, para imagens de disco (`.cue`) que a têm.
    #[serde(default)]
    pub disc: Option<DiscId>,
}

/// O que identifica um disco no DAT Redump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscId {
    /// Número de série do `SYSTEM.CNF` (ex.: `SCUS-94900`).
    pub serial: String,
    /// Tamanho do primeiro `.bin`, que separa revisões do mesmo número.
    pub first_file_size: u64,
}

/// Lê a ROM em `path`. `Ok(None)` quando não é ROM de nenhum sistema: extensão
/// desconhecida, ou `.zip` sem exatamente uma ROM dentro.
pub fn read_rom(path: &Path) -> io::Result<Option<RomFile>> {
    let Some(extension) = lowercase_extension(path) else {
        return Ok(None);
    };
    if extension == "zip" {
        return zip_rom(File::open(path)?);
    }
    let Some(system) = systems::for_extension(&extension) else {
        return Ok(None);
    };
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let fingerprint = stream_fingerprint(BufReader::new(file), size, system.format)?;
    let disc = (extension == "cue").then(|| disc_id(path)).flatten();
    Ok(Some(RomFile {
        fingerprint,
        extension,
        disc,
    }))
}

/// Número de série do disco do `.cue`. Uma imagem que não abre ainda entra
/// na biblioteca, sem reconhecimento; o erro aparece ao jogar.
fn disc_id(cue: &Path) -> Option<DiscId> {
    match psx::Disc::open(cue) {
        Ok(disc) => disc.serial().map(|serial| DiscId {
            serial,
            first_file_size: disc.first_file_size(),
        }),
        Err(error) => {
            eprintln!("não foi possível abrir o disco {}: {error}", cue.display());
            None
        }
    }
}

/// Extensão em minúsculas, se for UTF-8 válido.
pub fn lowercase_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
}

/// CRC32 dos bytes normalizados de `reader`, que tem `size` bytes.
pub fn stream_fingerprint(
    mut reader: impl Read,
    size: u64,
    format: RomFormat,
) -> io::Result<Fingerprint> {
    let mut size = size;
    if format == RomFormat::Snes && size % 1024 == SNES_COPIER_HEADER {
        io::copy(&mut (&mut reader).take(SNES_COPIER_HEADER), &mut io::sink())?;
        size -= SNES_COPIER_HEADER;
    }

    let mut hasher = crc32fast::Hasher::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut order: Option<N64Order> = None;
    loop {
        let read = read_full(&mut reader, &mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = &mut buffer[..read];
        if format == RomFormat::N64 {
            let order = *order.get_or_insert_with(|| N64Order::detect(chunk));
            order.to_big_endian(chunk);
        }
        hasher.update(chunk);
    }
    Ok(Fingerprint {
        size,
        crc32: hasher.finalize(),
    })
}

/// Lê até encher `buffer` ou chegar ao fim. Mantém os blocos alinhados a 4
/// bytes, o que a conversão de ordem do N64 exige.
fn read_full(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Ordem de bytes de um dump de N64, detectada pelos 4 primeiros bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum N64Order {
    /// `.z64`: 80 37 12 40 (referência).
    BigEndian,
    /// `.v64`: 37 80 40 12 (pares de bytes trocados).
    ByteSwapped,
    /// `.n64`: 40 12 37 80 (palavras de 32 bits invertidas).
    LittleEndian,
}

impl N64Order {
    fn detect(start: &[u8]) -> Self {
        match start.get(..4) {
            Some([0x37, 0x80, 0x40, 0x12]) => Self::ByteSwapped,
            Some([0x40, 0x12, 0x37, 0x80]) => Self::LittleEndian,
            // Desconhecido: usa como está; o CRC simplesmente não vai bater.
            _ => Self::BigEndian,
        }
    }

    fn to_big_endian(self, chunk: &mut [u8]) {
        match self {
            Self::BigEndian => {}
            Self::ByteSwapped => chunk
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .for_each(|pair| pair.swap(0, 1)),
            Self::LittleEndian => chunk
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|word| word.reverse()),
        }
    }
}

/// Usa o CRC32 e o tamanho registrados no diretório central do zip. Só aceita
/// arquivos com exatamente uma ROM de extensão conhecida. Não normaliza (SNES
/// com cabeçalho, N64 fora de ordem): esses casos ficam sem reconhecimento.
fn zip_rom(reader: impl Read + Seek) -> io::Result<Option<RomFile>> {
    let mut archive = zip::ZipArchive::new(reader).map_err(io::Error::other)?;
    let mut found: Option<RomFile> = None;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(io::Error::other)?;
        if !entry.is_file() {
            continue;
        }
        // Nome não decodificável não é uma ROM que saibamos tratar.
        let Ok(name) = entry.name() else { continue };
        let Some(extension) = lowercase_extension(Path::new(name.as_ref()))
            .filter(|ext| systems::for_extension(ext).is_some())
        else {
            continue;
        };
        if found.is_some() {
            return Ok(None);
        }
        found = Some(RomFile {
            fingerprint: Fingerprint {
                size: entry.size(),
                crc32: entry.crc32(),
            },
            extension,
            disc: None,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn crc(data: &[u8]) -> u32 {
        crc32fast::hash(data)
    }

    fn fp(data: &[u8], format: RomFormat) -> Fingerprint {
        stream_fingerprint(Cursor::new(data), data.len() as u64, format)
            .expect("leitura em memória")
    }

    /// ROM de N64 sintética, em big-endian, maior que um bloco de leitura.
    fn z64_rom() -> Vec<u8> {
        let mut rom = vec![0x80, 0x37, 0x12, 0x40];
        rom.extend((0..(CHUNK * 2 + 4) as u32).map(|i| (i * 31 % 251) as u8));
        rom
    }

    #[test]
    fn plain_rom_uses_raw_bytes() {
        let data = b"ROM de teste".repeat(100);
        assert_eq!(
            fp(&data, RomFormat::Plain),
            Fingerprint {
                size: data.len() as u64,
                crc32: crc(&data)
            }
        );
    }

    #[test]
    fn snes_strips_copier_header_only_when_present() {
        let rom = vec![0xAB; 1024 * 4];
        let mut headered = vec![0u8; 512];
        headered.extend(&rom);

        assert_eq!(fp(&headered, RomFormat::Snes), fp(&rom, RomFormat::Snes));
        assert_eq!(fp(&rom, RomFormat::Snes).size, rom.len() as u64);
    }

    #[test]
    fn n64_byte_orders_normalize_to_z64() {
        let z64 = z64_rom();
        let v64: Vec<u8> = z64.chunks(2).flat_map(|p| [p[1], p[0]]).collect();
        let n64: Vec<u8> = z64
            .chunks(4)
            .flat_map(|w| [w[3], w[2], w[1], w[0]])
            .collect();

        let expected = Fingerprint {
            size: z64.len() as u64,
            crc32: crc(&z64),
        };
        assert_eq!(fp(&z64, RomFormat::N64), expected);
        assert_eq!(fp(&v64, RomFormat::N64), expected);
        assert_eq!(fp(&n64, RomFormat::N64), expected);
    }

    #[test]
    fn reads_large_input_across_chunk_boundaries() {
        let data: Vec<u8> = (0..CHUNK * 3 + 17).map(|i| (i % 253) as u8).collect();
        assert_eq!(fp(&data, RomFormat::Plain).crc32, crc(&data));
    }

    fn zip_with(files: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in files {
            writer.start_file(*name, options).expect("entrada zip");
            writer.write_all(data).expect("escrita zip");
        }
        let mut cursor = writer.finish().expect("zip finalizado");
        cursor.set_position(0);
        cursor
    }

    #[test]
    fn zip_uses_central_directory_crc_of_the_single_rom() {
        let rom = b"conteudo da rom".repeat(10);
        let archive = zip_with(&[("leia-me.txt", b"x"), ("Jogo.gba", &rom)]);

        let result = zip_rom(archive).expect("zip válido");
        assert_eq!(
            result,
            Some(RomFile {
                fingerprint: Fingerprint {
                    size: rom.len() as u64,
                    crc32: crc(&rom)
                },
                extension: "gba".into(),
                disc: None,
            })
        );
    }

    #[test]
    fn zip_with_zero_or_many_roms_is_not_identified() {
        let none = zip_with(&[("leia-me.txt", b"x")]);
        let many = zip_with(&[("a.gba", b"a"), ("b.gba", b"b")]);

        assert_eq!(zip_rom(none).unwrap(), None);
        assert_eq!(zip_rom(many).unwrap(), None);
    }

    #[test]
    fn read_rom_normalizes_by_extension_and_skips_unknown_files() {
        let dir = tempfile::tempdir().expect("pasta");
        let rom = vec![0xAB; 1024 * 4];
        let mut headered = vec![0u8; 512];
        headered.extend(&rom);
        let sfc = dir.path().join("Jogo.SFC");
        std::fs::write(&sfc, &headered).expect("escrita");
        let txt = dir.path().join("notas.txt");
        std::fs::write(&txt, b"x").expect("escrita");

        let read = read_rom(&sfc).expect("leitura").expect("é ROM");
        assert_eq!(read.extension, "sfc");
        assert_eq!(read.fingerprint, fp(&rom, RomFormat::Plain));
        assert_eq!(read_rom(&txt).expect("leitura"), None);
    }

    #[test]
    fn invalid_zip_is_an_error_not_a_panic() {
        assert!(zip_rom(Cursor::new(b"nao e zip".to_vec())).is_err());
    }
}
