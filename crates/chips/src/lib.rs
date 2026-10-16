//! Chips usados por mais de um console.
//!
//! - [`z80`]: CPU do Master System e CPU de som do Mega Drive.
//! - [`psg`]: SN76489, o som do Master System, embutido no VDP do Mega Drive.
//! - [`state`]: primitivas do formato binário dos estados salvos.

pub mod psg;
pub mod state;
pub mod z80;
