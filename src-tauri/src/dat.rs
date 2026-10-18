//! Leitura de DATs no formato clrmamepro (No-Intro via libretro-database).
//!
//! Só o necessário para identificar ROMs: cada linha `rom ( … size N crc X … )`
//! é indexada por `(tamanho, CRC32)` e aponta para o `name` do `game` que a
//! contém. Não há dependência externa: o formato é estável e orientado a linhas.
//!
//! Discos (Redump) são grandes demais para o CRC na varredura; o índice deles
//! é pelo número de série do `game` ([`parse_serials`]).

use std::collections::HashMap;

/// Índice de identificação de um sistema.
#[derive(Debug, Default)]
pub struct DatIndex {
    by_fingerprint: HashMap<(u64, u32), String>,
}

impl DatIndex {
    /// Nome No-Intro do jogo com esse tamanho e CRC32, se conhecido.
    pub fn lookup(&self, size: u64, crc32: u32) -> Option<&str> {
        self.by_fingerprint.get(&(size, crc32)).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.by_fingerprint.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_fingerprint.is_empty()
    }
}

/// Índice de discos pelo número de série.
#[derive(Debug, Default)]
pub struct SerialIndex {
    /// Discos de cada número, na ordem do DAT: (tamanho do primeiro arquivo,
    /// nome).
    by_serial: HashMap<String, Vec<(u64, String)>>,
}

impl SerialIndex {
    /// Nome Redump do disco. Revisões e versões do mesmo jogo podem repetir
    /// o número; vale a de primeiro arquivo do mesmo tamanho e, sem ela, a
    /// primeira do DAT.
    pub fn lookup(&self, serial: &str, first_file_size: u64) -> Option<&str> {
        let discs = self.by_serial.get(serial)?;
        discs
            .iter()
            .find(|(size, _)| *size == first_file_size)
            .or(discs.first())
            .map(|(_, name)| name.as_str())
    }

    pub fn len(&self) -> usize {
        self.by_serial.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_serial.is_empty()
    }
}

/// Monta o índice de discos de um DAT Redump do libretro: cada `game` com
/// `serial "…"` (às vezes vários, separados por vírgula) e o tamanho da sua
/// primeira `rom`.
pub fn parse_serials(text: &str) -> SerialIndex {
    #[derive(Default)]
    struct Disc<'a> {
        name: Option<&'a str>,
        serial: Option<&'a str>,
        size: Option<u64>,
    }
    let mut index = SerialIndex::default();
    let mut add = |disc: &Disc| {
        let (Some(name), Some(serials)) = (disc.name, disc.serial) else {
            return;
        };
        for serial in serials.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            index
                .by_serial
                .entry(serial.to_owned())
                .or_default()
                .push((disc.size.unwrap_or(0), name.to_owned()));
        }
    };
    let mut current: Option<Disc> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("game (") {
            if let Some(disc) = current.take() {
                add(&disc);
            }
            current = Some(Disc::default());
        } else if let Some(disc) = current.as_mut() {
            if let Some(rest) = line.strip_prefix("name ") {
                disc.name = disc.name.or(quoted(rest));
            } else if let Some(rest) = line.strip_prefix("serial ") {
                disc.serial = disc.serial.or(quoted(rest));
            } else if line.starts_with("rom (") && disc.size.is_none() {
                disc.size = field(line, "size").and_then(|size| size.parse().ok());
            }
        }
    }
    if let Some(disc) = current {
        add(&disc);
    }
    index
}

/// Monta o índice a partir do texto de um DAT. Linhas malformadas são
/// ignoradas: um DAT parcialmente inválido ainda identifica o restante.
pub fn parse(text: &str) -> DatIndex {
    let mut index = DatIndex::default();
    let mut in_game = false;
    let mut current_game: Option<&str> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("game (") {
            in_game = true;
            current_game = None;
        } else if let Some(rest) = line.strip_prefix("name ") {
            // O `name` do cabeçalho `clrmamepro ( … )` não é um jogo.
            if in_game && current_game.is_none() {
                current_game = quoted(rest);
            }
        } else if line.starts_with("rom (") {
            let (Some(game), Some(size), Some(crc)) =
                (current_game, field(line, "size"), field(line, "crc"))
            else {
                continue;
            };
            if let (Ok(size), Ok(crc)) = (size.parse::<u64>(), u32::from_str_radix(crc, 16)) {
                // Em caso de colisão, mantém a primeira entrada (ordem do DAT).
                index
                    .by_fingerprint
                    .entry((size, crc))
                    .or_insert_with(|| game.to_owned());
            }
        }
    }
    index
}

/// Conteúdo entre as primeiras aspas de `s`.
fn quoted(s: &str) -> Option<&str> {
    let start = s.find('"')? + 1;
    let len = s[start..].find('"')?;
    Some(&s[start..start + len])
}

/// Valor do token após `key` numa linha `rom ( … )`, ignorando o trecho entre
/// aspas do nome do arquivo (que pode conter as mesmas palavras).
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let after_name = match line.find("name \"") {
        Some(pos) => {
            let rest = &line[pos + 6..];
            &rest[rest.find('"')? + 1..]
        }
        None => line,
    };
    let mut tokens = after_name.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == key {
            return tokens.next();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"clrmamepro (
	name "Nintendo - Super Nintendo Entertainment System"
	version "2026.10.07"
)

game (
	name "Super Mario World (USA)"
	region "USA"
	rom ( name "Super Mario World (USA).sfc" size 524288 crc B19ED489 md5 CDD3C8C37322978CA8669B34BC89C804 sha1 6B47BB75D16514B6A476AA0C73A683A2A4C18765 )
)
game (
	name "Jogo crc size (Teste)"
	rom ( name "crc 1 size 2.sfc" size 1048576 crc 0000ABCD )
	rom ( name "parte 2.sfc" size 2048 crc 12345678 )
)
game (
	name "Sem CRC"
	rom ( name "x.sfc" size 10 )
)
"#;

    #[test]
    fn indexes_by_size_and_crc() {
        let index = parse(SAMPLE);

        assert_eq!(
            index.lookup(524_288, 0xB19E_D489),
            Some("Super Mario World (USA)")
        );
        assert_eq!(index.lookup(524_288, 0xDEAD_BEEF), None);
        assert_eq!(index.lookup(1, 0xB19E_D489), None);
    }

    #[test]
    fn ignores_keywords_inside_rom_file_name() {
        let index = parse(SAMPLE);

        assert_eq!(
            index.lookup(1_048_576, 0x0000_ABCD),
            Some("Jogo crc size (Teste)")
        );
    }

    #[test]
    fn maps_every_rom_of_a_multi_file_game() {
        let index = parse(SAMPLE);

        assert_eq!(
            index.lookup(2048, 0x1234_5678),
            Some("Jogo crc size (Teste)")
        );
    }

    #[test]
    fn skips_entries_without_crc_and_header_name() {
        let index = parse(SAMPLE);

        assert_eq!(index.len(), 3);
    }

    const DISCS: &str = r#"clrmamepro (
	name "Sony - PlayStation"
	serial "NAO-E-JOGO"
)

game (
	name "Ace Combat 2 (Japan)"
	serial "SLPS-00830"
	rom ( name "Ace Combat 2 (Japan) (Track 1).bin" size 580273680 crc 1 serial "SLPS-00830" )
	rom ( name "Ace Combat 2 (Japan) (Track 2).bin" size 999 crc 2 )
)
game (
	name "Ace Combat 2 (Japan) (Rev 1)"
	serial "SLPS-00830"
	rom ( name "Ace Combat 2 (Japan) (Rev 1) (Track 1).bin" size 580271328 crc 3 )
)
game (
	name "Dois Numeros (USA)"
	serial "SLUS-00001, SLUS-00002"
	rom ( name "x.bin" size 10 crc 4 )
)
game (
	name "Sem Numero (USA)"
	rom ( name "y.bin" size 10 crc 5 )
)
"#;

    #[test]
    fn discs_are_found_by_serial_and_first_file_size() {
        let index = parse_serials(DISCS);

        assert_eq!(index.len(), 3, "sem o cabeçalho e o jogo sem número");
        assert_eq!(
            index.lookup("SLPS-00830", 580_271_328),
            Some("Ace Combat 2 (Japan) (Rev 1)")
        );
        assert_eq!(
            index.lookup("SLPS-00830", 580_273_680),
            Some("Ace Combat 2 (Japan)")
        );
        assert_eq!(
            index.lookup("SLPS-00830", 999),
            Some("Ace Combat 2 (Japan)"),
            "tamanho desconhecido: o primeiro do DAT"
        );
        assert_eq!(index.lookup("SLUS-00002", 10), Some("Dois Numeros (USA)"));
        assert_eq!(index.lookup("SLUS-99999", 10), None);
        assert_eq!(parse_serials("").len(), 0);
    }

    #[test]
    fn empty_or_garbage_input_yields_empty_index() {
        assert!(parse("").is_empty());
        assert!(parse("rom ( size abc crc zz )\n game (").is_empty());
    }
}
