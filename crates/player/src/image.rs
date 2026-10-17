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
    /// MMPX (McGuire & Gagiu, 2021): reconstrói curvas, diagonais e cantos
    /// vivos da pixel art, preservando detalhes de um pixel e sem criar
    /// cores.
    Mmpx,
    /// Interpolação bilinear: suaviza serrilhado e pontilhado do 3D.
    Bilinear,
    /// Sharp-bilinear: pixels do mesmo tamanho em qualquer escala, só a
    /// divisa entre eles misturada; em escala inteira, igual ao original.
    SharpBilinear,
}

impl Scaler {
    pub const ALL: [Self; 5] = [
        Self::Nearest,
        Self::Epx,
        Self::Mmpx,
        Self::Bilinear,
        Self::SharpBilinear,
    ];

    /// Nome estável, usado nas configurações salvas, na linha de comando e
    /// na interface.
    pub fn id(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Epx => "epx",
            Self::Mmpx => "mmpx",
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
                &[Self::Nearest, Self::Epx, Self::Mmpx, Self::SharpBilinear]
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

/// Proporção da imagem na tela (etapa de ajuste).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectRatio {
    /// Cada pixel do console vira um quadrado.
    Square,
    /// A proporção da TV para a qual o console foi feito (4:3); os pixels
    /// ficam levemente esticados, como na época.
    Tv,
}

impl AspectRatio {
    pub const ALL: [Self; 2] = [Self::Square, Self::Tv];

    pub fn id(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Tv => "tv",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|aspect| aspect.id() == id)
    }

    /// O Game Boy tem tela própria de pixels quadrados; o PlayStation muda
    /// a largura em pixels conforme o modo de vídeo, então só a proporção da
    /// TV faz sentido nele.
    pub fn available(system: System) -> &'static [Self] {
        match system {
            System::MasterSystem | System::Genesis => &Self::ALL,
            System::GameBoy => &[Self::Square],
            System::PlayStation => &[Self::Tv],
        }
    }
}

/// Como a imagem ocupa a janela (etapa de ajuste).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Escala inteira: todos os pixels do mesmo tamanho, com barras.
    Integer,
    /// O maior tamanho que cabe na janela, mantendo a proporção.
    Fill,
}

impl Fit {
    pub const ALL: [Self; 2] = [Self::Integer, Self::Fill];

    pub fn id(self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Fill => "fill",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|fit| fit.id() == id)
    }

    pub fn available(_system: System) -> &'static [Self] {
        &Self::ALL
    }
}

/// Simulação da tela (etapa de tela), aplicada depois da ampliação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenEffect {
    None,
    /// Linhas escuras entre as linhas do console, como no CRT.
    Scanlines,
    /// Scanlines e máscara de abertura (colunas R, G e B) de um tubo.
    Crt,
    /// Divisões entre os pixels de uma tela de LCD.
    LcdGrid,
}

impl ScreenEffect {
    pub const ALL: [Self; 4] = [Self::None, Self::Scanlines, Self::Crt, Self::LcdGrid];

    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Scanlines => "scanlines",
            Self::Crt => "crt",
            Self::LcdGrid => "lcd-grid",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|effect| effect.id() == id)
    }

    /// Efeitos de TV nos consoles de TV; grade de LCD no portátil.
    pub fn available(system: System) -> &'static [Self] {
        match system {
            System::MasterSystem | System::Genesis | System::PlayStation => {
                &[Self::None, Self::Scanlines, Self::Crt]
            }
            System::GameBoy => &[Self::None, Self::LcdGrid],
        }
    }
}

/// Sinal de vídeo simulado (etapa de cor, antes da ampliação).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Imagem limpa, como num cabo RGB.
    Rgb,
    /// Composto simplificado: funde o pontilhado que os jogos usavam como
    /// transparência na TV. Não é uma simulação completa do sinal NTSC.
    Composite,
}

impl Signal {
    pub const ALL: [Self; 2] = [Self::Rgb, Self::Composite];

    pub fn id(self) -> &'static str {
        match self {
            Self::Rgb => "rgb",
            Self::Composite => "composite",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|signal| signal.id() == id)
    }

    pub fn available(system: System) -> &'static [Self] {
        match system {
            System::MasterSystem | System::Genesis | System::PlayStation => &Self::ALL,
            System::GameBoy => &[Self::Rgb],
        }
    }
}

/// Rastro do LCD (frame blending): cada quadro misturado com o anterior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameBlend {
    Off,
    /// Recria o rastro da tela do portátil; efeitos que piscam a cada
    /// quadro viram transparência em vez de tremer.
    On,
}

impl FrameBlend {
    pub const ALL: [Self; 2] = [Self::Off, Self::On];

    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|blend| blend.id() == id)
    }

    pub fn available(system: System) -> &'static [Self] {
        match system {
            System::GameBoy => &Self::ALL,
            System::MasterSystem | System::Genesis | System::PlayStation => &[Self::Off],
        }
    }
}

/// Configuração de imagem de um console: uma escolha por técnica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSettings {
    pub palette: Palette,
    pub scaler: Scaler,
    pub aspect: AspectRatio,
    pub fit: Fit,
    pub screen: ScreenEffect,
    pub signal: Signal,
    pub blend: FrameBlend,
}

impl ImageSettings {
    /// Preset Original: etapas de cor e escala desligadas (pixel a pixel) e
    /// a apresentação de sempre do console: pixels quadrados em escala
    /// inteira nos 2D, tela 4:3 preenchida no PlayStation.
    pub fn original(system: System) -> Self {
        let (aspect, fit) = match system {
            System::MasterSystem | System::Genesis | System::GameBoy => {
                (AspectRatio::Square, Fit::Integer)
            }
            System::PlayStation => (AspectRatio::Tv, Fit::Fill),
        };
        Self {
            palette: Palette::Core,
            scaler: Scaler::Nearest,
            aspect,
            fit,
            screen: ScreenEffect::None,
            signal: Signal::Rgb,
            blend: FrameBlend::Off,
        }
    }

    /// Preset Recomendado do console (o padrão), escolhido pelo hardware
    /// original: contornos de pixel art nos 2D, suavização no 3D e, no
    /// Game Boy, o verde da tela original. A apresentação é a do Original,
    /// e os efeitos de tela, sinal e rastro ficam desligados (são escolha
    /// do usuário, não correção).
    pub fn recommended(system: System) -> Self {
        let scaler = match system {
            System::MasterSystem | System::Genesis | System::GameBoy => Scaler::Epx,
            System::PlayStation => Scaler::Bilinear,
        };
        let palette = match system {
            System::GameBoy => Palette::Dmg,
            System::MasterSystem | System::Genesis | System::PlayStation => Palette::Core,
        };
        Self {
            palette,
            scaler,
            ..Self::original(system)
        }
    }

    /// Confere se cada escolha existe no console.
    pub fn check(self, system: System) -> Result<Self, ImageError> {
        let error = |option, value| ImageError {
            option,
            value,
            system,
        };
        if !Palette::available(system).contains(&self.palette) {
            return Err(error("paleta", self.palette.id()));
        }
        if !Scaler::available(system).contains(&self.scaler) {
            return Err(error("ampliação", self.scaler.id()));
        }
        if !AspectRatio::available(system).contains(&self.aspect) {
            return Err(error("proporção", self.aspect.id()));
        }
        if !Fit::available(system).contains(&self.fit) {
            return Err(error("encaixe", self.fit.id()));
        }
        if !ScreenEffect::available(system).contains(&self.screen) {
            return Err(error("efeito de tela", self.screen.id()));
        }
        if !Signal::available(system).contains(&self.signal) {
            return Err(error("sinal", self.signal.id()));
        }
        if !FrameBlend::available(system).contains(&self.blend) {
            return Err(error("rastro", self.blend.id()));
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
            let original = ImageSettings::original(system);
            assert_ne!(recommended, original, "{system:?}");
            assert_eq!(recommended.check(system), Ok(recommended));
            assert_eq!(original.check(system), Ok(original));
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
            ..ImageSettings::original(System::MasterSystem)
        };
        let error = bilinear_on_2d
            .check(System::MasterSystem)
            .expect_err("bilinear não é oferecida nos 2D");
        assert_eq!(error.value, "bilinear");
        assert!(error.to_string().contains("master-system"));
        for pixel_art in [Scaler::Epx, Scaler::Mmpx] {
            let on_3d = ImageSettings {
                scaler: pixel_art,
                ..ImageSettings::original(System::PlayStation)
            };
            assert!(on_3d.check(System::PlayStation).is_err(), "{pixel_art:?}");
            let on_2d = ImageSettings {
                scaler: pixel_art,
                ..ImageSettings::original(System::Genesis)
            };
            assert!(on_2d.check(System::Genesis).is_ok(), "{pixel_art:?}");
        }
    }

    #[test]
    fn presentation_follows_each_screen() {
        for id in ["square", "tv"] {
            let aspect = AspectRatio::from_id(id).expect("proporção");
            assert_eq!(aspect.id(), id);
        }
        for id in ["integer", "fill"] {
            assert_eq!(Fit::from_id(id).map(Fit::id), Some(id));
        }
        assert_eq!(AspectRatio::from_id("16:9"), None);
        // O de sempre: pixels quadrados inteiros nos 2D, 4:3 cheio no PS1.
        let sms = ImageSettings::original(System::MasterSystem);
        assert_eq!((sms.aspect, sms.fit), (AspectRatio::Square, Fit::Integer));
        let ps1 = ImageSettings::original(System::PlayStation);
        assert_eq!((ps1.aspect, ps1.fit), (AspectRatio::Tv, Fit::Fill));
        // O Game Boy não tem TV; o PS1 não tem pixels quadrados.
        let gb_tv = ImageSettings {
            aspect: AspectRatio::Tv,
            ..ImageSettings::original(System::GameBoy)
        };
        assert_eq!(
            gb_tv.check(System::GameBoy).expect_err("sem TV").option,
            "proporção"
        );
        let ps1_square = ImageSettings {
            aspect: AspectRatio::Square,
            ..ps1
        };
        assert!(ps1_square.check(System::PlayStation).is_err());
        let sms_tv_fill = ImageSettings {
            aspect: AspectRatio::Tv,
            fit: Fit::Fill,
            ..sms
        };
        assert!(sms_tv_fill.check(System::MasterSystem).is_ok());
    }

    #[test]
    fn screen_signal_and_blend_follow_tv_or_lcd() {
        for effect in ScreenEffect::ALL {
            assert_eq!(ScreenEffect::from_id(effect.id()), Some(effect));
        }
        for signal in Signal::ALL {
            assert_eq!(Signal::from_id(signal.id()), Some(signal));
        }
        for blend in FrameBlend::ALL {
            assert_eq!(FrameBlend::from_id(blend.id()), Some(blend));
        }
        for system in System::ALL {
            let recommended = ImageSettings::recommended(system);
            assert_eq!(recommended.screen, ScreenEffect::None);
            assert_eq!(recommended.signal, Signal::Rgb);
            assert_eq!(recommended.blend, FrameBlend::Off);
        }
        let crt_gb = ImageSettings {
            screen: ScreenEffect::Crt,
            ..ImageSettings::original(System::GameBoy)
        };
        assert_eq!(
            crt_gb.check(System::GameBoy).expect_err("sem CRT").option,
            "efeito de tela"
        );
        let grid_sms = ImageSettings {
            screen: ScreenEffect::LcdGrid,
            ..ImageSettings::original(System::MasterSystem)
        };
        assert!(grid_sms.check(System::MasterSystem).is_err());
        let blend_genesis = ImageSettings {
            blend: FrameBlend::On,
            ..ImageSettings::original(System::Genesis)
        };
        assert!(blend_genesis.check(System::Genesis).is_err());
        let full_gb = ImageSettings {
            screen: ScreenEffect::LcdGrid,
            blend: FrameBlend::On,
            ..ImageSettings::recommended(System::GameBoy)
        };
        assert!(full_gb.check(System::GameBoy).is_ok());
        let composite_sms = ImageSettings {
            signal: Signal::Composite,
            screen: ScreenEffect::Crt,
            ..ImageSettings::recommended(System::MasterSystem)
        };
        assert!(composite_sms.check(System::MasterSystem).is_ok());
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
