//! Onde fica o estado salvo de cada jogo ("continuar de onde parou").
//!
//! Um arquivo por jogo em `<dados do app>/saves/<console>/`. O nome mistura
//! uma versão legível do caminho do jogo na biblioteca com um hash desse
//! caminho, para ser único mesmo quando dois jogos têm o mesmo nome de
//! arquivo em pastas diferentes. O conteúdo é opaco aqui: quem lê e grava é
//! o processo do jogo.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::library::Game;

/// Limite do trecho legível do nome, para não estourar o limite de
/// caminho de nenhum sistema.
const MAX_STEM_CHARS: usize = 80;

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
            .join(file_name(&game.id))
    }

    pub fn exists(&self, game: &Game) -> bool {
        self.path_for(game).is_file()
    }

    /// Apaga o estado salvo; não ter estado nenhum não é erro.
    pub fn delete(&self, game: &Game) -> io::Result<()> {
        match fs::remove_file(self.path_for(game)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

fn file_name(game_id: &str) -> String {
    let stem: String = sanitize(game_id).chars().take(MAX_STEM_CHARS).collect();
    format!("{stem}-{:016x}.state", fnv1a(game_id.as_bytes()))
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
        assert!(store.exists(&g));
        store.delete(&g).expect("apaga");
        assert!(!store.exists(&g));
    }
}
