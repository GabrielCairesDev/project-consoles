//! Capas dos jogos, com cache em disco.
//!
//! Ordem de resolução:
//! 1. capa de caixa já em cache;
//! 2. download do acervo libretro-thumbnails (pelo nome No-Intro). Quando a
//!    capa não existe lá, um marcador evita perguntar de novo. Sem rede, nada
//!    é marcado e a próxima abertura tenta outra vez;
//! 3. ícone embutido na ROM (Nintendo DS), convertido para PNG.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::library::Game;
use crate::systems::System;

const LIBRETRO_THUMBNAILS: &str = "https://thumbnails.libretro.com";
/// Limite de tamanho de uma capa baixada; capas reais têm algumas centenas de KB.
const MAX_BOXART_BYTES: u64 = 10 * 1024 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CoverKind {
    /// Capa da caixa.
    Boxart,
    /// Ícone de baixa resolução extraído da ROM.
    Icon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cover {
    pub path: PathBuf,
    pub kind: CoverKind,
}

/// Resultado de uma consulta ao acervo de capas.
#[derive(Debug)]
pub enum Fetched {
    Found(Vec<u8>),
    /// O acervo respondeu que a capa não existe.
    NotFound,
    /// Falha de rede ou resposta inesperada: tentar de novo depois.
    Unavailable,
}

/// Origem das capas de caixa. Abstraída para os testes não usarem rede.
pub trait BoxartSource {
    fn fetch(&self, system: &str, file_name: &str) -> Fetched;
}

/// Acervo público libretro-thumbnails (o mesmo usado pelo RetroArch).
pub struct LibretroThumbnails {
    agent: ureq::Agent,
}

impl Default for LibretroThumbnails {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .into();
        Self { agent }
    }
}

impl BoxartSource for LibretroThumbnails {
    fn fetch(&self, system: &str, file_name: &str) -> Fetched {
        let url = format!(
            "{LIBRETRO_THUMBNAILS}/{}/Named_Boxarts/{}.png",
            percent_encode(system),
            percent_encode(file_name)
        );
        let mut response = match self.agent.get(&url).call() {
            Ok(response) => response,
            Err(error) => {
                eprintln!("capa indisponível ({url}): {error}");
                return Fetched::Unavailable;
            }
        };
        match response.status().as_u16() {
            200 => match response
                .body_mut()
                .with_config()
                .limit(MAX_BOXART_BYTES)
                .read_to_vec()
            {
                Ok(bytes) if bytes.starts_with(PNG_SIGNATURE) => Fetched::Found(bytes),
                Ok(_) => Fetched::Unavailable,
                Err(error) => {
                    eprintln!("falha ao baixar capa ({url}): {error}");
                    Fetched::Unavailable
                }
            },
            404 => Fetched::NotFound,
            _ => Fetched::Unavailable,
        }
    }
}

/// Nome de arquivo no acervo libretro: estes caracteres viram `_`.
pub fn libretro_file_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '&' | '*' | '/' | ':' | '`' | '<' | '>' | '?' | '\\' | '|' | '"' => '_',
            other => other,
        })
        .collect()
}

/// Codifica um segmento de URL (RFC 3986): só os caracteres não reservados
/// passam sem escape.
fn percent_encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len() * 3);
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Cache de capas em `root/<sistema>/`.
pub struct CoverStore {
    root: PathBuf,
}

impl CoverStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn resolve(
        &self,
        system: &System,
        game: &Game,
        source: &dyn BoxartSource,
    ) -> io::Result<Option<Cover>> {
        let dir = self.root.join(libretro_file_name(system.libretro_name));

        if let Some(name) = &game.no_intro_name {
            let file_name = libretro_file_name(name);
            let boxart = dir.join(format!("{file_name}.png"));
            if boxart.is_file() {
                return Ok(Some(Cover {
                    path: boxart,
                    kind: CoverKind::Boxart,
                }));
            }
            let missing_marker = dir.join(format!("{file_name}.none"));
            if !missing_marker.exists() {
                match source.fetch(system.libretro_name, &file_name) {
                    Fetched::Found(bytes) => {
                        write_atomically(&boxart, &bytes)?;
                        return Ok(Some(Cover {
                            path: boxart,
                            kind: CoverKind::Boxart,
                        }));
                    }
                    Fetched::NotFound => write_atomically(&missing_marker, b"")?,
                    Fetched::Unavailable => {}
                }
            }
        }

        if system.has_embedded_icon {
            let key = match &game.no_intro_name {
                Some(name) => libretro_file_name(name),
                None => format!("arquivo-{}", libretro_file_name(&game.file_name)),
            };
            let icon = dir.join("icons").join(format!("{key}.png"));
            if icon.is_file() {
                return Ok(Some(Cover {
                    path: icon,
                    kind: CoverKind::Icon,
                }));
            }
            if let Some(png) = read_nds_icon(&game.path)? {
                write_atomically(&icon, &png)?;
                return Ok(Some(Cover {
                    path: icon,
                    kind: CoverKind::Icon,
                }));
            }
        }
        Ok(None)
    }
}

/// Escreve em arquivo temporário e renomeia: leitores nunca veem um PNG pela metade.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Sufixo sobre o nome inteiro: `X.png` e `X.none` não disputam o mesmo temporário.
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

// --- Ícone do Nintendo DS -------------------------------------------------

const NDS_BANNER_OFFSET_FIELD: u64 = 0x68;
const NDS_ICON_SIDE: usize = 32;
const NDS_BITMAP_START: usize = 0x20;
const NDS_BITMAP_LEN: usize = 0x200;
const NDS_PALETTE_START: usize = 0x220;
const NDS_BANNER_MIN_LEN: usize = 0x240;

/// Extrai o ícone 32×32 do banner de uma ROM `.nds` e devolve um PNG.
/// `Ok(None)` quando o arquivo não é uma ROM de DS com banner (ex.: `.zip`).
fn read_nds_icon(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let is_nds = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("nds"));
    if !is_nds {
        return Ok(None);
    }
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();

    let mut offset_field = [0u8; 4];
    file.seek(SeekFrom::Start(NDS_BANNER_OFFSET_FIELD))?;
    if file.read_exact(&mut offset_field).is_err() {
        return Ok(None);
    }
    let banner_offset = u64::from(u32::from_le_bytes(offset_field));
    if banner_offset == 0 || banner_offset + NDS_BANNER_MIN_LEN as u64 > file_len {
        return Ok(None);
    }

    let mut banner = vec![0u8; NDS_BANNER_MIN_LEN];
    file.seek(SeekFrom::Start(banner_offset))?;
    file.read_exact(&mut banner)?;
    match nds_icon_rgba(&banner) {
        Some(rgba) => encode_png(&rgba, NDS_ICON_SIDE as u32).map(Some),
        None => Ok(None),
    }
}

/// Decodifica o ícone: 4 bits por pixel, em blocos de 8×8 dispostos em 4×4,
/// com paleta de 16 cores BGR555; o índice 0 é transparente.
fn nds_icon_rgba(banner: &[u8]) -> Option<Vec<u8>> {
    let version = u16::from_le_bytes([*banner.first()?, *banner.get(1)?]);
    if !matches!(version, 0x0001 | 0x0002 | 0x0003 | 0x0103) {
        return None;
    }
    let bitmap = banner.get(NDS_BITMAP_START..NDS_BITMAP_START + NDS_BITMAP_LEN)?;
    let palette: Vec<[u8; 4]> = banner
        .get(NDS_PALETTE_START..NDS_PALETTE_START + 32)?
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
        .map(|(i, &raw)| {
            let color = u16::from_le_bytes(raw);
            let channel = |shift: u16| {
                let five = ((color >> shift) & 0x1F) as u8;
                (five << 3) | (five >> 2)
            };
            let alpha = if i == 0 { 0 } else { 255 };
            [channel(0), channel(5), channel(10), alpha]
        })
        .collect();

    let mut rgba = vec![0u8; NDS_ICON_SIDE * NDS_ICON_SIDE * 4];
    for (byte_index, byte) in bitmap.iter().enumerate() {
        let tile = byte_index / 32;
        let within = byte_index % 32;
        let (tile_x, tile_y) = (tile % 4, tile / 4);
        let (row, col) = (within / 4, (within % 4) * 2);
        for (dx, nibble) in [byte & 0x0F, byte >> 4].into_iter().enumerate() {
            let x = tile_x * 8 + col + dx;
            let y = tile_y * 8 + row;
            let start = (y * NDS_ICON_SIDE + x) * 4;
            rgba[start..start + 4].copy_from_slice(&palette[nibble as usize]);
        }
    }
    Some(rgba)
}

fn encode_png(rgba: &[u8], side: u32) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    writer.write_image_data(rgba).map_err(io::Error::other)?;
    writer.finish().map_err(io::Error::other)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems;
    use std::cell::Cell;

    struct FakeSource {
        response: fn() -> Fetched,
        calls: Cell<usize>,
    }

    impl FakeSource {
        fn new(response: fn() -> Fetched) -> Self {
            Self {
                response,
                calls: Cell::new(0),
            }
        }
    }

    impl BoxartSource for FakeSource {
        fn fetch(&self, _system: &str, _file_name: &str) -> Fetched {
            self.calls.set(self.calls.get() + 1);
            (self.response)()
        }
    }

    fn fake_png() -> Fetched {
        Fetched::Found(encode_png(&[255; 4], 1).expect("png"))
    }

    fn game(path: PathBuf, no_intro_name: Option<&str>) -> Game {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Game {
            id: file_name.clone(),
            console_id: "game-boy".into(),
            title: file_name.clone(),
            file_name,
            recognized: no_intro_name.is_some(),
            path,
            no_intro_name: no_intro_name.map(str::to_owned),
        }
    }

    /// ROM de DS mínima: cabeçalho apontando para um banner com paleta e
    /// bitmap conhecidos (pixel (0,0) usa a cor 1; (1,0) usa a cor 0).
    fn synthetic_nds(dir: &Path) -> PathBuf {
        let banner_offset = 0x200usize;
        let mut rom = vec![0u8; banner_offset + NDS_BANNER_MIN_LEN];
        rom[0x68..0x6C].copy_from_slice(&(banner_offset as u32).to_le_bytes());
        let banner = &mut rom[banner_offset..];
        banner[0..2].copy_from_slice(&1u16.to_le_bytes());
        banner[NDS_BITMAP_START] = 0x01; // nibble baixo = pixel x=0 → cor 1
        let red_bgr555: u16 = 0x001F;
        banner[NDS_PALETTE_START + 2..NDS_PALETTE_START + 4]
            .copy_from_slice(&red_bgr555.to_le_bytes());
        let path = dir.join("jogo.nds");
        fs::write(&path, rom).expect("rom de teste");
        path
    }

    #[test]
    fn sanitizes_names_like_libretro() {
        assert_eq!(
            libretro_file_name(r#"Q*bert: A/B <C> "D" | E? \F` & G"#),
            "Q_bert_ A_B _C_ _D_ _ E_ _F_ _ G"
        );
        assert_eq!(libretro_file_name("Pokémon (USA)"), "Pokémon (USA)");
    }

    #[test]
    fn percent_encodes_path_segments() {
        assert_eq!(
            percent_encode("Nintendo - Game Boy"),
            "Nintendo%20-%20Game%20Boy"
        );
        assert_eq!(percent_encode("Pokémon (USA)"), "Pok%C3%A9mon%20%28USA%29");
    }

    #[test]
    fn downloads_once_then_serves_from_cache() {
        let root = tempfile::tempdir().expect("cache");
        let store = CoverStore::new(root.path());
        let gb = systems::for_console("game-boy").expect("GB");
        let source = FakeSource::new(fake_png);
        let g = game(PathBuf::from("/x/tetris.gb"), Some("Tetris (World)"));

        let first = store
            .resolve(gb, &g, &source)
            .expect("resolve")
            .expect("capa");
        let second = store
            .resolve(gb, &g, &source)
            .expect("resolve")
            .expect("capa");

        assert_eq!(first, second);
        assert_eq!(first.kind, CoverKind::Boxart);
        assert!(
            first
                .path
                .ends_with("Nintendo - Game Boy/Tetris (World).png")
        );
        assert_eq!(source.calls.get(), 1);
    }

    #[test]
    fn remembers_missing_covers_but_retries_when_offline() {
        let root = tempfile::tempdir().expect("cache");
        let store = CoverStore::new(root.path());
        let gb = systems::for_console("game-boy").expect("GB");
        let g = game(PathBuf::from("/x/a.gb"), Some("A (USA)"));

        let missing = FakeSource::new(|| Fetched::NotFound);
        assert_eq!(store.resolve(gb, &g, &missing).expect("resolve"), None);
        assert_eq!(store.resolve(gb, &g, &missing).expect("resolve"), None);
        assert_eq!(missing.calls.get(), 1);

        let g2 = game(PathBuf::from("/x/b.gb"), Some("B (USA)"));
        let offline = FakeSource::new(|| Fetched::Unavailable);
        assert_eq!(store.resolve(gb, &g2, &offline).expect("resolve"), None);
        assert_eq!(store.resolve(gb, &g2, &offline).expect("resolve"), None);
        assert_eq!(offline.calls.get(), 2);
    }

    #[test]
    fn unrecognized_games_skip_the_online_archive() {
        let root = tempfile::tempdir().expect("cache");
        let store = CoverStore::new(root.path());
        let gb = systems::for_console("game-boy").expect("GB");
        let source = FakeSource::new(fake_png);

        let result = store
            .resolve(gb, &game(PathBuf::from("/x/caseiro.gb"), None), &source)
            .expect("resolve");
        assert_eq!(result, None);
        assert_eq!(source.calls.get(), 0);
    }

    #[test]
    fn falls_back_to_nds_icon_when_archive_has_no_cover() {
        let roms = tempfile::tempdir().expect("roms");
        let root = tempfile::tempdir().expect("cache");
        let store = CoverStore::new(root.path());
        let nds = systems::for_console("nintendo-ds").expect("DS");
        let g = game(synthetic_nds(roms.path()), Some("Jogo (Brazil)"));

        let cover = store
            .resolve(nds, &g, &FakeSource::new(|| Fetched::NotFound))
            .expect("resolve")
            .expect("ícone");
        assert_eq!(cover.kind, CoverKind::Icon);

        let decoder = png::Decoder::new(io::BufReader::new(File::open(&cover.path).expect("png")));
        let mut reader = decoder.read_info().expect("cabeçalho png");
        let mut pixels = vec![0; reader.output_buffer_size().expect("tamanho")];
        reader.next_frame(&mut pixels).expect("pixels");
        assert_eq!((reader.info().width, reader.info().height), (32, 32));
        assert_eq!(&pixels[0..4], &[255, 0, 0, 255], "pixel (0,0) vermelho");
        assert_eq!(pixels[7], 0, "pixel (1,0) transparente");
    }

    #[test]
    fn rejects_banner_with_unknown_version() {
        let mut banner = vec![0u8; NDS_BANNER_MIN_LEN];
        banner[0..2].copy_from_slice(&0x7777u16.to_le_bytes());
        assert!(nds_icon_rgba(&banner).is_none());
        assert!(nds_icon_rgba(&banner[..10]).is_none());
    }

    #[test]
    fn non_nds_or_truncated_files_have_no_icon() {
        let dir = tempfile::tempdir().expect("pasta");
        let zip = dir.path().join("jogo.zip");
        fs::write(&zip, b"PK").expect("arquivo");
        let short = dir.path().join("curto.nds");
        fs::write(&short, b"pequeno").expect("arquivo");

        assert_eq!(read_nds_icon(&zip).expect("leitura"), None);
        assert_eq!(read_nds_icon(&short).expect("leitura"), None);
    }
}
