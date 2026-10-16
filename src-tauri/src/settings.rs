//! Preferências do usuário persistidas em JSON.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Pasta da biblioteca, com uma subpasta por console.
    pub library_dir: Option<PathBuf>,
}

impl Settings {
    /// Carrega as preferências. Arquivo ausente vira o padrão; arquivo
    /// corrompido é erro, para não sobrescrever silenciosamente o que o
    /// usuário configurou.
    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().expect("pasta");
        assert_eq!(
            Settings::load(&dir.path().join("settings.json")).expect("load"),
            Settings::default()
        );
    }

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().expect("pasta");
        let path = dir.path().join("config/settings.json");
        let settings = Settings {
            library_dir: Some(PathBuf::from("/Jogos")),
        };
        settings.save(&path).expect("save");
        assert_eq!(Settings::load(&path).expect("load"), settings);
    }

    #[test]
    fn corrupted_file_is_an_error() {
        let dir = tempfile::tempdir().expect("pasta");
        let path = dir.path().join("settings.json");
        fs::write(&path, b"{ quebrado").expect("escrita");
        assert!(Settings::load(&path).is_err());
    }
}
