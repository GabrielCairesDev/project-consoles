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
}

impl Scaler {
    pub const ALL: [Self; 3] = [Self::Nearest, Self::Epx, Self::Bilinear];

    /// Nome estável, usado nas configurações salvas, na linha de comando e
    /// na interface.
    pub fn id(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Epx => "epx",
            Self::Bilinear => "bilinear",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scaler| scaler.id() == id)
    }

    /// Ampliações que fazem sentido para o console: as de pixel art nos 2D
    /// e a suavização no 3D, além da original em todos.
    pub fn available(system: System) -> &'static [Self] {
        match system {
            System::MasterSystem | System::Genesis | System::GameBoy => &[Self::Nearest, Self::Epx],
            System::PlayStation => &[Self::Nearest, Self::Bilinear],
        }
    }
}

/// Configuração de imagem de um console: uma escolha por técnica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSettings {
    pub scaler: Scaler,
}

impl ImageSettings {
    /// Preset Original: todas as etapas desligadas, pixel a pixel.
    pub const ORIGINAL: Self = Self {
        scaler: Scaler::Nearest,
    };

    /// Preset Recomendado do console (o padrão), escolhido pelo hardware
    /// original: contornos de pixel art nos 2D, suavização no 3D.
    pub fn recommended(system: System) -> Self {
        let scaler = match system {
            System::MasterSystem | System::Genesis | System::GameBoy => Scaler::Epx,
            System::PlayStation => Scaler::Bilinear,
        };
        Self { scaler }
    }

    /// Confere se cada escolha existe no console.
    pub fn check(self, system: System) -> Result<Self, ImageError> {
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
        assert_eq!(Scaler::from_id("xbrz"), None);
        assert_eq!(Scaler::from_id(""), None);
    }

    #[test]
    fn every_console_offers_original_and_its_recommended() {
        for system in System::ALL {
            let available = Scaler::available(system);
            assert!(available.contains(&Scaler::Nearest), "{system:?}");
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
        };
        let error = bilinear_on_2d
            .check(System::MasterSystem)
            .expect_err("bilinear não é oferecida nos 2D");
        assert_eq!(error.value, "bilinear");
        assert!(error.to_string().contains("master-system"));
        let epx_on_3d = ImageSettings {
            scaler: Scaler::Epx,
        };
        assert!(epx_on_3d.check(System::PlayStation).is_err());
    }
}
