//! Imagem de disco: `.cue` com um ou mais `.bin` de setores brutos (2352
//! bytes), o formato dos dumps (Redump), ou um `.bin` sozinho de uma
//! trilha de dados.
//!
//! As posições são setores absolutos do disco (MSF em frames de 1/75 s):
//! a trilha 1 começa em 00:02:00, o setor 150. Os setores de pausa que não
//! estão no arquivo (`PREGAP`) são lidos como zeros.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Tamanho de um setor bruto.
pub(crate) const SECTOR_SIZE: usize = 2352;
/// Setor absoluto do começo da trilha 1 (00:02:00).
pub(crate) const FIRST_SECTOR: u32 = 150;
/// Fim do maior disco endereçável (99:59:74).
const MAX_SECTORS: u32 = 100 * 60 * 75;

#[derive(Debug)]
pub enum DiscError {
    /// Falha ao ler um arquivo da imagem.
    Io(PathBuf, io::Error),
    /// Linha inválida no `.cue` (número da linha, a partir de 1).
    Cue(usize, String),
    /// Formato de trilha que não é de setores brutos (ex.: `MODE1/2048`).
    UnsupportedTrack(String),
    /// O `.bin` não tem um número inteiro de setores de 2352 bytes.
    PartialSector(PathBuf),
    /// O `.cue` não tem trilhas.
    NoTracks,
}

impl fmt::Display for DiscError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(path, e) => write!(f, "{}: {e}", path.display()),
            Self::Cue(line, msg) => write!(f, "linha {line} do .cue: {msg}"),
            Self::UnsupportedTrack(kind) => write!(
                f,
                "trilha {kind} não suportada: use um dump com setores brutos (2352 bytes)"
            ),
            Self::PartialSector(path) => write!(
                f,
                "{}: o tamanho não é múltiplo de {SECTOR_SIZE} bytes",
                path.display()
            ),
            Self::NoTracks => f.write_str("o .cue não tem trilhas"),
        }
    }
}

impl std::error::Error for DiscError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackKind {
    Data,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Track {
    pub(crate) number: u8,
    pub(crate) kind: TrackKind,
    /// Começo da pausa (INDEX 00) e da trilha (INDEX 01).
    pub(crate) pregap: u32,
    pub(crate) start: u32,
    /// Fim (exclusivo): o começo da pausa da próxima trilha.
    pub(crate) end: u32,
    file: usize,
    /// Primeiro setor guardado no arquivo e a posição dele no arquivo.
    first_stored: u32,
    file_sector: u64,
}

enum Storage {
    File(Mutex<File>),
    #[cfg(test)]
    Memory(Vec<u8>),
}

impl Storage {
    fn read(&self, sector: u64, out: &mut [u8; SECTOR_SIZE]) -> io::Result<()> {
        match self {
            Self::File(file) => {
                let mut file = file
                    .lock()
                    .map_err(|_| io::Error::other("leitura anterior do disco falhou"))?;
                file.seek(SeekFrom::Start(sector * SECTOR_SIZE as u64))?;
                file.read_exact(out)
            }
            #[cfg(test)]
            Self::Memory(bytes) => {
                let at = sector as usize * SECTOR_SIZE;
                out.copy_from_slice(&bytes[at..at + SECTOR_SIZE]);
                Ok(())
            }
        }
    }
}

/// Disco no drive.
pub struct Disc {
    tracks: Vec<Track>,
    files: Vec<(Storage, u64)>,
    /// Letra da região na licença do setor 4 ('A', 'E' ou 'I'), se houver.
    region: Option<u8>,
}

impl fmt::Debug for Disc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Disc")
            .field("tracks", &self.tracks)
            .field("region", &self.region.map(char::from))
            .finish_non_exhaustive()
    }
}

/// Trilha como aparece no `.cue`, antes de virar posições no disco.
struct CueTrack {
    /// Linha do TRACK, para as mensagens de erro.
    line: usize,
    number: u8,
    kind: TrackKind,
    file: usize,
    /// Setores de `PREGAP`, que não estão no arquivo.
    pregap: u32,
    index0: Option<u32>,
    index1: Option<u32>,
}

impl Disc {
    /// Abre um `.cue` (com os `.bin` ao lado) ou um `.bin` de uma trilha.
    pub fn open(path: &Path) -> Result<Self, DiscError> {
        let is_cue = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cue"));
        if !is_cue {
            let storage = open_file(path)?;
            return Self::single_track(storage);
        }
        let text =
            std::fs::read_to_string(path).map_err(|e| DiscError::Io(path.to_path_buf(), e))?;
        let folder = path.parent().unwrap_or(Path::new(""));
        Self::from_cue(&text, |name| open_file(&folder.join(name)))
    }

    fn single_track(storage: (Storage, u64)) -> Result<Self, DiscError> {
        let mut disc = Self {
            tracks: vec![Track {
                number: 1,
                kind: TrackKind::Data,
                pregap: 0,
                start: FIRST_SECTOR,
                end: FIRST_SECTOR + storage.1 as u32,
                file: 0,
                first_stored: FIRST_SECTOR,
                file_sector: 0,
            }],
            files: vec![storage],
            region: None,
        };
        disc.region = disc.find_region();
        Ok(disc)
    }

    fn from_cue(
        text: &str,
        mut open: impl FnMut(&str) -> Result<(Storage, u64), DiscError>,
    ) -> Result<Self, DiscError> {
        let mut files = Vec::new();
        let mut cue_tracks: Vec<CueTrack> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let error = |msg: &str| DiscError::Cue(n + 1, msg.to_owned());
            let line = line.trim();
            let (command, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let rest = rest.trim();
            match command.to_ascii_uppercase().as_str() {
                "FILE" => {
                    let (name, kind) = file_name(rest).ok_or_else(|| error("FILE sem nome"))?;
                    if !kind.eq_ignore_ascii_case("BINARY") {
                        return Err(error("só arquivos BINARY são suportados"));
                    }
                    files.push(open(name)?);
                }
                "TRACK" => {
                    let file = files
                        .len()
                        .checked_sub(1)
                        .ok_or_else(|| error("TRACK antes de FILE"))?;
                    let (number, kind) = rest
                        .split_once(char::is_whitespace)
                        .ok_or_else(|| error("TRACK sem tipo"))?;
                    let number = number
                        .parse()
                        .ok()
                        .filter(|n| (1..=99).contains(n))
                        .ok_or_else(|| error("número de trilha inválido"))?;
                    let kind = match kind.trim().to_ascii_uppercase().as_str() {
                        "MODE2/2352" | "MODE1/2352" => TrackKind::Data,
                        "AUDIO" => TrackKind::Audio,
                        other => return Err(DiscError::UnsupportedTrack(other.to_owned())),
                    };
                    cue_tracks.push(CueTrack {
                        line: n + 1,
                        number,
                        kind,
                        file,
                        pregap: 0,
                        index0: None,
                        index1: None,
                    });
                }
                "INDEX" | "PREGAP" => {
                    let track = cue_tracks
                        .last_mut()
                        .ok_or_else(|| error("INDEX antes de TRACK"))?;
                    let (index, time) = if command.eq_ignore_ascii_case("PREGAP") {
                        (None, rest)
                    } else {
                        let (index, time) = rest
                            .split_once(char::is_whitespace)
                            .ok_or_else(|| error("INDEX sem tempo"))?;
                        (Some(index), time.trim())
                    };
                    let sectors = parse_msf(time).ok_or_else(|| error("tempo inválido"))?;
                    match index {
                        None => track.pregap = sectors,
                        Some("00" | "0") => track.index0 = Some(sectors),
                        Some("01" | "1") => track.index1 = Some(sectors),
                        // Índices 02+ não mudam onde as trilhas começam.
                        Some(_) => {}
                    }
                }
                _ => {}
            }
        }
        Self::layout(&cue_tracks, files)
    }

    /// Converte os tempos do `.cue` (relativos a cada arquivo) em posições
    /// absolutas no disco.
    fn layout(cue_tracks: &[CueTrack], files: Vec<(Storage, u64)>) -> Result<Self, DiscError> {
        if cue_tracks.is_empty() {
            return Err(DiscError::NoTracks);
        }
        // Onde cada arquivo começa, sem os PREGAPs.
        let mut file_starts = Vec::with_capacity(files.len());
        let mut at = FIRST_SECTOR;
        for (_, sectors) in &files {
            file_starts.push(at);
            at = at.saturating_add(*sectors as u32);
        }
        let mut shift = 0;
        let mut tracks: Vec<Track> = Vec::with_capacity(cue_tracks.len());
        for (i, t) in cue_tracks.iter().enumerate() {
            let index1 = t
                .index1
                .ok_or_else(|| DiscError::Cue(t.line, "a trilha não tem INDEX 01".into()))?;
            shift += t.pregap;
            let base = file_starts[t.file] + shift;
            let first_in_file = t.index0.unwrap_or(index1);
            let start = base + index1;
            let pregap = if i == 0 {
                0
            } else {
                t.index0.map_or(start - t.pregap, |i0| base + i0)
            };
            if let Some(previous) = tracks.last_mut() {
                previous.end = pregap;
            }
            tracks.push(Track {
                number: t.number,
                kind: t.kind,
                pregap,
                start,
                end: start,
                file: t.file,
                first_stored: base + first_in_file,
                file_sector: u64::from(first_in_file),
            });
        }
        let last = tracks.last_mut().expect("há trilhas");
        last.end = file_starts[last.file] + shift + files[last.file].1 as u32;
        let mut disc = Self {
            tracks,
            files,
            region: None,
        };
        disc.region = disc.find_region();
        Ok(disc)
    }

    /// Região da licença gravada no setor 4 da trilha 1, que o drive
    /// devolve no GetID.
    fn find_region(&self) -> Option<u8> {
        const LICENSE: &[u8] = b"Sony Computer Entertainment ";
        let mut sector = [0; SECTOR_SIZE];
        self.read(FIRST_SECTOR + 4, &mut sector).ok()?;
        let at = sector.windows(LICENSE.len()).position(|w| w == LICENSE)?;
        let rest = &sector[at + LICENSE.len()..];
        [(b"Amer", b'A'), (b"Euro", b'E'), (b"Inc.", b'I')]
            .into_iter()
            .find(|(name, _)| rest.starts_with(*name))
            .map(|(_, region)| region)
    }

    pub(crate) fn region(&self) -> Option<u8> {
        self.region
    }

    pub(crate) fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Começo da área de saída (lead-out), depois da última trilha.
    pub(crate) fn end(&self) -> u32 {
        self.tracks.last().map_or(FIRST_SECTOR, |t| t.end)
    }

    pub(crate) fn track_at(&self, sector: u32) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|t| (t.pregap..t.end).contains(&sector))
    }

    /// Lê o setor absoluto; pausas fora do arquivo e o lead-in vêm zerados.
    pub(crate) fn read(&self, sector: u32, out: &mut [u8; SECTOR_SIZE]) -> io::Result<()> {
        let stored = self
            .track_at(sector)
            .filter(|t| sector >= t.first_stored)
            .map(|t| (t.file, t.file_sector + u64::from(sector - t.first_stored)))
            .filter(|&(file, at)| at < self.files[file].1);
        match stored {
            Some((file, at)) => self.files[file].0.read(at, out),
            None => {
                out.fill(0);
                Ok(())
            }
        }
    }
}

fn open_file(path: &Path) -> Result<(Storage, u64), DiscError> {
    let file = File::open(path).map_err(|e| DiscError::Io(path.to_path_buf(), e))?;
    let len = file
        .metadata()
        .map_err(|e| DiscError::Io(path.to_path_buf(), e))?
        .len();
    if len % SECTOR_SIZE as u64 != 0 || len / SECTOR_SIZE as u64 >= u64::from(MAX_SECTORS) {
        return Err(DiscError::PartialSector(path.to_path_buf()));
    }
    Ok((Storage::File(Mutex::new(file)), len / SECTOR_SIZE as u64))
}

/// `"nome com espaços.bin" BINARY` ou `nome.bin BINARY`.
fn file_name(rest: &str) -> Option<(&str, &str)> {
    if let Some(quoted) = rest.strip_prefix('"') {
        let (name, kind) = quoted.split_once('"')?;
        Some((name, kind.trim()))
    } else {
        let (name, kind) = rest.rsplit_once(char::is_whitespace)?;
        Some((name.trim(), kind.trim()))
    }
}

/// `mm:ss:ff` em setores.
fn parse_msf(time: &str) -> Option<u32> {
    let mut parts = time.split(':').map(|p| p.parse::<u32>().ok());
    let (m, s, f) = (parts.next()??, parts.next()??, parts.next()??);
    (parts.next().is_none() && s < 60 && f < 75).then_some((m * 60 + s) * 75 + f)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Setor de dados modo 2 com o cabeçalho de `sector` e um byte marcador.
    pub(crate) fn data_sector(sector: u32, marker: u8) -> Vec<u8> {
        let mut bytes = vec![0; SECTOR_SIZE];
        bytes[1..11].fill(0xFF);
        let msf = [sector / 75 / 60, sector / 75 % 60, sector % 75];
        for (i, v) in msf.into_iter().enumerate() {
            bytes[12 + i] = (v / 10 * 16 + v % 10) as u8;
        }
        bytes[15] = 2;
        bytes[24] = marker;
        bytes[SECTOR_SIZE - 1] = marker;
        bytes
    }

    /// Disco de uma trilha de dados com `count` setores marcados pelo
    /// índice (e a licença americana no setor 4).
    pub(crate) fn data_disc(count: u32) -> Disc {
        let mut bytes = Vec::new();
        for i in 0..count {
            let mut sector = data_sector(FIRST_SECTOR + i, i as u8);
            if i == 4 {
                let text = b"Licensed by Sony Computer Entertainment Amer  ica";
                sector[24..24 + text.len()].copy_from_slice(text);
            }
            bytes.extend(sector);
        }
        Disc::single_track((Storage::Memory(bytes), u64::from(count))).expect("disco")
    }

    /// Disco de uma trilha de dados com os setores dados.
    pub(crate) fn disc_with(sectors: &[[u8; SECTOR_SIZE]]) -> Disc {
        let bytes = sectors.concat();
        Disc::single_track((Storage::Memory(bytes), sectors.len() as u64)).expect("disco")
    }

    /// Trilha 1 de dados com `data` setores e as trilhas de áudio dadas
    /// (bytes crus, sem pausa entre elas).
    pub(crate) fn audio_disc(data: u32, audio: &[Vec<u8>]) -> Disc {
        let mut cue =
            String::from("FILE data.bin BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:00:00\n");
        for (i, _) in audio.iter().enumerate() {
            cue += &format!(
                "FILE {i}.bin BINARY\nTRACK {:02} AUDIO\nINDEX 01 00:00:00\n",
                i + 2
            );
        }
        let mut files = std::iter::once(
            (0..data)
                .flat_map(|i| data_sector(FIRST_SECTOR + i, 0))
                .collect(),
        )
        .chain(audio.iter().cloned());
        Disc::from_cue(&cue, |_| {
            let bytes: Vec<u8> = files.next().expect("arquivo");
            let sectors = (bytes.len() / SECTOR_SIZE) as u64;
            Ok((Storage::Memory(bytes), sectors))
        })
        .expect("cue válido")
    }

    fn memory(sectors: u32, fill: u8) -> (Storage, u64) {
        let bytes = vec![fill; sectors as usize * SECTOR_SIZE];
        (Storage::Memory(bytes), u64::from(sectors))
    }

    #[test]
    fn multi_file_cue_with_index_00_and_pregap() {
        let cue = r#"
            FILE "Game (Track 1).bin" BINARY
              TRACK 01 MODE2/2352
                INDEX 01 00:00:00
            FILE "Game (Track 2).bin" BINARY
              TRACK 02 AUDIO
                INDEX 00 00:00:00
                INDEX 01 00:02:00
            FILE track3.bin BINARY
              TRACK 03 AUDIO
                PREGAP 00:02:00
                INDEX 01 00:00:00
        "#;
        let sizes = [(1000, 1), (500, 2), (300, 3)];
        let mut next = sizes.iter();
        let disc = Disc::from_cue(cue, |_| {
            let &(sectors, fill) = next.next().expect("3 arquivos");
            Ok(memory(sectors, fill))
        })
        .expect("cue válido");
        let t: Vec<_> = disc
            .tracks()
            .iter()
            .map(|t| (t.number, t.kind, t.pregap, t.start, t.end))
            .collect();
        assert_eq!(
            t,
            [
                (1, TrackKind::Data, 0, 150, 1150),
                (2, TrackKind::Audio, 1150, 1300, 1650),
                (3, TrackKind::Audio, 1650, 1800, 2100),
            ]
        );
        let mut sector = [0; SECTOR_SIZE];
        let mut byte_at = |at| {
            disc.read(at, &mut sector).expect("leitura");
            sector[0]
        };
        assert_eq!(byte_at(1149), 1);
        assert_eq!(byte_at(1150), 2, "pausa da trilha 2 está no arquivo");
        assert_eq!(byte_at(1700), 0, "PREGAP não está no arquivo");
        assert_eq!(byte_at(1800), 3);
        assert_eq!(byte_at(2099), 3);
        assert_eq!(byte_at(2100), 0, "lead-out");
        assert_eq!(byte_at(10), 0, "lead-in");
    }

    #[test]
    fn invalid_cues_are_refused() {
        let ok = |_: &str| Ok(memory(10, 0));
        assert!(matches!(
            Disc::from_cue("TRACK 01 MODE2/2352", ok),
            Err(DiscError::Cue(1, _))
        ));
        assert!(matches!(
            Disc::from_cue(
                "FILE a.iso BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00",
                ok
            ),
            Err(DiscError::UnsupportedTrack(_))
        ));
        assert!(matches!(
            Disc::from_cue(
                "FILE a.bin BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:61:00",
                ok
            ),
            Err(DiscError::Cue(3, _))
        ));
        assert!(matches!(
            Disc::from_cue("FILE a.bin BINARY", ok),
            Err(DiscError::NoTracks)
        ));
    }

    #[test]
    fn region_comes_from_the_license_sector() {
        assert_eq!(data_disc(8).region(), Some(b'A'));
        assert_eq!(data_disc(3).region(), None, "setor 4 fora do disco");
    }

    #[test]
    fn bin_with_partial_sector_is_refused() {
        let dir = std::env::temp_dir().join(format!("psx-disc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        let bin = dir.join("partial.bin");
        std::fs::write(&bin, vec![0; SECTOR_SIZE + 1]).expect("escrita");
        assert!(matches!(Disc::open(&bin), Err(DiscError::PartialSector(_))));
        std::fs::write(&bin, data_sector(FIRST_SECTOR, 7)).expect("escrita");
        let cue = dir.join("game.cue");
        std::fs::write(
            &cue,
            "FILE \"partial.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        )
        .expect("escrita");
        let disc = Disc::open(&cue).expect("abre");
        let mut sector = [0; SECTOR_SIZE];
        disc.read(FIRST_SECTOR, &mut sector).expect("leitura");
        assert_eq!(sector[24], 7);
        assert!(matches!(
            Disc::open(&dir.join("missing.cue")),
            Err(DiscError::Io(..))
        ));
        std::fs::remove_dir_all(&dir).expect("limpeza");
    }
}
