//! Preferências do usuário persistidas em JSON.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use player::controls::{BindingError, Bindings, System};
use player::image::{
    AspectRatio, Fit, FrameBlend, ImageSettings, Palette, Scaler, ScreenEffect, Signal,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Pasta da biblioteca, com uma subpasta por console.
    pub library_dir: Option<PathBuf>,
    /// Teclas por console: `console id → (ação → tecla)`. Ausente = padrão.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub controls: BTreeMap<String, BTreeMap<String, String>>,
    /// Opções de cada console jogável. Ausente = [`ConsoleSettings::default`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub consoles: BTreeMap<String, ConsoleSettings>,
}

/// Opções de jogo de um console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "StoredConsoleSettings")]
pub struct ConsoleSettings {
    /// Salvar o progresso ao fechar e continuar dele ao abrir.
    pub resume: bool,
    /// Escolhas de imagem do usuário (`docs/melhoria-de-imagem.md`).
    pub image: ImageChoices,
}

impl Default for ConsoleSettings {
    fn default() -> Self {
        Self {
            resume: true,
            image: ImageChoices::default(),
        }
    }
}

/// Formato gravado, incluindo o antigo: até a imagem configurável, um
/// único `enhance` ligava ou desligava a melhoria.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredConsoleSettings {
    #[serde(default = "resume_by_default")]
    resume: bool,
    image: Option<ImageChoices>,
    enhance: Option<bool>,
}

fn resume_by_default() -> bool {
    ConsoleSettings::default().resume
}

impl From<StoredConsoleSettings> for ConsoleSettings {
    /// `enhance: false` desliga cor e escala (o que a melhoria fazia);
    /// `true` ou ausente vale o Recomendado. Proporção e encaixe ficam no
    /// padrão do console, que era o único comportamento antes. Com `image`
    /// gravado, `enhance` não vale mais.
    fn from(stored: StoredConsoleSettings) -> Self {
        let image = match (stored.image, stored.enhance) {
            (Some(image), _) => image,
            (None, Some(false)) => ImageChoices::filters_off(),
            (None, _) => ImageChoices::default(),
        };
        Self {
            resume: stored.resume,
            image,
        }
    }
}

/// Escolhas de imagem gravadas, uma por técnica, pelos ids estáveis do
/// player. Ausente vale o Recomendado do console, que acompanha melhorias
/// futuras dele.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageChoices {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaler: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend: Option<String>,
}

impl From<ImageSettings> for ImageChoices {
    fn from(image: ImageSettings) -> Self {
        Self {
            palette: Some(image.palette.id().to_owned()),
            scaler: Some(image.scaler.id().to_owned()),
            aspect: Some(image.aspect.id().to_owned()),
            fit: Some(image.fit.id().to_owned()),
            screen: Some(image.screen.id().to_owned()),
            signal: Some(image.signal.id().to_owned()),
            blend: Some(image.blend.id().to_owned()),
        }
    }
}

/// Uma técnica de imagem do player, vista pelos ids gravados.
struct Technique<T: 'static> {
    /// Nome para mensagens ("a ampliação", "a paleta").
    name: &'static str,
    from_id: fn(&str) -> Option<T>,
    available: fn(System) -> &'static [T],
}

const PALETTE: Technique<Palette> = Technique {
    name: "a paleta",
    from_id: Palette::from_id,
    available: Palette::available,
};

const SCALER: Technique<Scaler> = Technique {
    name: "a ampliação",
    from_id: Scaler::from_id,
    available: Scaler::available,
};

const ASPECT: Technique<AspectRatio> = Technique {
    name: "a proporção",
    from_id: AspectRatio::from_id,
    available: AspectRatio::available,
};

const FIT: Technique<Fit> = Technique {
    name: "o encaixe",
    from_id: Fit::from_id,
    available: Fit::available,
};

const SCREEN: Technique<ScreenEffect> = Technique {
    name: "o efeito de tela",
    from_id: ScreenEffect::from_id,
    available: ScreenEffect::available,
};

const SIGNAL: Technique<Signal> = Technique {
    name: "o sinal de vídeo",
    from_id: Signal::from_id,
    available: Signal::available,
};

const BLEND: Technique<FrameBlend> = Technique {
    name: "o rastro do LCD",
    from_id: FrameBlend::from_id,
    available: FrameBlend::available,
};

impl<T: Copy + PartialEq> Technique<T> {
    /// O valor de `id`, se ele existir no console.
    fn find(&self, system: System, id: &str) -> Option<T> {
        (self.from_id)(id).filter(|value| (self.available)(system).contains(value))
    }

    /// Escolha gravada; ausente ou inválida vale `recommended` (inválida,
    /// com aviso).
    fn resolve(&self, system: System, stored: Option<&str>, recommended: T) -> T {
        let Some(id) = stored else {
            return recommended;
        };
        self.find(system, id).unwrap_or_else(|| {
            eprintln!(
                "{} gravada para {} não existe ({id}); usando a recomendada",
                self.name,
                system.id()
            );
            recommended
        })
    }

    fn check(&self, system: System, stored: Option<&str>) -> Result<(), String> {
        match stored {
            Some(id) if self.find(system, id).is_none() => {
                let mut name = self.name.to_owned();
                name[..1].make_ascii_uppercase();
                Err(format!("{name} \"{id}\" não existe para este console."))
            }
            _ => Ok(()),
        }
    }
}

impl ImageChoices {
    /// Cor e escala desligadas, sem depender do console (o resto vale o
    /// padrão dele).
    pub fn filters_off() -> Self {
        Self {
            palette: Some(Palette::Core.id().to_owned()),
            scaler: Some(Scaler::Nearest.id().to_owned()),
            ..Self::default()
        }
    }

    /// Configuração com que o jogo roda. Uma escolha gravada que não existe
    /// no console (arquivo editado à mão ou de outra versão) vale o
    /// Recomendado, com aviso; as outras escolhas são mantidas.
    pub fn resolve(&self, system: System) -> ImageSettings {
        let recommended = ImageSettings::recommended(system);
        ImageSettings {
            palette: PALETTE.resolve(system, self.palette.as_deref(), recommended.palette),
            scaler: SCALER.resolve(system, self.scaler.as_deref(), recommended.scaler),
            aspect: ASPECT.resolve(system, self.aspect.as_deref(), recommended.aspect),
            fit: FIT.resolve(system, self.fit.as_deref(), recommended.fit),
            screen: SCREEN.resolve(system, self.screen.as_deref(), recommended.screen),
            signal: SIGNAL.resolve(system, self.signal.as_deref(), recommended.signal),
            blend: BLEND.resolve(system, self.blend.as_deref(), recommended.blend),
        }
    }

    /// Confere escolhas vindas da interface: qualquer valor que não exista
    /// no console é recusado, sem gravar nada.
    pub fn check(&self, system: System) -> Result<(), String> {
        PALETTE.check(system, self.palette.as_deref())?;
        SCALER.check(system, self.scaler.as_deref())?;
        ASPECT.check(system, self.aspect.as_deref())?;
        FIT.check(system, self.fit.as_deref())?;
        SCREEN.check(system, self.screen.as_deref())?;
        SIGNAL.check(system, self.signal.as_deref())?;
        BLEND.check(system, self.blend.as_deref())
    }
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
    pub fn controls_for(&self, system: System) -> Result<Bindings, BindingError> {
        match self.controls.get(system.id()) {
            Some(keys) => {
                Bindings::from_pairs(system, keys.iter().map(|(a, k)| (a.as_str(), k.as_str())))
            }
            None => Ok(Bindings::defaults(system)),
        }
    }

    /// Grava as teclas no console de `bindings`.
    pub fn set_controls(&mut self, bindings: &Bindings) {
        let keys = bindings
            .pairs()
            .map(|(action, key)| (action.to_owned(), key.to_owned()))
            .collect();
        self.controls
            .insert(bindings.system().id().to_owned(), keys);
    }

    pub fn console(&self, console_id: &str) -> ConsoleSettings {
        self.consoles.get(console_id).cloned().unwrap_or_default()
    }

    pub fn set_console(&mut self, console_id: &str, settings: ConsoleSettings) {
        self.consoles.insert(console_id.to_owned(), settings);
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
        let custom = Bindings::from_pairs(
            System::MasterSystem,
            [
                ("up", "KeyW"),
                ("down", "KeyS"),
                ("left", "KeyA"),
                ("right", "KeyD"),
                ("button1", "KeyJ"),
                ("button2", "KeyK"),
                ("pause", "Space"),
            ],
        )
        .expect("mapa válido");
        settings.set_controls(&custom);
        settings.set_console(
            "master-system",
            ConsoleSettings {
                resume: false,
                image: ImageChoices::default(),
            },
        );
        settings.set_console(
            "playstation",
            ConsoleSettings {
                resume: true,
                image: ImageChoices::from(ImageSettings::original(System::PlayStation)),
            },
        );
        settings.save(&path).expect("save");
        let loaded = Settings::load(&path).expect("load");
        assert_eq!(loaded, settings);
        assert_eq!(loaded.controls_for(System::MasterSystem), Ok(custom));
        assert_eq!(
            loaded.controls_for(System::Genesis),
            Ok(Bindings::defaults(System::Genesis)),
            "cada console tem as próprias teclas"
        );
        assert!(!loaded.console("master-system").resume);
        assert_eq!(
            loaded
                .console("master-system")
                .image
                .resolve(System::MasterSystem),
            ImageSettings::recommended(System::MasterSystem)
        );
        assert_eq!(
            loaded
                .console("playstation")
                .image
                .resolve(System::PlayStation),
            ImageSettings::original(System::PlayStation)
        );
    }

    #[test]
    fn controls_default_when_absent_and_reject_corrupted() {
        let mut settings = Settings::default();
        assert_eq!(
            settings.controls_for(System::MasterSystem),
            Ok(Bindings::defaults(System::MasterSystem))
        );
        settings.controls.insert(
            "master-system".to_owned(),
            BTreeMap::from([("up".to_owned(), "Escape".to_owned())]),
        );
        assert!(settings.controls_for(System::MasterSystem).is_err());
    }

    #[test]
    fn console_settings_default_to_resume_and_recommended_image() {
        let settings = Settings::default();
        let console = settings.console("master-system");
        assert!(console.resume);
        assert_eq!(
            console.image.resolve(System::MasterSystem),
            ImageSettings::recommended(System::MasterSystem)
        );
        let parsed: ConsoleSettings = serde_json::from_str("{}").expect("json");
        assert_eq!(parsed, ConsoleSettings::default());
        let older: ConsoleSettings = serde_json::from_str(r#"{ "resume": false }"#).expect("json");
        assert!(!older.resume);
        assert_eq!(older.image, ImageChoices::default());
    }

    #[test]
    fn old_enhance_flag_migrates_to_a_preset() {
        let on: ConsoleSettings =
            serde_json::from_str(r#"{ "resume": true, "enhance": true }"#).expect("json");
        assert_eq!(on.image, ImageChoices::default(), "ligada = Recomendado");
        let off: ConsoleSettings =
            serde_json::from_str(r#"{ "resume": false, "enhance": false }"#).expect("json");
        assert!(!off.resume);
        for system in System::ALL {
            assert_eq!(
                off.image.resolve(system),
                ImageSettings::original(system),
                "{system:?}"
            );
        }
        // Com `image` gravado, o `enhance` antigo não vale mais.
        let both: ConsoleSettings =
            serde_json::from_str(r#"{ "enhance": false, "image": { "scaler": "epx" } }"#)
                .expect("json");
        assert_eq!(both.image.scaler.as_deref(), Some("epx"));
        // O formato novo não grava mais `enhance`.
        let written = serde_json::to_string(&off).expect("json");
        assert!(!written.contains("enhance"), "{written}");
        let reread: ConsoleSettings = serde_json::from_str(&written).expect("json");
        assert_eq!(reread, off);
    }

    #[test]
    fn each_stored_choice_resolves_on_its_own() {
        let stored: ConsoleSettings =
            serde_json::from_str(r#"{ "image": { "palette": "pocket", "scaler": "nao-existe" } }"#)
                .expect("json");
        let image = stored.image.resolve(System::GameBoy);
        assert_eq!(image.palette, Palette::Pocket, "a escolha válida é mantida");
        assert_eq!(
            image.scaler,
            ImageSettings::recommended(System::GameBoy).scaler
        );
    }

    #[test]
    fn invalid_stored_choice_falls_back_to_recommended() {
        for id in ["xbrz-de-outra-versao", "bilinear"] {
            let stored: ConsoleSettings = serde_json::from_str(&format!(
                r#"{{ "resume": false, "image": {{ "scaler": "{id}" }} }}"#
            ))
            .expect("um valor desconhecido não corrompe o arquivo");
            assert!(!stored.resume, "o resto é preservado");
            assert_eq!(
                stored.image.resolve(System::Genesis),
                ImageSettings::recommended(System::Genesis),
                "{id}"
            );
        }
    }

    #[test]
    fn choices_from_the_interface_are_checked_per_console() {
        let epx = ImageChoices {
            scaler: Some("epx".to_owned()),
            ..ImageChoices::default()
        };
        assert_eq!(epx.check(System::GameBoy), Ok(()));
        assert!(epx.check(System::PlayStation).is_err());
        let unknown = ImageChoices {
            scaler: Some("xbrz".to_owned()),
            ..ImageChoices::default()
        };
        let error = unknown.check(System::GameBoy).expect_err("desconhecida");
        assert_eq!(error, "A ampliação \"xbrz\" não existe para este console.");
        let green = ImageChoices {
            palette: Some("dmg".to_owned()),
            ..ImageChoices::default()
        };
        assert_eq!(green.check(System::GameBoy), Ok(()));
        let error = green.check(System::Genesis).expect_err("sem paletas");
        assert_eq!(error, "A paleta \"dmg\" não existe para este console.");
        assert_eq!(ImageChoices::default().check(System::PlayStation), Ok(()));
        let tv = ImageChoices {
            aspect: Some("tv".to_owned()),
            fit: Some("fill".to_owned()),
            ..ImageChoices::default()
        };
        assert_eq!(tv.check(System::Genesis), Ok(()));
        assert_eq!(
            tv.check(System::GameBoy),
            Err("A proporção \"tv\" não existe para este console.".to_owned())
        );
        let crt_on_gb = ImageChoices {
            screen: Some("crt".to_owned()),
            ..ImageChoices::default()
        };
        assert_eq!(
            crt_on_gb.check(System::GameBoy),
            Err("O efeito de tela \"crt\" não existe para este console.".to_owned())
        );
        let ghosting = ImageChoices {
            screen: Some("lcd-grid".to_owned()),
            blend: Some("on".to_owned()),
            ..ImageChoices::default()
        };
        assert_eq!(ghosting.check(System::GameBoy), Ok(()));
        assert!(ghosting.check(System::Genesis).is_err());
        let stretched = ImageChoices {
            fit: Some("stretch".to_owned()),
            ..ImageChoices::default()
        };
        assert_eq!(
            stretched.check(System::Genesis),
            Err("O encaixe \"stretch\" não existe para este console.".to_owned())
        );
    }

    #[test]
    fn older_file_without_controls_still_loads() {
        let dir = tempfile::tempdir().expect("pasta");
        let path = dir.path().join("settings.json");
        fs::write(&path, br#"{ "libraryDir": "/Jogos" }"#).expect("escrita");
        let settings = Settings::load(&path).expect("load");
        assert!(settings.controls.is_empty());
        assert!(settings.consoles.is_empty());
    }

    #[test]
    fn corrupted_file_is_an_error() {
        let dir = tempfile::tempdir().expect("pasta");
        let path = dir.path().join("settings.json");
        fs::write(&path, b"{ quebrado").expect("escrita");
        assert!(Settings::load(&path).is_err());
    }
}
