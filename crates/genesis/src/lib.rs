//! Núcleo do Mega Drive / Genesis em safe Rust.
//!
//! Em construção por etapas: a CPU 68000 ([`m68k`]) vem primeiro; o VDP, o
//! som e o barramento completo vêm nas próximas.

pub mod m68k;
