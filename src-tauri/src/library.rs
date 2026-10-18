//! Varredura da pasta da biblioteca e identificação dos jogos.
//!
//! Uma única pasta (com subpastas) guarda os jogos de todos os consoles. Cada
//! arquivo é identificado pelos DATs; o console vem do DAT que o reconheceu,
//! ou da extensão quando ela é exclusiva de um sistema. Discos são
//! reconhecidos pelo número de série no DAT Redump do sistema.
//!
//! Calcular o CRC de ROMs grandes custa caro, então o resultado fica em um
//! índice persistente chaveado por caminho, tamanho e data de modificação:
//! arquivos inalterados não são relidos.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::identify::{self, Fingerprint, RomFile};
use crate::systems;

/// Versão do formato do índice. Mudanças na forma de identificar exigem
/// incrementá-la, para descartar valores antigos incompatíveis.
const CACHE_VERSION: u32 = 4;

/// Extensões usadas também por arquivos que não são jogos. Sem
/// reconhecimento no DAT, ficam fora da lista.
const AMBIGUOUS_EXTENSIONS: &[&str] = &["bin", "zip"];

/// Maior `.bin` lido. Nenhum jogo de Mega Drive passa de alguns MB; o limite
/// evita ler arquivos enormes que só compartilham a extensão.
const MAX_BIN_BYTES: u64 = 16 * 1024 * 1024;

/// Um jogo encontrado na biblioteca.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    /// Estável na biblioteca: caminho relativo à pasta, com `/`.
    pub id: String,
    pub console_id: String,
    /// Nome No-Intro (ou Redump, em discos) quando reconhecido; senão, o nome
    /// do arquivo sem extensão.
    pub title: String,
    pub file_name: String,
    pub recognized: bool,
    /// Há progresso salvo para continuar (preenchido pelo launcher).
    pub has_save: bool,
    /// Há save do próprio jogo (`.sav`) para exportar (preenchido pelo
    /// launcher).
    pub has_game_save: bool,
    #[serde(skip)]
    pub path: PathBuf,
    /// Nome No-Intro (ou Redump, em discos), usado para localizar a capa.
    #[serde(skip)]
    pub no_intro_name: Option<String>,
}

/// Resultado de consultar uma ROM nos DATs: console e nome No-Intro.
pub type Identification = (&'static str, String);

/// Índice persistente de ROMs já lidas.
#[derive(Debug, Serialize, Deserialize)]
pub struct FingerprintCache {
    #[serde(default)]
    version: u32,
    entries: HashMap<PathBuf, CachedRom>,
}

impl Default for FingerprintCache {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CachedRom {
    size: u64,
    modified_nanos: u128,
    rom: Option<RomFile>,
}

impl FingerprintCache {
    /// Carrega o índice salvo. Arquivo ausente, corrompido ou de outra versão
    /// vira índice vazio: o pior caso é recalcular os CRCs.
    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|cache| cache.version == CACHE_VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Escreve em arquivo temporário e renomeia: uma falha no meio não
        // deixa o índice truncado.
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(self).map_err(io::Error::other)?)?;
        fs::rename(tmp, path)
    }

    fn get_or_read(
        &mut self,
        path: &Path,
        read: impl FnOnce(u64) -> io::Result<Option<RomFile>>,
    ) -> io::Result<Option<RomFile>> {
        let metadata = fs::metadata(path)?;
        let size = metadata.len();
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());

        if let Some(cached) = self.entries.get(path)
            && cached.size == size
            && cached.modified_nanos == modified_nanos
        {
            return Ok(cached.rom.clone());
        }
        let rom = read(size)?;
        self.entries.insert(
            path.to_owned(),
            CachedRom {
                size,
                modified_nanos,
                rom: rom.clone(),
            },
        );
        Ok(rom)
    }

    /// Mantém só entradas de arquivos encontrados nesta varredura.
    fn retain(&mut self, existing: &[PathBuf]) {
        let existing: HashSet<&PathBuf> = existing.iter().collect();
        self.entries.retain(|path, _| existing.contains(path));
    }
}

/// Lista e identifica os jogos em `root` e subpastas, ordenados pelo título.
///
/// `identify` consulta os DATs. Arquivos ilegíveis são registrados e
/// ignorados: um arquivo ruim não esconde o resto da biblioteca.
pub fn scan(
    root: &Path,
    cache: &mut FingerprintCache,
    identify: impl Fn(Fingerprint) -> Option<Identification>,
) -> io::Result<Vec<Game>> {
    let mut paths = Vec::new();
    collect_candidates(root, &mut paths)?;
    paths.sort();
    cache.retain(&paths);

    let mut games: Vec<Game> = paths
        .into_iter()
        .filter_map(|path| {
            let rom = cache
                .get_or_read(&path, |size| {
                    let is_big_bin = identify::lowercase_extension(&path).as_deref() == Some("bin")
                        && size > MAX_BIN_BYTES;
                    if is_big_bin {
                        Ok(None)
                    } else {
                        identify::read_rom(&path)
                    }
                })
                .unwrap_or_else(|error| {
                    eprintln!("não foi possível ler {}: {error}", path.display());
                    None
                })?;
            game_for(root, path, &rom, &identify)
        })
        .collect();
    games.sort_by_key(|game| game.title.to_lowercase());
    Ok(games)
}

/// Junta os arquivos com extensão de ROM (ou `.zip`), recursivamente. Ignora
/// itens ocultos e não segue links simbólicos, evitando ciclos.
fn collect_candidates(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let hidden = entry
            .file_name()
            .to_str()
            .is_none_or(|name| name.starts_with('.'));
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if hidden || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            // Subpasta inacessível não interrompe a varredura do resto.
            if let Err(error) = collect_candidates(&path, out) {
                eprintln!("não foi possível ler {}: {error}", path.display());
            }
        } else if file_type.is_file() {
            let candidate = identify::lowercase_extension(&path)
                .is_some_and(|ext| ext == "zip" || systems::for_extension(&ext).is_some());
            if candidate {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn game_for(
    root: &Path,
    path: PathBuf,
    rom: &RomFile,
    identify: impl Fn(Fingerprint) -> Option<Identification>,
) -> Option<Game> {
    // Vale a extensão do próprio arquivo (`.zip`) e a da ROM dentro dele.
    let ambiguous = [
        identify::lowercase_extension(&path),
        Some(rom.extension.clone()),
    ]
    .iter()
    .flatten()
    .any(|ext| AMBIGUOUS_EXTENSIONS.contains(&ext.as_str()));
    let (console_id, no_intro_name) = match identify(rom.fingerprint) {
        Some((console_id, name)) => (console_id, Some(name)),
        None if ambiguous => return None,
        None => {
            let system = systems::for_extension(&rom.extension)?;
            let disc_name = rom
                .disc
                .as_ref()
                .and_then(|disc| system.disc_name(&disc.serial, disc.first_file_size));
            (system.console_id, disc_name.map(str::to_owned))
        }
    };
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.clone());
    let id = path
        .strip_prefix(root)
        .unwrap_or(&path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");

    Some(Game {
        id,
        console_id: console_id.to_owned(),
        title: no_intro_name.clone().unwrap_or(stem),
        file_name,
        recognized: no_intro_name.is_some(),
        has_save: false,
        has_game_save: false,
        path,
        no_intro_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn write(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("pasta");
        }
        File::create(&path)
            .and_then(|mut file| file.write_all(data))
            .expect("arquivo de teste");
        path
    }

    fn write_zip(dir: &Path, name: &str, inner: &str, data: &[u8]) {
        let mut zip = zip::ZipWriter::new(File::create(dir.join(name)).expect("zip"));
        zip.start_file(
            inner,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .expect("entrada");
        zip.write_all(data).expect("escrita");
        zip.finish().expect("zip finalizado");
    }

    /// DAT de teste: reconhece exatamente as ROMs dadas.
    fn dat(
        known: &[(&'static str, &'static str, &[u8])],
    ) -> impl Fn(Fingerprint) -> Option<Identification> {
        let known: Vec<(Fingerprint, &'static str, String)> = known
            .iter()
            .map(|(console, name, data)| {
                (
                    Fingerprint {
                        size: data.len() as u64,
                        crc32: crc32fast::hash(data),
                    },
                    *console,
                    (*name).to_owned(),
                )
            })
            .collect();
        move |fp| {
            known
                .iter()
                .find(|(k, _, _)| *k == fp)
                .map(|(_, console, name)| (*console, name.clone()))
        }
    }

    fn summary(games: &[Game]) -> Vec<(&str, &str, &str, bool)> {
        games
            .iter()
            .map(|g| {
                (
                    g.id.as_str(),
                    g.console_id.as_str(),
                    g.title.as_str(),
                    g.recognized,
                )
            })
            .collect()
    }

    const GBA_ROM: &[u8] = b"rom de gba conhecida";
    const SNES_ROM: &[u8] = b"rom de snes conhecida";

    #[test]
    fn assigns_each_game_to_its_console_across_subfolders() {
        let dir = tempfile::tempdir().expect("pasta");
        write(dir.path(), "Aria.gba", GBA_ROM);
        write(dir.path(), "snes/sub/Mario.sfc", SNES_ROM);
        write(dir.path(), "caseiro.nds", b"homebrew");
        write(dir.path(), "notas.txt", b"ignorar");
        write(dir.path(), ".oculta/Escondido.gba", GBA_ROM);

        let games = scan(
            dir.path(),
            &mut FingerprintCache::default(),
            dat(&[
                ("game-boy-advance", "Castlevania (USA)", GBA_ROM),
                ("snes", "Super Mario World (USA)", SNES_ROM),
            ]),
        )
        .expect("varredura");

        assert_eq!(
            summary(&games),
            [
                ("caseiro.nds", "nintendo-ds", "caseiro", false),
                ("Aria.gba", "game-boy-advance", "Castlevania (USA)", true),
                (
                    "snes/sub/Mario.sfc",
                    "snes",
                    "Super Mario World (USA)",
                    true
                ),
            ]
        );
    }

    #[test]
    fn dat_wins_over_a_wrong_extension() {
        let dir = tempfile::tempdir().expect("pasta");
        write(dir.path(), "renomeado.gb", GBA_ROM);

        let games = scan(
            dir.path(),
            &mut FingerprintCache::default(),
            dat(&[("game-boy-advance", "Castlevania (USA)", GBA_ROM)]),
        )
        .expect("varredura");

        assert_eq!(games[0].console_id, "game-boy-advance");
    }

    #[test]
    fn zips_count_only_when_recognized() {
        let dir = tempfile::tempdir().expect("pasta");
        write_zip(dir.path(), "Aria.zip", "Aria.gba", GBA_ROM);
        write_zip(dir.path(), "desconhecido.zip", "x.gba", b"outra coisa");
        write_zip(dir.path(), "documentos.zip", "leia-me.txt", b"texto");

        let games = scan(
            dir.path(),
            &mut FingerprintCache::default(),
            dat(&[("game-boy-advance", "Castlevania (USA)", GBA_ROM)]),
        )
        .expect("varredura");

        assert_eq!(
            summary(&games),
            [("Aria.zip", "game-boy-advance", "Castlevania (USA)", true)]
        );
    }

    #[test]
    fn unrecognized_bin_files_are_left_out() {
        let dir = tempfile::tempdir().expect("pasta");
        write(dir.path(), "firmware.bin", b"nao e jogo");

        let games =
            scan(dir.path(), &mut FingerprintCache::default(), dat(&[])).expect("varredura");
        assert!(games.is_empty());
    }

    #[test]
    fn playstation_discs_are_listed_by_their_cue_sheet() {
        let dir = tempfile::tempdir().expect("pasta");
        let disc = dir.path().join("PS1/Crash Bandicoot (USA)");
        std::fs::create_dir_all(&disc).expect("pasta");
        write(
            &disc,
            "Crash Bandicoot (USA).cue",
            b"FILE \"Crash Bandicoot (USA).bin\" BINARY",
        );
        write(&disc, "Crash Bandicoot (USA).bin", &[0; 2352]);
        let games =
            scan(dir.path(), &mut FingerprintCache::default(), dat(&[])).expect("varredura");
        assert_eq!(games.len(), 1, "o .bin do disco não é outro jogo");
        let crash = &games[0];
        assert_eq!(crash.console_id, "playstation");
        assert_eq!(crash.title, "Crash Bandicoot (USA)");
        assert_eq!(
            crash.id,
            "PS1/Crash Bandicoot (USA)/Crash Bandicoot (USA).cue"
        );
        assert!(!crash.recognized);
    }

    /// Trilha 1 de um disco ISO 9660 cujo `SYSTEM.CNF` tem `boot`.
    fn disc_image(boot: &str) -> Vec<u8> {
        const SECTOR: usize = 2352;
        const DATA: usize = 24;
        let mut image = vec![0; 21 * SECTOR];
        let mut put = |lba: usize, at: usize, bytes: &[u8]| {
            let start = lba * SECTOR + DATA + at;
            image[start..start + bytes.len()].copy_from_slice(bytes);
        };
        let record = |name: &[u8], lba: u32, size: u32, flags: u8| {
            let mut r = vec![0; 34 + name.len()];
            r[0] = r.len() as u8;
            r[2..6].copy_from_slice(&lba.to_le_bytes());
            r[10..14].copy_from_slice(&size.to_le_bytes());
            r[25] = flags;
            r[32] = name.len() as u8;
            r[33..33 + name.len()].copy_from_slice(name);
            r
        };
        put(16, 0, b"\x01CD001");
        put(16, 156, &record(b"\0", 18, 2048, 2));
        put(18, 0, &record(b"SYSTEM.CNF;1", 20, boot.len() as u32, 0));
        put(20, 0, boot.as_bytes());
        image
    }

    #[test]
    fn playstation_discs_are_recognized_by_serial() {
        let dir = tempfile::tempdir().expect("pasta");
        write(
            dir.path(),
            "meu crash.cue",
            b"FILE \"meu crash.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        );
        write(
            dir.path(),
            "meu crash.bin",
            &disc_image("BOOT = cdrom:\\SCUS_949.00;1\r\nTCB = 4\r\n"),
        );
        write(
            dir.path(),
            "caseiro.cue",
            b"FILE \"caseiro.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        );
        write(
            dir.path(),
            "caseiro.bin",
            &disc_image("BOOT = cdrom:\\SLUS_999.99;1\r\n"),
        );
        let mut cache = FingerprintCache::default();
        let games = scan(dir.path(), &mut cache, dat(&[])).expect("varredura");

        assert_eq!(
            summary(&games),
            [
                ("caseiro.cue", "playstation", "caseiro", false),
                (
                    "meu crash.cue",
                    "playstation",
                    "Crash Bandicoot (USA)",
                    true
                ),
            ]
        );
        assert_eq!(
            games[1].no_intro_name.as_deref(),
            Some("Crash Bandicoot (USA)"),
            "a capa vem do nome Redump, não do arquivo"
        );
        // O número de série fica no índice: a próxima varredura não relê.
        let again = scan(dir.path(), &mut cache, dat(&[])).expect("varredura");
        assert_eq!(again, games);
    }

    #[test]
    fn missing_folder_is_an_error() {
        let result = scan(
            Path::new("/caminho/que/nao/existe"),
            &mut FingerprintCache::default(),
            dat(&[]),
        );
        assert!(result.is_err());
    }

    #[test]
    fn cache_reuses_reads_until_file_changes() {
        let dir = tempfile::tempdir().expect("pasta");
        let path = write(dir.path(), "a.gb", b"versao 1");
        let mut cache = FingerprintCache::default();
        let mut reads = 0;
        let mut read = |cache: &mut FingerprintCache| {
            cache
                .get_or_read(&path, |_| {
                    reads += 1;
                    identify::read_rom(&path)
                })
                .expect("leitura")
        };

        let first = read(&mut cache);
        let second = read(&mut cache);
        assert_eq!(first, second);

        // Tamanho diferente invalida a entrada, mesmo que o mtime coincida.
        write(dir.path(), "a.gb", b"versao 2 maior");
        let third = read(&mut cache);
        assert_ne!(third, first);
        assert_eq!(reads, 2);
    }

    #[test]
    fn cache_round_trips_and_discards_corrupt_or_old_files() {
        let dir = tempfile::tempdir().expect("pasta");
        let rom = write(dir.path(), "a.gb", b"rom");
        let index_path = dir.path().join("index/rom-index.json");

        let mut cache = FingerprintCache::default();
        cache
            .get_or_read(&rom, |_| identify::read_rom(&rom))
            .expect("leitura");
        cache.save(&index_path).expect("salvar índice");
        assert_eq!(FingerprintCache::load(&index_path).entries, cache.entries);

        fs::write(&index_path, b"{ corrompido").expect("corromper");
        assert!(FingerprintCache::load(&index_path).entries.is_empty());

        fs::write(
            &index_path,
            br#"{"version":2,"entries":{"/a.gb":{"size":1,"modified_nanos":1,"rom":null}}}"#,
        )
        .expect("versão antiga");
        assert!(FingerprintCache::load(&index_path).entries.is_empty());
    }

    #[test]
    fn scan_drops_cache_entries_of_deleted_files() {
        let dir = tempfile::tempdir().expect("pasta");
        let path = write(dir.path(), "a.gb", b"rom");
        let mut cache = FingerprintCache::default();

        scan(dir.path(), &mut cache, dat(&[])).expect("varredura");
        assert!(cache.entries.contains_key(&path));

        fs::remove_file(&path).expect("remover");
        scan(dir.path(), &mut cache, dat(&[])).expect("varredura");
        assert!(cache.entries.is_empty());
    }
}
