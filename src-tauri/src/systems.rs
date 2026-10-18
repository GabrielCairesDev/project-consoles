//! Consoles com reconhecimento de ROMs: nome do sistema no acervo libretro,
//! extensões próprias, formato de normalização e DAT No-Intro embutido.
//!
//! Cada extensão pertence a um único sistema; ela define como normalizar os
//! bytes. Discos (PlayStation) não têm DAT embutido: o jogo vem do `.cue` e
//! o título do nome do arquivo. A identificação final consulta os DATs de todos os sistemas, então
//! uma ROM com extensão trocada ainda é reconhecida.
//!
//! DATs: libretro-database (CC-BY-SA 4.0); ver `data/dats/SOURCES.md`.

use std::sync::OnceLock;

use crate::dat::{self, DatIndex};

/// Como os bytes da ROM devem ser normalizados antes do CRC32, para bater
/// com o dump de referência do No-Intro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RomFormat {
    /// Arquivo usado como está.
    Plain,
    /// SNES: descarta o cabeçalho de copiadora de 512 bytes, quando houver.
    Snes,
    /// Nintendo 64: converte `.v64`/`.n64` para a ordem big-endian (`.z64`).
    N64,
}

/// Um sistema reconhecível.
pub struct System {
    /// `id` do console no catálogo.
    pub console_id: &'static str,
    /// Nome do sistema no libretro (DATs e acervo de capas).
    pub libretro_name: &'static str,
    /// Extensões próprias deste sistema, em minúsculas e sem ponto.
    pub extensions: &'static [&'static str],
    pub format: RomFormat,
    /// O sistema guarda um ícone dentro da ROM que serve de capa reserva.
    pub has_embedded_icon: bool,
    /// DAT No-Intro embutido; `None` para sistemas reconhecidos só pela
    /// extensão.
    dat: Option<&'static str>,
    index: OnceLock<DatIndex>,
}

impl System {
    /// Índice do DAT, montado na primeira consulta e reutilizado depois.
    pub fn dat_index(&self) -> &DatIndex {
        self.index
            .get_or_init(|| dat::parse(self.dat.unwrap_or_default()))
    }

    /// `true` se o sistema tem DAT embutido (cartuchos); discos são
    /// reconhecidos só pela extensão.
    pub fn has_dat(&self) -> bool {
        self.dat.is_some()
    }

    /// `true` se a extensão é deste sistema.
    pub fn accepts(&self, extension: &str) -> bool {
        self.extensions
            .iter()
            .any(|accepted| accepted.eq_ignore_ascii_case(extension))
    }
}

macro_rules! dat_file {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data/dats/",
            $name,
            ".dat"
        ))
    };
}

const fn system(
    console_id: &'static str,
    libretro_name: &'static str,
    extensions: &'static [&'static str],
    format: RomFormat,
    has_embedded_icon: bool,
    dat: Option<&'static str>,
) -> System {
    System {
        console_id,
        libretro_name,
        extensions,
        format,
        has_embedded_icon,
        dat,
        index: OnceLock::new(),
    }
}

static SYSTEMS: [System; 11] = [
    system(
        "master-system",
        "Sega - Master System - Mark III",
        &["sms"],
        RomFormat::Plain,
        false,
        Some(dat_file!("Sega - Master System - Mark III")),
    ),
    system(
        "mega-drive",
        "Sega - Mega Drive - Genesis",
        &["md", "gen", "bin"],
        RomFormat::Plain,
        false,
        Some(dat_file!("Sega - Mega Drive - Genesis")),
    ),
    system(
        "game-boy",
        "Nintendo - Game Boy",
        &["gb"],
        RomFormat::Plain,
        false,
        Some(dat_file!("Nintendo - Game Boy")),
    ),
    system(
        "game-gear",
        "Sega - Game Gear",
        &["gg"],
        RomFormat::Plain,
        false,
        Some(dat_file!("Sega - Game Gear")),
    ),
    system(
        "snes",
        "Nintendo - Super Nintendo Entertainment System",
        &["sfc", "smc"],
        RomFormat::Snes,
        false,
        Some(dat_file!("Nintendo - Super Nintendo Entertainment System")),
    ),
    system(
        "nintendo-64",
        "Nintendo - Nintendo 64",
        &["z64", "n64", "v64"],
        RomFormat::N64,
        false,
        Some(dat_file!("Nintendo - Nintendo 64")),
    ),
    system(
        "game-boy-color",
        "Nintendo - Game Boy Color",
        &["gbc"],
        RomFormat::Plain,
        false,
        Some(dat_file!("Nintendo - Game Boy Color")),
    ),
    system(
        "game-boy-advance",
        "Nintendo - Game Boy Advance",
        &["gba"],
        RomFormat::Plain,
        false,
        Some(dat_file!("Nintendo - Game Boy Advance")),
    ),
    system(
        "nintendo-ds",
        "Nintendo - Nintendo DS",
        &["nds"],
        RomFormat::Plain,
        true,
        Some(dat_file!("Nintendo - Nintendo DS")),
    ),
    system(
        "nintendo-3ds",
        "Nintendo - Nintendo 3DS",
        &["3ds", "cci"],
        RomFormat::Plain,
        false,
        Some(dat_file!("Nintendo - Nintendo 3DS")),
    ),
    // Só o `.cue`: o `.bin` do disco é do Mega Drive na varredura, e os
    // `.bin` grandes nem são lidos.
    system(
        "playstation",
        "Sony - PlayStation",
        &["cue"],
        RomFormat::Plain,
        false,
        None,
    ),
];

/// Sistema reconhecível do console, ou `None` se ainda não houver suporte.
pub fn for_console(console_id: &str) -> Option<&'static System> {
    SYSTEMS
        .iter()
        .find(|system| system.console_id == console_id)
}

/// Sistema dono da extensão (sem distinguir maiúsculas), se houver.
pub fn for_extension(extension: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|system| system.accepts(extension))
}

/// Procura a ROM nos DATs de todos os sistemas. Devolve o sistema e o nome
/// No-Intro do jogo.
pub fn identify(size: u64, crc32: u32) -> Option<(&'static System, &'static str)> {
    SYSTEMS.iter().find_map(|system| {
        system
            .dat_index()
            .lookup(size, crc32)
            .map(|name| (system, name))
    })
}

/// Todos os sistemas reconhecíveis.
pub fn all() -> &'static [System] {
    &SYSTEMS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::consoles;

    #[test]
    fn every_system_maps_to_a_catalog_console() {
        for system in all() {
            assert!(
                consoles().iter().any(|c| c.id == system.console_id),
                "{} não existe no catálogo",
                system.console_id
            );
        }
    }

    #[test]
    fn every_embedded_dat_parses_into_a_non_empty_index() {
        for system in all().iter().filter(|s| s.dat.is_some()) {
            assert!(
                system.dat_index().len() > 100,
                "{}: DAT vazio ou ilegível",
                system.console_id
            );
        }
    }

    #[test]
    fn identifies_a_known_dump() {
        let snes = for_console("snes").expect("SNES é suportado");
        assert_eq!(
            snes.dat_index().lookup(524_288, 0xB19E_D489),
            Some("Super Mario World (USA)")
        );
    }

    #[test]
    fn extension_matching_ignores_case() {
        let gba = for_console("game-boy-advance").expect("GBA é suportado");
        assert!(gba.accepts("GBA"));
        assert!(!gba.accepts("gb"));
    }

    #[test]
    fn each_extension_has_a_single_owner_and_zip_has_none() {
        for system in all() {
            for ext in system.extensions {
                let owner = for_extension(ext).expect("extensão tem dono");
                assert_eq!(owner.console_id, system.console_id, "{ext} repetida");
            }
        }
        assert_eq!(
            for_extension("GBA").map(|s| s.console_id),
            Some("game-boy-advance")
        );
        assert!(for_extension("zip").is_none());
        assert!(for_extension("txt").is_none());
    }

    #[test]
    fn identifies_across_all_dats() {
        let (system, name) = identify(524_288, 0xB19E_D489).expect("SMW está no DAT do SNES");
        assert_eq!(
            (system.console_id, name),
            ("snes", "Super Mario World (USA)")
        );
        assert!(identify(1, 0).is_none());
    }

    #[test]
    fn playstation_discs_are_found_by_their_cue_sheet() {
        let playstation = for_console("playstation").expect("PlayStation é suportado");
        assert!(playstation.accepts("CUE"));
        assert!(!playstation.accepts("bin"), "o .bin é do Mega Drive");
        assert_eq!(playstation.dat_index().len(), 0, "sem DAT");
        assert!(for_console("playstation-2").is_none());
        assert!(for_console("inexistente").is_none());
    }
}
