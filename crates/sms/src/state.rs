//! Formato binário dos estados salvos ("save states").
//!
//! Layout: `MAGIC`, versão (`u16`), hash da ROM (`u64`) e as seções de CPU,
//! barramento, VDP e PSG, em little-endian e sem preenchimento. O hash
//! impede carregar o estado de outro jogo; a versão permite mudar o formato
//! no futuro sem interpretar lixo. Todo valor lido que vira índice ou enum é
//! validado: um arquivo corrompido gera erro, nunca pânico.

use std::fmt;

pub(crate) const MAGIC: &[u8; 8] = b"PCSMSST\0";
pub(crate) const VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// Não é um estado deste núcleo.
    NotAState,
    /// Estado de uma versão de formato que este núcleo não lê.
    UnsupportedVersion(u16),
    /// Estado salvo com outra ROM.
    WrongRom,
    Truncated,
    /// Valor fora do intervalo válido (arquivo corrompido).
    Invalid(&'static str),
    TrailingData,
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAState => write!(f, "o arquivo não é um estado salvo"),
            Self::UnsupportedVersion(v) => write!(f, "versão de estado não suportada: {v}"),
            Self::WrongRom => write!(f, "o estado salvo é de outro jogo"),
            Self::Truncated => write!(f, "o estado salvo está incompleto"),
            Self::Invalid(what) => write!(f, "o estado salvo está corrompido ({what})"),
            Self::TrailingData => write!(f, "o estado salvo tem dados a mais"),
        }
    }
}

impl std::error::Error for StateError {}

/// Hash FNV-1a de 64 bits: rápido, estável entre versões e sem dependência.
pub(crate) fn rom_hash(rom: &[u8]) -> u64 {
    rom.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[derive(Default)]
pub(crate) struct Writer {
    pub bytes: Vec<u8>,
}

impl Writer {
    pub fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    pub fn u16(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn slice(&mut self, v: &[u8]) {
        self.bytes.extend_from_slice(v);
    }
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], StateError> {
        if self.bytes.len() < n {
            return Err(StateError::Truncated);
        }
        let (head, rest) = self.bytes.split_at(n);
        self.bytes = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StateError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, StateError> {
        Ok(self.array::<1>()?[0])
    }
    pub fn bool(&mut self) -> Result<bool, StateError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(StateError::Invalid("booleano")),
        }
    }
    pub fn u16(&mut self) -> Result<u16, StateError> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    pub fn u32(&mut self) -> Result<u32, StateError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub fn u64(&mut self) -> Result<u64, StateError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub fn fill(&mut self, out: &mut [u8]) -> Result<(), StateError> {
        out.copy_from_slice(self.take(out.len())?);
        Ok(())
    }

    pub fn finish(self) -> Result<(), StateError> {
        if self.bytes.is_empty() {
            Ok(())
        } else {
            Err(StateError::TrailingData)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_round_trip() {
        let mut w = Writer::default();
        w.u8(7);
        w.bool(true);
        w.u16(0xBEEF);
        w.u32(0xDEAD_BEEF);
        w.u64(u64::MAX - 1);
        w.slice(&[1, 2, 3]);
        let mut r = Reader::new(&w.bytes);
        assert_eq!(r.u8(), Ok(7));
        assert_eq!(r.bool(), Ok(true));
        assert_eq!(r.u16(), Ok(0xBEEF));
        assert_eq!(r.u32(), Ok(0xDEAD_BEEF));
        assert_eq!(r.u64(), Ok(u64::MAX - 1));
        let mut buf = [0; 3];
        assert_eq!(r.fill(&mut buf), Ok(()));
        assert_eq!(buf, [1, 2, 3]);
        assert_eq!(r.finish(), Ok(()));
    }

    #[test]
    fn reader_reports_truncation_and_bad_bool() {
        assert_eq!(Reader::new(&[1]).u16(), Err(StateError::Truncated));
        assert_eq!(
            Reader::new(&[2]).bool(),
            Err(StateError::Invalid("booleano"))
        );
        assert_eq!(Reader::new(&[0]).finish(), Err(StateError::TrailingData));
    }

    #[test]
    fn rom_hash_is_stable_and_sensitive() {
        assert_eq!(rom_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_ne!(rom_hash(&[0, 1]), rom_hash(&[1, 0]));
    }
}
