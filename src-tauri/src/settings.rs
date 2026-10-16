//! Preferências do usuário persistidas em JSON.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use player::controls::{BindingError, Bindings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Pasta da biblioteca, com uma subpasta por console.
    pub library_dir: Option<PathBuf>,
    /// Teclas por console: `console id → (ação → tecla)`. Ausente = padrão.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub controls: BTreeMap<String, BTreeMap<String, String>>,
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

    /// Teclas configuradas para o console, ou o padrão se não houver.
    pub fn controls_for(&self, console_id: &str) -> Result<Bindings, BindingError> {
        match self.controls.get(console_id) {
            Some(keys) => Bindings::from_pairs(keys.iter().map(|(a, k)| (a.as_str(), k.as_str()))),
            None => Ok(Bindings::default()),
        }
    }

    pub fn set_controls(&mut self, console_id: &str, bindings: &Bindings) {
        let keys = bindings
            .pairs()
            .map(|(action, key)| (action.to_owned(), key.to_owned()))
            .collect();
        self.controls.insert(console_id.to_owned(), keys);
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
        let mut settings = Settings {
            library_dir: Some(PathBuf::from("/Jogos")),
            ..Settings::default()
        };
        let custom = Bindings::from_pairs([
            ("up", "KeyW"),
            ("down", "KeyS"),
            ("left", "KeyA"),
            ("right", "KeyD"),
            ("button1", "KeyJ"),
            ("button2", "KeyK"),
            ("pause", "Space"),
        ])
        .expect("mapa válido");
        settings.set_controls("master-system", &custom);
        settings.save(&path).expect("save");
        let loaded = Settings::load(&path).expect("load");
        assert_eq!(loaded, settings);
        assert_eq!(loaded.controls_for("master-system"), Ok(custom));
    }

    #[test]
    fn controls_default_when_absent_and_reject_corrupted() {
        let mut settings = Settings::default();
        assert_eq!(
            settings.controls_for("master-system"),
            Ok(Bindings::default())
        );
        settings.controls.insert(
            "master-system".to_owned(),
            BTreeMap::from([("up".to_owned(), "Escape".to_owned())]),
        );
        assert!(settings.controls_for("master-system").is_err());
    }

    #[test]
    fn older_file_without_controls_still_loads() {
        let dir = tempfile::tempdir().expect("pasta");
        let path = dir.path().join("settings.json");
        fs::write(&path, br#"{ "libraryDir": "/Jogos" }"#).expect("escrita");
        let settings = Settings::load(&path).expect("load");
        assert!(settings.controls.is_empty());
    }

    #[test]
    fn corrupted_file_is_an_error() {
        let dir = tempfile::tempdir().expect("pasta");
        let path = dir.path().join("settings.json");
        fs::write(&path, b"{ quebrado").expect("escrita");
        assert!(Settings::load(&path).is_err());
    }
}
