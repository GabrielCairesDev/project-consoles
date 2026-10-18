//! Catálogo estático dos principais consoles oficiais.
//!
//! O catálogo é o domínio puro: não depende do Tauri nem da interface. Os
//! `id`s são estáveis e em kebab-case, pois a interface os usa para localizar
//! o logo de cada console (`src/assets/consoles/<id>.svg`).
//!
//! Entram apenas consoles com pelo menos [`MIN_UNITS_SOLD`] unidades vendidas.

use serde::Serialize;

/// Corte de relevância: consoles abaixo disso ficam fora do catálogo.
pub const MIN_UNITS_SOLD: u64 = 5_000_000;

/// Origem dos números de vendas.
///
/// Wikipedia, "List of best-selling game consoles", revisão 1373483872
/// (outubro de 2026). Consoles ainda à venda têm números que mudam com o
/// tempo.
pub const SALES_SOURCE: &str = "Wikipedia — List of best-selling game consoles (rev. 1373483872)";

/// Categoria de hardware de um console.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConsoleKind {
    /// Console de mesa, ligado à TV.
    Home,
    /// Console portátil.
    Handheld,
    /// Funciona tanto na TV quanto como portátil.
    Hybrid,
}

/// Um console oficial lançado comercialmente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Console {
    /// Identificador estável em kebab-case.
    pub id: &'static str,
    /// Nome comercial; consoles com nomes regionais usam "Nome A / Nome B".
    pub name: &'static str,
    /// Fabricante original.
    pub manufacturer: &'static str,
    /// Ano do primeiro lançamento em qualquer região.
    pub release_year: u16,
    /// Categoria de hardware.
    pub kind: ConsoleKind,
    /// Unidades vendidas segundo [`SALES_SOURCE`].
    ///
    /// Game Boy e Game Boy Color são contabilizados juntos pela fonte; os
    /// dois registros carregam o mesmo total combinado.
    pub units_sold: u64,
}

const fn console(
    id: &'static str,
    name: &'static str,
    manufacturer: &'static str,
    release_year: u16,
    kind: ConsoleKind,
    units_sold: u64,
) -> Console {
    Console {
        id,
        name,
        manufacturer,
        release_year,
        kind,
        units_sold,
    }
}

use ConsoleKind::{Handheld, Home, Hybrid};

/// Consoles ordenados por ano de lançamento e, dentro do ano, por nome.
const CONSOLES: &[Console] = &[
    console(
        "master-system",
        "Master System",
        "Sega",
        1985,
        Home,
        13_000_000,
    ),
    console(
        "mega-drive",
        "Mega Drive / Genesis",
        "Sega",
        1988,
        Home,
        30_750_000,
    ),
    console(
        "game-boy",
        "Game Boy",
        "Nintendo",
        1989,
        Handheld,
        118_690_000,
    ),
    console("game-gear", "Game Gear", "Sega", 1990, Handheld, 14_000_000),
    console(
        "snes",
        "Super Famicom / SNES",
        "Nintendo",
        1990,
        Home,
        49_100_000,
    ),
    console(
        "playstation",
        "PlayStation",
        "Sony",
        1994,
        Home,
        102_490_000,
    ),
    console("sega-saturn", "Sega Saturn", "Sega", 1994, Home, 9_260_000),
    console(
        "nintendo-64",
        "Nintendo 64",
        "Nintendo",
        1996,
        Home,
        32_930_000,
    ),
    console("dreamcast", "Dreamcast", "Sega", 1998, Home, 9_130_000),
    console(
        "game-boy-color",
        "Game Boy Color",
        "Nintendo",
        1998,
        Handheld,
        118_690_000,
    ),
    console(
        "playstation-2",
        "PlayStation 2",
        "Sony",
        2000,
        Home,
        160_000_000,
    ),
    console(
        "game-boy-advance",
        "Game Boy Advance",
        "Nintendo",
        2001,
        Handheld,
        81_510_000,
    ),
    console("gamecube", "GameCube", "Nintendo", 2001, Home, 21_740_000),
    console("xbox", "Xbox", "Microsoft", 2001, Home, 24_000_000),
    console(
        "nintendo-ds",
        "Nintendo DS",
        "Nintendo",
        2004,
        Handheld,
        154_020_000,
    ),
    console("psp", "PSP", "Sony", 2004, Handheld, 80_000_000),
    console("xbox-360", "Xbox 360", "Microsoft", 2005, Home, 84_000_000),
    console(
        "playstation-3",
        "PlayStation 3",
        "Sony",
        2006,
        Home,
        87_400_000,
    ),
    console("wii", "Wii", "Nintendo", 2006, Home, 101_630_000),
    console(
        "nintendo-3ds",
        "Nintendo 3DS",
        "Nintendo",
        2011,
        Handheld,
        75_940_000,
    ),
    console("ps-vita", "PS Vita", "Sony", 2011, Handheld, 16_000_000),
    console("wii-u", "Wii U", "Nintendo", 2012, Home, 13_560_000),
    console(
        "playstation-4",
        "PlayStation 4",
        "Sony",
        2013,
        Home,
        117_200_000,
    ),
    console("xbox-one", "Xbox One", "Microsoft", 2013, Home, 58_000_000),
    console(
        "switch",
        "Nintendo Switch",
        "Nintendo",
        2017,
        Hybrid,
        156_590_000,
    ),
];

/// Retorna todos os consoles, ordenados por ano de lançamento e nome.
pub fn consoles() -> &'static [Console] {
    CONSOLES
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for console in consoles() {
            assert!(seen.insert(console.id), "id duplicado: {}", console.id);
        }
    }

    #[test]
    fn ids_are_kebab_case() {
        for console in consoles() {
            let valid = !console.id.is_empty()
                && !console.id.starts_with('-')
                && !console.id.ends_with('-')
                && !console.id.contains("--")
                && console
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            assert!(valid, "id fora do padrão kebab-case: {:?}", console.id);
        }
    }

    #[test]
    fn sorted_by_year_then_name() {
        let is_sorted = consoles().windows(2).all(|pair| {
            (pair[0].release_year, pair[0].name) <= (pair[1].release_year, pair[1].name)
        });
        assert!(is_sorted, "catálogo fora de ordem (ano, nome)");
    }

    #[test]
    fn text_fields_are_filled() {
        for console in consoles() {
            assert!(
                !console.name.trim().is_empty(),
                "{}: nome vazio",
                console.id
            );
            assert!(
                !console.manufacturer.trim().is_empty(),
                "{}: fabricante vazio",
                console.id
            );
        }
    }

    #[test]
    fn catalog_stops_before_the_2020s() {
        for console in consoles() {
            assert!(
                console.release_year < 2020,
                "{}: consoles a partir de 2020 ficam fora do catálogo",
                console.id
            );
        }
    }

    #[test]
    fn release_years_are_plausible() {
        // 1972 é o ano do primeiro console doméstico (Magnavox Odyssey).
        for console in consoles() {
            assert!(
                (1972..=2100).contains(&console.release_year),
                "{}: ano implausível {}",
                console.id,
                console.release_year
            );
        }
    }

    #[test]
    fn every_console_meets_relevance_cut() {
        for console in consoles() {
            assert!(
                console.units_sold >= MIN_UNITS_SOLD,
                "{}: {} unidades, abaixo do corte de {MIN_UNITS_SOLD}",
                console.id,
                console.units_sold
            );
        }
    }

    #[test]
    fn serializes_with_camel_case_contract() {
        let json = serde_json::to_value(consoles()[0]).expect("Console é serializável");
        assert_eq!(
            json,
            serde_json::json!({
                "id": "master-system",
                "name": "Master System",
                "manufacturer": "Sega",
                "releaseYear": 1985,
                "kind": "home",
                "unitsSold": 13_000_000,
            })
        );
        assert_eq!(
            serde_json::to_value(ConsoleKind::Handheld).expect("ConsoleKind é serializável"),
            "handheld"
        );
    }
}
