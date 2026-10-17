//! Opções de imagem escolhidas pelo usuário, por console.
//!
//! Cada técnica do catálogo (`docs/melhoria-de-imagem.md`) é um campo de
//! [`ImageSettings`], com os valores que cada console aceita. Os presets
//! Original e Recomendado só preenchem os campos; o preset atual é
//! deduzido comparando com eles, nunca guardado à parte.

use std::fmt;

use crate::controls::System;

/// Como o quadro é ampliado para a janela (etapa de escala).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaler {
    /// Vizinho mais próximo: a imagem original, pixel a pixel.
    Nearest,
    /// Scale2x/3x/4x (EPX): arredonda contornos sem borrar e sem criar
    /// cores, para pixel art.
    Epx,
    /// Interpolação bilinear: suaviza serrilhado e pontilhado do 3D.
    Bilinear,
    /// Sharp-bilinear: pixels do mesmo tamanho em qualquer escala, só a
    /// divisa entre eles misturada; em escala inteira, igual ao original.
    SharpBilinear,
}

impl Scaler {
    pub const ALL: [Self; 4] = [
        Self::Nearest,
        Self::Epx,
        Self::Bilinear,
        Self::SharpBilinear,
    ];

    /// Nome estável, usado nas configurações salvas, na linha de comando e
    /// na interface.
    pub fn id(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Epx => "epx",
            Self::Bilinear => "bilinear",
            Self::SharpBilinear => "sharp-bilinear",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scaler| scaler.id() == id)
    }

    /// Ampliações que fazem sentido para o console: as de pixel art nos 2D
    /// e a suavização no 3D, além da original e da sharp-bilinear em todos.
    pub fn available(system: System) -> &'static [Self] {
        match system {
            System::MasterSystem | System::Genesis | System::GameBoy => {
                &[Self::Nearest, Self::Epx, Self::SharpBilinear]
            }
            System::PlayStation => &[Self::Nearest, Self::Bilinear, Self::SharpBilinear],
        }
    }
}

/// Cores da tela (etapa de cor). Só os portáteis monocromáticos têm
/// paletas; nos outros consoles a única opção é a do núcleo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    /// As cores que o núcleo desenha (tons de cinza, no Game Boy).
    Core,
    /// Verde-oliva da tela do Game Boy original (DMG-01).
    Dmg,
    /// Tons neutros e escuros da tela do Game Boy Pocket.
    Pocket,
}

impl Palette {
    pub const ALL: [Self; 3] = [Self::Core, Self::Dmg, Self::Pocket];

    pub fn id(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Dmg => "dmg",
            Self::Pocket => "pocket",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|palette| palette.id() == id)
    }

    pub fn available(system: System) -> &'static [Self] {
        match system {
            System::GameBoy => &Self::ALL,
            System::MasterSystem | System::Genesis | System::PlayStation => &[Self::Core],
        }
    }

    /// Os 4 tons, do mais claro ao mais escuro, que substituem os do núcleo;
    /// `None` mantém as cores do núcleo.
    pub(crate) fn shades(self) -> Option<[u32; 4]> {
        match self {
            Self::Core => None,
            Self::Dmg => Some([0x009B_BC0F, 0x008B_AC0F, 0x0030_6230, 0x000F_380F]),
            Self::Pocket => Some([0x00C4_CFA1, 0x008B_956D, 0x004D_533C, 0x001F_1F1F]),
        }
    }
}

/// Configuração de imagem de um console: uma escolha por técnica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSettings {
    pub palette: Palette,
    pub scaler: Scaler,
}

impl ImageSettings {
    /// Preset Original: todas as etapas desligadas, pixel a pixel.
    pub const ORIGINAL: Self = Self {
        palette: Palette::Core,
        scaler: Scaler::Nearest,
    };

    /// Preset Recomendado do console (o padrão), escolhido pelo hardware
    /// original: contornos de pixel art nos 2D, suavização no 3D e, no
    /// Game Boy, o verde da tela original.
    pub fn recommended(system: System) -> Self {
        let scaler = match system {
            System::MasterSystem | System::Genesis | System::GameBoy => Scaler::Epx,
            System::PlayStation => Scaler::Bilinear,
        };
        let palette = match system {
            System::GameBoy => Palette::Dmg,
            System::MasterSystem | System::Genesis | System::PlayStation => Palette::Core,
        };
        Self { palette, scaler }
    }

    /// Confere se cada escolha existe no console.
    pub fn check(self, system: System) -> Result<Self, ImageError> {
        if !Palette::available(system).contains(&self.palette) {
            return Err(ImageError {
                option: "paleta",
                value: self.palette.id(),
                system,
            });
        }
        if !Scaler::available(system).contains(&self.scaler) {
            return Err(ImageError {
                option: "ampliação",
                value: self.scaler.id(),
                system,
            });
        }
        Ok(self)
    }
}

/// Escolha de imagem que não existe no console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageError {
    pub option: &'static str,
    pub value: &'static str,
    pub system: System,
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} \"{}\" não existe para {}",
            self.option,
            self.value,
            self.system.id()
        )
    }
}

impl std::error::Error for ImageError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_unknown_is_none() {
        for scaler in Scaler::ALL {
            assert_eq!(Scaler::from_id(scaler.id()), Some(scaler));
        }
        for palette in Palette::ALL {
            assert_eq!(Palette::from_id(palette.id()), Some(palette));
        }
        assert_eq!(Palette::from_id("gbc"), None);
        assert_eq!(Scaler::from_id("xbrz"), None);
        assert_eq!(Scaler::from_id(""), None);
    }

    #[test]
    fn every_console_offers_original_and_its_recommended() {
        for system in System::ALL {
            let available = Scaler::available(system);
            assert!(available.contains(&Scaler::Nearest), "{system:?}");
            assert!(available.contains(&Scaler::SharpBilinear), "{system:?}");
            assert!(
                Palette::available(system).contains(&Palette::Core),
                "{system:?}"
            );
            let recommended = ImageSettings::recommended(system);
            assert_ne!(recommended, ImageSettings::ORIGINAL, "{system:?}");
            assert_eq!(recommended.check(system), Ok(recommended));
            assert_eq!(
                ImageSettings::ORIGINAL.check(system),
                Ok(ImageSettings::ORIGINAL)
            );
        }
    }

    #[test]
    fn scalers_follow_the_kind_of_console() {
        assert_eq!(
            ImageSettings::recommended(System::GameBoy).scaler,
            Scaler::Epx
        );
        assert_eq!(
            ImageSettings::recommended(System::PlayStation).scaler,
            Scaler::Bilinear
        );
        let bilinear_on_2d = ImageSettings {
            scaler: Scaler::Bilinear,
            ..ImageSettings::ORIGINAL
        };
        let error = bilinear_on_2d
            .check(System::MasterSystem)
            .expect_err("bilinear não é oferecida nos 2D");
        assert_eq!(error.value, "bilinear");
        assert!(error.to_string().contains("master-system"));
        let epx_on_3d = ImageSettings {
            scaler: Scaler::Epx,
            ..ImageSettings::ORIGINAL
        };
        assert!(epx_on_3d.check(System::PlayStation).is_err());
    }

    #[test]
    fn palettes_exist_only_on_the_game_boy() {
        assert_eq!(
            ImageSettings::recommended(System::GameBoy).palette,
            Palette::Dmg
        );
        let green_sms = ImageSettings {
            palette: Palette::Dmg,
            ..ImageSettings::recommended(System::MasterSystem)
        };
        let error = green_sms
            .check(System::MasterSystem)
            .expect_err("o Master System tem cores próprias");
        assert_eq!((error.option, error.value), ("paleta", "dmg"));
        assert_eq!(Palette::Core.shades(), None);
        for palette in [Palette::Dmg, Palette::Pocket] {
            let shades = palette.shades().expect("tons");
            let luma = |c: u32| (c >> 16 & 0xFF) * 3 + (c >> 8 & 0xFF) * 6 + (c & 0xFF);
            assert!(
                shades.windows(2).all(|w| luma(w[0]) > luma(w[1])),
                "{palette:?} vai do claro ao escuro"
            );
        }
    }
}
