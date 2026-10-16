//! Onde fica o estado salvo de cada jogo ("continuar de onde parou") e o
//! save do próprio jogo (RAM com bateria do cartucho, `.sav`; no
//! PlayStation, o memory card, `.mcr`).
//!
//! Um arquivo por jogo em `<dados do app>/saves/<console>/`. O nome mistura
//! uma versão legível do caminho do jogo na biblioteca com um hash desse
//! caminho, para ser único mesmo quando dois jogos têm o mesmo nome de
//! arquivo em pastas diferentes. O conteúdo é opaco aqui: quem lê e grava é
//! o processo do jogo, exceto ao exportar e importar o save do jogo.

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::library::Game;

/// Limite do trecho legível do nome, para não estourar o limite de
/// caminho de nenhum sistema.
const MAX_STEM_CHARS: usize = 80;

/// Maior save importado. Os maiores saves dos consoles suportados são a RAM
/// do Game Boy e o memory card do PlayStation (128 KB); o limite só evita
/// ler arquivos que não são saves.
const MAX_IMPORT_BYTES: u64 = 512 * 1024;

/// Falha ao exportar ou importar o save do jogo. Em nenhum caso o save
/// existente é alterado.
#[derive(Debug)]
pub enum TransferError {
    /// O jogo ainda não tem save para exportar.
    NoSave,
    /// O arquivo escolhido é grande demais para ser um save.
    TooLarge(u64),
    /// O conteúdo não serve para este jogo (motivo legível).
    Rejected(String),
    Io(io::Error),
}

impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSave => f.write_str("este jogo ainda não tem save"),
            Self::TooLarge(size) => {
                write!(f, "o arquivo tem {size} bytes; grande demais para um save")
            }
            Self::Rejected(reason) => f.write_str(reason),
            Self::Io(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for TransferError {}

pub struct SaveStore {
    dir: PathBuf,
}

impl SaveStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn path_for(&self, game: &Game) -> PathBuf {
        self.dir
            .join(sanitize(&game.console_id))
            .join(file_name(&game.id, "state"))
    }

    /// Save do próprio jogo, ao lado do estado. Nunca é apagado pelo
    /// "Recomeçar do início": é do jogador, como num cartucho de verdade.
    /// No PlayStation é um memory card por jogo, no formato bruto `.mcr`.
    pub fn battery_path_for(&self, game: &Game) -> PathBuf {
        let extension = match game.console_id.as_str() {
            "playstation" => "mcr",
            _ => "sav",
        };
        self.dir
            .join(sanitize(&game.console_id))
            .join(file_name(&game.id, extension))
    }

    pub fn exists(&self, game: &Game) -> bool {
        self.path_for(game).is_file()
    }

    /// Há save do próprio jogo.
    pub fn battery_exists(&self, game: &Game) -> bool {
        self.battery_path_for(game).is_file()
    }

    /// Copia o save do jogo, como está (formato bruto do cartucho), para
    /// `destination`.
    pub fn export_battery(&self, game: &Game, destination: &Path) -> Result<(), TransferError> {
        let bytes = fs::read(self.battery_path_for(game)).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => TransferError::NoSave,
            _ => TransferError::Io(e),
        })?;
        write_atomic(destination, &bytes).map_err(TransferError::Io)
    }

    /// Instala `source` como save do jogo, se `validate` aceitar o
    /// conteúdo. Recusado ou com falha, o save existente fica intacto.
    pub fn import_battery(
        &self,
        game: &Game,
        source: &Path,
        validate: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<(), TransferError> {
        let file = fs::File::open(source).map_err(TransferError::Io)?;
        let size = file.metadata().map_err(TransferError::Io)?.len();
        if size > MAX_IMPORT_BYTES {
            return Err(TransferError::TooLarge(size));
        }
        let mut bytes = Vec::new();
        // O tamanho pode mudar entre `metadata` e a leitura; `take` mantém
        // o limite mesmo assim.
        file.take(MAX_IMPORT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(TransferError::Io)?;
        if bytes.len() as u64 > MAX_IMPORT_BYTES {
            return Err(TransferError::TooLarge(bytes.len() as u64));
        }
        validate(&bytes).map_err(TransferError::Rejected)?;
        write_atomic(&self.battery_path_for(game), &bytes).map_err(TransferError::Io)
    }

    /// Apaga o estado salvo (não o save do jogo); não ter estado nenhum não
    /// é erro.
    pub fn delete(&self, game: &Game) -> io::Result<()> {
        match fs::remove_file(self.path_for(game)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

/// Grava num arquivo temporário ao lado e renomeia por cima: quem lê
/// `path` vê o arquivo antigo ou o novo inteiro, nunca um pela metade.
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

fn file_name(game_id: &str, extension: &str) -> String {
    let stem: String = sanitize(game_id).chars().take(MAX_STEM_CHARS).collect();
    format!("{stem}-{:016x}.{extension}", fnv1a(game_id.as_bytes()))
}

/// Mantém letras, dígitos e pontuação segura em todos os sistemas.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str) -> Game {
        Game {
            id: id.to_owned(),
            console_id: "master-system".to_owned(),
            title: "Teste".to_owned(),
            file_name: "x.sms".to_owned(),
            recognized: false,
            has_save: false,
            has_game_save: false,
            path: PathBuf::from("/Jogos").join(id),
            no_intro_name: None,
        }
    }

    #[test]
    fn path_is_per_console_and_readable() {
        let store = SaveStore::new(PathBuf::from("/dados/saves"));
        let path = store.path_for(&game("SMS/Sonic (USA).sms"));
        assert!(path.starts_with("/dados/saves/master-system"));
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        assert!(name.starts_with("SMS_Sonic (USA).sms-"), "{name}");
        assert!(name.ends_with(".state"));
    }

    #[test]
    fn playstation_game_save_is_a_memory_card_file() {
        let store = SaveStore::new(PathBuf::from("/dados/saves"));
        let crash = Game {
            console_id: "playstation".to_owned(),
            ..game("PS1/Crash Bandicoot (USA).cue")
        };
        let path = store.battery_path_for(&crash);
        assert!(path.starts_with("/dados/saves/playstation"));
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("mcr"));
        assert_eq!(
            store.battery_path_for(&game("a.gb")).extension(),
            Some("sav".as_ref())
        );
    }

    #[test]
    fn similar_ids_do_not_collide() {
        let store = SaveStore::new(PathBuf::from("/s"));
        assert_ne!(
            store.path_for(&game("a/b.sms")),
            store.path_for(&game("a_b.sms"))
        );
    }

    #[test]
    fn long_ids_are_truncated() {
        let store = SaveStore::new(PathBuf::from("/s"));
        let long = "x".repeat(500);
        let name = store.path_for(&game(&long));
        let len = name.file_name().map_or(0, |n| n.len());
        assert!(len < 120, "{len}");
    }

    #[test]
    fn exists_and_delete() {
        let dir = tempfile::tempdir().expect("pasta");
        let store = SaveStore::new(dir.path().to_owned());
        let g = game("sonic.sms");
        assert!(!store.exists(&g));
        assert!(
            store.delete(&g).is_ok(),
            "apagar o que não existe não é erro"
        );

        let path = store.path_for(&g);
        fs::create_dir_all(path.parent().expect("pasta pai")).expect("pasta");
        fs::write(&path, b"estado").expect("escrita");
        let battery = store.battery_path_for(&g);
        fs::write(&battery, b"save do jogo").expect("escrita");
        assert!(store.exists(&g));
        store.delete(&g).expect("apaga");
        assert!(!store.exists(&g));
        assert!(battery.is_file(), "o save do jogo continua");
        assert_eq!(battery.parent(), path.parent());
        assert_ne!(battery, path);
    }

    fn store_with_save(bytes: &[u8]) -> (tempfile::TempDir, SaveStore, Game) {
        let dir = tempfile::tempdir().expect("pasta");
        let store = SaveStore::new(dir.path().join("saves"));
        let g = game("pokemon.gb");
        write_atomic(&store.battery_path_for(&g), bytes).expect("save");
        (dir, store, g)
    }

    fn accept_8k(bytes: &[u8]) -> Result<(), String> {
        (bytes.len() == 0x2000)
            .then_some(())
            .ok_or_else(|| "tamanho errado".to_owned())
    }

    #[test]
    fn export_then_import_round_trips() {
        let (dir, store, g) = store_with_save(&[0x5A; 0x2000]);
        assert!(store.battery_exists(&g));
        let exported = dir.path().join("fora/Pokemon Red.sav");
        store.export_battery(&g, &exported).expect("exporta");
        assert_eq!(fs::read(&exported).expect("leitura"), [0x5A; 0x2000]);

        fs::write(&exported, [0x11; 0x2000]).expect("escrita");
        store
            .import_battery(&g, &exported, accept_8k)
            .expect("importa");
        let installed = store.battery_path_for(&g);
        assert_eq!(fs::read(&installed).expect("leitura"), [0x11; 0x2000]);
        assert!(!installed.with_extension("sav.tmp").exists());
    }

    #[test]
    fn import_creates_a_save_for_a_game_without_one() {
        let dir = tempfile::tempdir().expect("pasta");
        let store = SaveStore::new(dir.path().join("saves"));
        let g = game("novo.gb");
        let source = dir.path().join("outro-emulador.sav");
        fs::write(&source, [7; 0x2000]).expect("escrita");
        store
            .import_battery(&g, &source, accept_8k)
            .expect("importa");
        assert!(store.battery_exists(&g));
    }

    #[test]
    fn export_without_save_is_an_error() {
        let dir = tempfile::tempdir().expect("pasta");
        let store = SaveStore::new(dir.path().join("saves"));
        let destination = dir.path().join("x.sav");
        assert!(matches!(
            store.export_battery(&game("a.gb"), &destination),
            Err(TransferError::NoSave)
        ));
        assert!(!destination.exists());
    }

    #[test]
    fn rejected_import_keeps_existing_save() {
        let (dir, store, g) = store_with_save(&[0x5A; 0x2000]);
        let installed = store.battery_path_for(&g);

        let wrong_size = dir.path().join("errado.sav");
        fs::write(&wrong_size, [1; 100]).expect("escrita");
        let error = store
            .import_battery(&g, &wrong_size, accept_8k)
            .expect_err("recusa");
        assert!(matches!(error, TransferError::Rejected(_)), "{error}");

        let huge = dir.path().join("enorme.sav");
        fs::write(&huge, vec![0; MAX_IMPORT_BYTES as usize + 1]).expect("escrita");
        assert!(matches!(
            store.import_battery(&g, &huge, |_| panic!("não valida o que não leu")),
            Err(TransferError::TooLarge(_))
        ));

        let missing = dir.path().join("nao-existe.sav");
        assert!(matches!(
            store.import_battery(&g, &missing, accept_8k),
            Err(TransferError::Io(_))
        ));

        assert_eq!(fs::read(&installed).expect("leitura"), [0x5A; 0x2000]);
    }

    #[test]
    fn delete_keeps_imported_save() {
        let (_dir, store, g) = store_with_save(&[3; 0x2000]);
        store.delete(&g).expect("recomeçar");
        assert!(store.battery_exists(&g));
    }
}
