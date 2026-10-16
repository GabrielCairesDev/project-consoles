//! Controlador do CD-ROM (0x1F801800–0x1F801803) e o drive.
//!
//! A CPU escreve os parâmetros e o comando; o controlador responde depois
//! com interrupções numeradas, cada uma com bytes na fila de respostas:
//! INT3 confirma o comando, INT2 avisa que ele terminou, INT1 entrega um
//! setor lido, INT4 o fim do disco e INT5 um erro. Só uma interrupção fica
//! visível por vez: as seguintes esperam a CPU reconhecer a anterior.
//!
//! Um setor lido fica no buffer até a CPU pedir os dados (HCHPCTL bit 7);
//! então eles vão para a fila de dados, lida byte a byte ou pelo DMA 3.
//!
//! O som do disco (CD-DA no Play, XA-ADPCM na leitura) sai a 44,1 kHz,
//! já com os volumes ATV, na fila [`Cdrom::audio`], que o SPU consome.
//!
//! Os tempos são os medidos no hardware (programa `cdrom/timing`), em
//! média. Ainda não são modelados: a tampa aberta, a troca de disco, o
//! envio de XA-ADPCM pela CPU (sound map) e os comandos Forward/Backward.

mod audio;

use std::collections::VecDeque;
use std::sync::Arc;

use crate::CPU_CLOCK;
use crate::disc::{Disc, FIRST_SECTOR, SECTOR_SIZE, TrackKind};
use audio::{Volumes, XaDecoder};

/// Um setor na velocidade normal (75 por segundo).
const SECTOR_TICKS: u64 = CPU_CLOCK as u64 / 75;
const ACK_TICKS: u64 = 50_000;
/// Da confirmação ao fim de comandos rápidos (GetID, SetSession).
const SECOND_TICKS: u64 = 35_000;
const INIT_TICKS: u64 = 400_000;
const MOTOR_TICKS: u64 = 400_000;
const SEEK_TICKS: u64 = 300_000;
/// Parar a leitura leva cerca de um milhão de ciclos nas duas velocidades.
const PAUSE_READING_TICKS: u64 = 1_000_000;
const PAUSE_IDLE_TICKS: u64 = 7_000;
const READ_TOC_TICKS: u64 = CPU_CLOCK as u64 / 2;
/// Da confirmação de uma interrupção à entrega da próxima da fila.
const DELIVER_TICKS: u64 = 1_000;

const STAT_ERROR: u8 = 0x01;
const STAT_MOTOR: u8 = 0x02;
const STAT_SEEK_ERROR: u8 = 0x04;
const STAT_READING: u8 = 0x20;
const STAT_SEEKING: u8 = 0x40;
const STAT_PLAYING: u8 = 0x80;

const MODE_AUTOPAUSE: u8 = 0x02;
const MODE_REPORT: u8 = 0x04;
const MODE_XA_FILTER: u8 = 0x08;
const MODE_XA_ADPCM: u8 = 0x40;
const MODE_WHOLE_SECTOR: u8 = 0x20;
const MODE_DOUBLE_SPEED: u8 = 0x80;

/// Som guardado sem o SPU consumir (um segundo); o mais antigo é
/// descartado.
const MAX_AUDIO: usize = 44_100;

/// Códigos de erro (segundo byte do INT5).
const ERROR_PARAM_VALUE: u8 = 0x10;
const ERROR_PARAM_COUNT: u8 = 0x20;
const ERROR_COMMAND: u8 = 0x40;
const ERROR_NOT_READY: u8 = 0x80;

const INT_DATA: u8 = 1;
const INT_COMPLETE: u8 = 2;
const INT_ACK: u8 = 3;
const INT_END: u8 = 4;
const INT_ERROR: u8 = 5;

/// Bytes por setor na fila de dados: só os dados do usuário ou o setor
/// inteiro menos a sincronização.
const DATA_SIZE: usize = 0x800;
const WHOLE_SIZE: usize = 0x924;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Response {
    int: u8,
    bytes: [u8; 16],
    len: u8,
}

impl Response {
    fn new(int: u8, bytes: &[u8]) -> Self {
        let mut response = Self {
            int,
            len: bytes.len() as u8,
            ..Self::default()
        };
        response.bytes[..bytes.len()].copy_from_slice(bytes);
        response
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterSeek {
    Read,
    Play,
    /// SeekL/SeekP: avisa com INT2.
    Report,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drive {
    /// Motor parado.
    Stopped,
    /// Motor girando, sem ler.
    Idle,
    Seeking {
        until: u64,
        then: AfterSeek,
    },
    Reading {
        next: u64,
    },
    Playing {
        next: u64,
    },
}

/// Segunda resposta pendente; o status é calculado na hora da entrega.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Second {
    Stat,
    Id,
}

#[derive(Clone)]
pub(crate) struct Cdrom {
    index: u8,
    params: VecDeque<u8>,
    response: Response,
    response_pos: u8,
    irq_enable: u8,
    irq_flag: u8,
    /// Nível da linha de interrupção e se ela acabou de subir.
    irq_line: bool,
    irq_rising: bool,
    /// Confirmação do comando atual: até ela chegar, o controlador está
    /// ocupado.
    ack: Option<(u64, Response)>,
    second: Option<(u64, Second)>,
    /// Respostas prontas esperando a CPU reconhecer a anterior.
    ready: VecDeque<Response>,
    deliver_at: Option<u64>,
    disc: Option<Arc<Disc>>,
    drive: Drive,
    mode: u8,
    filter: (u8, u8),
    /// Posição pedida pelo Setloc, usada pelo próximo comando que lê.
    target: Option<u32>,
    /// Setor sob a cabeça (o último lido ou procurado).
    position: u32,
    /// Cabeçalho e subcabeçalho do último setor de dados lido (GetlocL).
    header: Option<[u8; 8]>,
    sector: Box<[u8; SECTOR_SIZE]>,
    /// Setor no buffer, à espera do pedido de dados.
    sector_ready: bool,
    data: Box<[u8; WHOLE_SIZE]>,
    data_len: usize,
    data_pos: usize,
    /// Trilha tocando, para o AutoPause perceber a troca.
    play_track: Option<u8>,
    xa: XaDecoder,
    volumes: Volumes,
    /// Mute (todo o som) e ADPMUTE (só o XA-ADPCM).
    muted: bool,
    xa_muted: bool,
    /// Som a 44,1 kHz para o SPU, pela ordem.
    pub(crate) audio: VecDeque<[i16; 2]>,
}

impl Default for Cdrom {
    fn default() -> Self {
        Self {
            index: 0,
            params: VecDeque::with_capacity(16),
            response: Response::default(),
            response_pos: 0,
            irq_enable: 0,
            irq_flag: 0,
            irq_line: false,
            irq_rising: false,
            ack: None,
            second: None,
            ready: VecDeque::with_capacity(4),
            deliver_at: None,
            disc: None,
            drive: Drive::Stopped,
            mode: 0,
            filter: (0, 0),
            target: None,
            position: FIRST_SECTOR,
            header: None,
            sector: Box::new([0; SECTOR_SIZE]),
            sector_ready: false,
            data: Box::new([0; WHOLE_SIZE]),
            data_len: 0,
            data_pos: 0,
            play_track: None,
            xa: XaDecoder::default(),
            volumes: Volumes::default(),
            muted: false,
            xa_muted: false,
            audio: VecDeque::with_capacity(MAX_AUDIO),
        }
    }
}

/// Põe uma amostra do disco na fila do SPU, com os volumes ATV.
fn push_audio(audio: &mut VecDeque<[i16; 2]>, volumes: &Volumes, silent: bool, sample: [i16; 2]) {
    if audio.len() >= MAX_AUDIO {
        audio.pop_front();
    }
    audio.push_back(if silent { [0, 0] } else { volumes.mix(sample) });
}

fn bcd(value: u32) -> u8 {
    (value / 10 * 16 + value % 10) as u8
}

fn from_bcd(value: u8) -> Option<u32> {
    let (high, low) = (value >> 4, value & 0xF);
    (high < 10 && low < 10).then_some(u32::from(high) * 10 + u32::from(low))
}

/// Setor absoluto em minutos, segundos e frames (BCD).
fn msf(sector: u32) -> [u8; 3] {
    [
        bcd(sector / 75 / 60),
        bcd(sector / 75 % 60),
        bcd(sector % 75),
    ]
}

impl Cdrom {
    pub(crate) fn insert_disc(&mut self, disc: Disc) {
        self.disc = Some(Arc::new(disc));
        // Com a tampa fechada, o drive gira o disco sozinho.
        self.drive = Drive::Idle;
        self.header = None;
    }

    // --- Registradores ---

    pub(crate) fn read(&mut self, offset: u32) -> u8 {
        match offset {
            0 => self.status(),
            1 => {
                let byte = self.response.bytes[usize::from(self.response_pos & 15)];
                self.response_pos = self.response_pos.wrapping_add(1);
                byte
            }
            2 => self.read_data(),
            _ if self.index & 1 == 0 => self.irq_enable | 0xE0,
            _ => self.irq_flag | 0xE0,
        }
    }

    /// HSTS: índice, filas de parâmetros (vazia, com espaço), respostas e
    /// dados não vazias, e ocupado.
    fn status(&self) -> u8 {
        self.index
            | u8::from(self.params.is_empty()) << 3
            | u8::from(self.params.len() < 16) << 4
            | u8::from(self.response_pos < self.response.len) << 5
            | u8::from(self.data_pos < self.data_len) << 6
            | u8::from(self.ack.is_some()) << 7
    }

    fn read_data(&mut self) -> u8 {
        let Some(&byte) = self.data[..self.data_len].get(self.data_pos) else {
            return 0;
        };
        self.data_pos += 1;
        byte
    }

    /// Palavra lida pelo DMA (canal 3).
    pub(crate) fn dma_read(&mut self) -> u32 {
        u32::from_le_bytes(std::array::from_fn(|_| self.read_data()))
    }

    pub(crate) fn write(&mut self, offset: u32, value: u8, now: u64) {
        match (offset, self.index) {
            (0, _) => self.index = value & 3,
            (1, 0) => self.command(value, now),
            (2, 0) => {
                if self.params.len() < 16 {
                    self.params.push_back(value);
                }
            }
            (2, 1) => {
                self.irq_enable = value & 0x1F;
                self.update_irq();
            }
            (3, 0) => self.request(value),
            (3, 1) => self.acknowledge(value, now),
            (2, 2) => self.volumes.pending[0] = value,
            (3, 2) => self.volumes.pending[1] = value,
            (1, 3) => self.volumes.pending[2] = value,
            (2, 3) => self.volumes.pending[3] = value,
            (3, 3) => {
                self.xa_muted = value & 1 != 0;
                if value & 0x20 != 0 {
                    self.volumes.apply();
                }
            }
            // Envio de XA-ADPCM pela CPU (sound map): não modelado.
            _ => {}
        }
    }

    /// HCHPCTL: o bit 7 pede o setor do buffer na fila de dados; zerado,
    /// descarta a fila.
    fn request(&mut self, value: u8) {
        if value & 0x80 == 0 {
            self.data_len = 0;
            self.data_pos = 0;
            return;
        }
        if self.data_pos < self.data_len || !self.sector_ready {
            return;
        }
        self.sector_ready = false;
        let (start, len) = if self.mode & MODE_WHOLE_SECTOR != 0 {
            (12, WHOLE_SIZE)
        } else {
            (24, DATA_SIZE)
        };
        self.data[..len].copy_from_slice(&self.sector[start..start + len]);
        self.data_len = len;
        self.data_pos = 0;
    }

    /// HCLRCTL: reconhece as interrupções (bits 0–4) e limpa a fila de
    /// parâmetros (bit 6).
    fn acknowledge(&mut self, value: u8, now: u64) {
        self.irq_flag &= !(value & 0x1F);
        if value & 0x40 != 0 {
            self.params.clear();
        }
        if self.irq_flag == 0 && !self.ready.is_empty() && self.deliver_at.is_none() {
            self.deliver_at = Some(now + DELIVER_TICKS);
        }
        self.update_irq();
    }

    fn update_irq(&mut self) {
        let line = self.irq_flag & self.irq_enable & 0x1F != 0;
        self.irq_rising |= line && !self.irq_line;
        self.irq_line = line;
    }

    /// Se a linha de interrupção subiu desde a última consulta.
    pub(crate) fn take_irq(&mut self) -> bool {
        std::mem::take(&mut self.irq_rising)
    }

    // --- Respostas ---

    fn stat(&self) -> u8 {
        match self.drive {
            Drive::Stopped => 0,
            Drive::Idle => STAT_MOTOR,
            Drive::Seeking { .. } => STAT_MOTOR | STAT_SEEKING,
            Drive::Reading { .. } => STAT_MOTOR | STAT_READING,
            Drive::Playing { .. } => STAT_MOTOR | STAT_PLAYING,
        }
    }

    fn error(&self, code: u8) -> Response {
        Response::new(INT_ERROR, &[self.stat() | STAT_ERROR, code])
    }

    fn stat_response(&self, int: u8) -> Response {
        Response::new(int, &[self.stat()])
    }

    /// Põe a resposta na fila; aparece já se não houver outra pendente.
    fn push(&mut self, response: Response) {
        self.ready.push_back(response);
        if self.irq_flag == 0 && self.deliver_at.is_none() {
            self.deliver();
        }
    }

    fn deliver(&mut self) {
        if let Some(response) = self.ready.pop_front() {
            self.irq_flag = response.int;
            self.response = response;
            self.response_pos = 0;
            self.update_irq();
        }
    }

    // --- Comandos ---

    fn command(&mut self, command: u8, now: u64) {
        let params: Vec<u8> = self.params.drain(..).collect();
        let response = self.execute(command, &params, now);
        self.ack = Some((now + ACK_TICKS, response));
    }

    /// Quantos parâmetros cada comando aceita.
    fn param_count(command: u8) -> Option<std::ops::RangeInclusive<usize>> {
        Some(match command {
            0x02 => 3..=3,
            0x03 => 0..=1,
            0x0D => 2..=2,
            0x0E | 0x12 | 0x14 => 1..=1,
            0x19 => 1..=16,
            0x01 | 0x04..=0x0C | 0x0F..=0x11 | 0x13 | 0x15..=0x18 | 0x1A..=0x1F => 0..=0,
            _ => return None,
        })
    }

    /// Executa o comando e devolve a confirmação (INT3) ou o erro (INT5).
    fn execute(&mut self, command: u8, params: &[u8], now: u64) -> Response {
        let Some(count) = Self::param_count(command) else {
            return self.error(ERROR_COMMAND);
        };
        if !count.contains(&params.len()) {
            return self.error(ERROR_PARAM_COUNT);
        }
        let needs_disc = matches!(command, 0x03..=0x07 | 0x15 | 0x16 | 0x1B | 0x1E);
        if needs_disc && self.disc.is_none() {
            return self.error(ERROR_NOT_READY);
        }
        let ack = self.stat_response(INT_ACK);
        match command {
            0x01 => ack,
            0x02 => match params
                .iter()
                .map(|&p| from_bcd(p))
                .collect::<Option<Vec<_>>>()
            {
                Some(t) if t[1] < 60 && t[2] < 75 => {
                    self.target = Some((t[0] * 60 + t[1]) * 75 + t[2]);
                    ack
                }
                _ => self.error(ERROR_PARAM_VALUE),
            },
            0x03 => {
                if let Some(track) = params.first() {
                    let start = from_bcd(*track).and_then(|n| {
                        let disc = self.disc.as_ref()?;
                        disc.tracks().iter().find(|t| u32::from(t.number) == n)
                    });
                    if let Some(track) = start {
                        self.target = Some(track.start);
                    }
                }
                self.start(AfterSeek::Play, now);
                ack
            }
            0x06 | 0x1B => {
                self.xa = XaDecoder::default();
                self.start(AfterSeek::Read, now);
                ack
            }
            0x07 => {
                self.drive = Drive::Idle;
                self.finish_later(now + MOTOR_TICKS, Second::Stat);
                ack
            }
            0x08 => {
                let ticks = if self.drive == Drive::Stopped {
                    PAUSE_IDLE_TICKS
                } else {
                    PAUSE_READING_TICKS
                };
                self.drive = Drive::Stopped;
                self.finish_later(now + ticks, Second::Stat);
                ack
            }
            0x09 => {
                let ticks = match self.drive {
                    Drive::Reading { .. } | Drive::Playing { .. } => PAUSE_READING_TICKS,
                    _ => PAUSE_IDLE_TICKS,
                };
                if self.drive != Drive::Stopped {
                    self.drive = Drive::Idle;
                }
                self.finish_later(now + ticks, Second::Stat);
                ack
            }
            0x0A => {
                self.mode = MODE_WHOLE_SECTOR;
                self.drive = Drive::Idle;
                self.ready.clear();
                self.finish_later(now + INIT_TICKS, Second::Stat);
                ack
            }
            0x0B | 0x0C => {
                self.muted = command == 0x0B;
                ack
            }
            0x0D => {
                self.filter = (params[0], params[1]);
                ack
            }
            0x0E => {
                self.mode = params[0];
                ack
            }
            0x0F => Response::new(
                INT_ACK,
                &[self.stat(), self.mode, 0, self.filter.0, self.filter.1],
            ),
            0x10 => match self.header {
                Some(header) => Response::new(INT_ACK, &header),
                None => self.error(ERROR_NOT_READY),
            },
            0x11 => self.location(),
            0x12 if params[0] == 1 && self.disc.is_some() => {
                self.finish_later(now + SECOND_TICKS, Second::Stat);
                ack
            }
            0x12 => self.error(ERROR_PARAM_VALUE),
            0x13 => match self.disc.as_ref().map(|d| d.tracks()) {
                Some([first, .., last] | [first @ last]) => Response::new(
                    INT_ACK,
                    &[
                        self.stat(),
                        bcd(first.number.into()),
                        bcd(last.number.into()),
                    ],
                ),
                _ => self.error(ERROR_NOT_READY),
            },
            0x14 => self.track_start(params[0]),
            0x15 | 0x16 => {
                self.start(AfterSeek::Report, now);
                ack
            }
            0x19 => self.test(params[0]),
            0x1A => {
                self.finish_later(now + SECOND_TICKS, Second::Id);
                ack
            }
            0x1E => {
                self.drive = Drive::Idle;
                self.finish_later(now + READ_TOC_TICKS, Second::Stat);
                ack
            }
            _ => self.error(ERROR_COMMAND),
        }
    }

    fn finish_later(&mut self, at: u64, second: Second) {
        self.second = Some((at + ACK_TICKS, second));
    }

    /// Começa a ler ou tocar, procurando antes a posição do Setloc.
    fn start(&mut self, then: AfterSeek, now: u64) {
        self.second = None;
        self.ready.retain(|r| r.int != INT_DATA);
        match self.target.take() {
            Some(target) => {
                self.position = target;
                self.header = None;
                self.drive = Drive::Seeking {
                    until: now + SEEK_TICKS,
                    then,
                };
            }
            None => self.after_seek(then, now),
        }
    }

    fn sector_ticks(&self) -> u64 {
        if self.mode & MODE_DOUBLE_SPEED != 0 {
            SECTOR_TICKS / 2
        } else {
            SECTOR_TICKS
        }
    }

    fn after_seek(&mut self, then: AfterSeek, at: u64) {
        let end = self.disc.as_ref().map_or(0, |d| d.end());
        if self.position >= end {
            // Fora do disco: o motor para e o comando falha.
            self.drive = Drive::Stopped;
            self.push(Response::new(
                INT_ERROR,
                &[STAT_SEEK_ERROR, STAT_SEEK_ERROR],
            ));
            return;
        }
        let next = at + self.sector_ticks();
        match then {
            AfterSeek::Read => self.drive = Drive::Reading { next },
            AfterSeek::Play => {
                self.play_track = self.track_number(self.position);
                self.drive = Drive::Playing { next };
            }
            // A cabeça já leu o cabeçalho do setor procurado.
            AfterSeek::Report => {
                if self.load_sector() {
                    self.drive = Drive::Idle;
                    self.push(self.stat_response(INT_COMPLETE));
                }
            }
        }
    }

    /// Lê o setor da posição atual para o buffer e guarda o cabeçalho.
    /// Em erro de leitura do arquivo, para o drive, avisa com INT5 e
    /// devolve `false`.
    fn load_sector(&mut self) -> bool {
        let Some(disc) = self.disc.clone() else {
            return false;
        };
        if disc.read(self.position, &mut self.sector).is_err() {
            self.drive = Drive::Stopped;
            self.push(self.error(ERROR_NOT_READY));
            return false;
        }
        let data = disc
            .track_at(self.position)
            .is_some_and(|t| t.kind == TrackKind::Data);
        self.header = data.then(|| std::array::from_fn(|i| self.sector[12 + i]));
        true
    }

    /// Lê o próximo setor e o entrega com INT1.
    fn read_next(&mut self, at: u64) {
        let end = self.disc.as_ref().map_or(0, |d| d.end());
        if self.position >= end {
            self.drive = Drive::Idle;
            self.push(self.stat_response(INT_END));
            return;
        }
        if !self.load_sector() {
            return;
        }
        self.position += 1;
        self.drive = Drive::Reading {
            next: at + self.sector_ticks(),
        };
        if self.deliver_xa() {
            return;
        }
        self.sector_ready = true;
        // Se a CPU ainda não reconheceu o setor anterior, ele é perdido,
        // como no hardware.
        if !self.ready.iter().any(|r| r.int == INT_DATA) {
            self.push(self.stat_response(INT_DATA));
        }
    }

    /// Setores de som em tempo real (submodo áudio + tempo real) vão
    /// para o decodificador XA, se ligado e se passarem no filtro; com o
    /// filtro ligado, os que não passam são descartados. Devolve se o setor
    /// não vai para a CPU.
    fn deliver_xa(&mut self) -> bool {
        let sector = &self.sector;
        let realtime_audio = sector[15] == 2 && sector[18] & 0x44 == 0x44;
        if !realtime_audio {
            return false;
        }
        let filtering = self.mode & MODE_XA_FILTER != 0;
        if self.mode & MODE_XA_ADPCM != 0 && (!filtering || (sector[16], sector[17]) == self.filter)
        {
            let silent = self.muted || self.xa_muted;
            let (xa, volumes, audio) = (&mut self.xa, &self.volumes, &mut self.audio);
            xa.decode(sector, &mut |sample| {
                push_audio(audio, volumes, silent, sample);
            });
            return true;
        }
        filtering
    }

    fn track_number(&self, sector: u32) -> Option<u8> {
        self.disc.as_ref()?.track_at(sector).map(|t| t.number)
    }

    /// Toca o próximo setor (trilhas de dados saem mudas) e manda, se
    /// pedido, o relatório de posição.
    fn play_next(&mut self, at: u64) {
        let end = self.disc.as_ref().map_or(0, |d| d.end());
        let track = self.track_number(self.position);
        // Fim do disco: o motor para. Fim da trilha com AutoPause: pausa
        // no fim dela; sem AutoPause, segue para a próxima.
        if self.position >= end {
            self.drive = Drive::Stopped;
            self.push(self.stat_response(INT_END));
            return;
        }
        if track != self.play_track {
            if self.mode & MODE_AUTOPAUSE != 0 {
                self.drive = Drive::Idle;
                self.push(self.stat_response(INT_END));
                return;
            }
            self.play_track = track;
        }
        if !self.load_sector() {
            return;
        }
        let audio = self.header.is_none();
        let mut peak = 0u16;
        for frame in self.sector.as_chunks::<4>().0 {
            let sample = if audio {
                [
                    i16::from_le_bytes([frame[0], frame[1]]),
                    i16::from_le_bytes([frame[2], frame[3]]),
                ]
            } else {
                [0, 0]
            };
            peak = peak.max(sample[0].unsigned_abs().max(sample[1].unsigned_abs()));
            push_audio(&mut self.audio, &self.volumes, self.muted, sample);
        }
        let played = self.position;
        self.position += 1;
        self.drive = Drive::Playing {
            next: at + self.sector_ticks(),
        };
        if self.mode & MODE_REPORT != 0 && (played % 75).is_multiple_of(10) {
            self.report(played, peak.min(0x7FFF));
        }
    }

    /// Relatório do Play: posição absoluta nos frames 0, 20, 40 e 60, e
    /// relativa à trilha (segundos com o bit 7) nos 10, 30, 50 e 70.
    fn report(&mut self, sector: u32, peak: u16) {
        let Some((track, index, relative)) = self.subq(sector) else {
            return;
        };
        let [m, s, f] = if (sector % 75).is_multiple_of(20) {
            msf(sector)
        } else {
            let [m, s, f] = msf(relative);
            [m, s | 0x80, f]
        };
        let [low, high] = peak.to_le_bytes();
        if !self.ready.iter().any(|r| r.int == INT_DATA) {
            let response = [self.stat(), track, index, m, s, f, low, high];
            self.push(Response::new(INT_DATA, &response));
        }
    }

    /// Trilha, índice (BCD) e tempo dentro da trilha do subcanal Q.
    fn subq(&self, p: u32) -> Option<(u8, u8, u32)> {
        let disc = self.disc.as_ref()?;
        Some(match disc.track_at(p) {
            Some(t) if p >= t.start => (bcd(t.number.into()), 1, p - t.start),
            Some(t) => (bcd(t.number.into()), 0, t.start - p),
            None if p >= disc.end() => (0xAA, 1, p - disc.end()),
            // Lead-in: antes da pausa da trilha 1.
            None => (1, 0, FIRST_SECTOR - p),
        })
    }

    /// GetlocP: trilha, índice e posição na trilha e no disco.
    fn location(&self) -> Response {
        let p = self.position;
        let Some((track, index, relative)) = self.subq(p) else {
            return self.error(ERROR_NOT_READY);
        };
        let [m, s, f] = msf(relative);
        let [am, as_, af] = msf(p);
        Response::new(INT_ACK, &[track, index, m, s, f, am, as_, af])
    }

    /// GetTD: começo da trilha (0 = fim do disco), em minutos e segundos.
    fn track_start(&self, track: u8) -> Response {
        let Some(disc) = &self.disc else {
            return self.error(ERROR_NOT_READY);
        };
        let start = match from_bcd(track) {
            Some(0) => Some(disc.end()),
            Some(n) => disc
                .tracks()
                .iter()
                .find(|t| u32::from(t.number) == n)
                .map(|t| t.start),
            None => None,
        };
        match start {
            Some(start) => {
                let [m, s, _] = msf(start);
                Response::new(INT_ACK, &[self.stat(), m, s])
            }
            None => self.error(ERROR_PARAM_VALUE),
        }
    }

    /// Test: só a versão do controlador (a do SCPH-1001).
    fn test(&self, sub: u8) -> Response {
        match sub {
            0x20 => Response::new(INT_ACK, &[0x94, 0x09, 0x19, 0xC0]),
            _ => self.error(ERROR_PARAM_VALUE),
        }
    }

    /// GetID: disco licenciado responde "SCE" e a região.
    fn id(&self) -> Response {
        let Some(disc) = &self.disc else {
            return Response::new(INT_ERROR, &[0x08, 0x40, 0, 0, 0, 0, 0, 0]);
        };
        let audio = disc.tracks()[0].kind == TrackKind::Audio;
        match disc.region() {
            Some(region) if !audio => Response::new(
                INT_COMPLETE,
                &[self.stat(), 0x00, 0x20, 0x00, b'S', b'C', b'E', region],
            ),
            _ => {
                let flags = if audio { 0x90 } else { 0x80 };
                Response::new(INT_ERROR, &[self.stat() | 0x08, flags, 0, 0, 0, 0, 0, 0])
            }
        }
    }

    // --- Tempo ---

    fn drive_event(&self) -> Option<u64> {
        match self.drive {
            Drive::Seeking { until, .. } => Some(until),
            Drive::Reading { next } | Drive::Playing { next } => Some(next),
            Drive::Stopped | Drive::Idle => None,
        }
    }

    /// Próximo ciclo em que algo muda no controlador.
    pub(crate) fn next_event(&self) -> Option<u64> {
        [
            self.ack.map(|(at, _)| at),
            self.second.map(|(at, _)| at),
            self.deliver_at,
            self.drive_event(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Processa, em ordem, os eventos até `now`.
    pub(crate) fn sync(&mut self, now: u64) {
        while let Some(at) = self.next_event().filter(|&at| at <= now) {
            if self.ack.is_some_and(|(t, _)| t == at) {
                let (_, response) = self.ack.take().expect("confirmação pendente");
                self.push(response);
            } else if self.deliver_at == Some(at) {
                self.deliver_at = None;
                if self.irq_flag == 0 {
                    self.deliver();
                }
            } else if self.second.is_some_and(|(t, _)| t == at) {
                let (_, second) = self.second.take().expect("resposta pendente");
                let response = match second {
                    Second::Stat => self.stat_response(INT_COMPLETE),
                    Second::Id => self.id(),
                };
                self.push(response);
            } else {
                match self.drive {
                    Drive::Seeking { then, .. } => self.after_seek(then, at),
                    Drive::Reading { .. } => self.read_next(at),
                    Drive::Playing { .. } => self.play_next(at),
                    Drive::Stopped | Drive::Idle => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disc::tests::{audio_disc, data_disc, disc_with};

    /// Controlador com as interrupções ligadas e, se pedido, um disco.
    fn cdrom(disc: Option<Disc>) -> Cdrom {
        let mut cd = Cdrom::default();
        if let Some(disc) = disc {
            cd.insert_disc(disc);
        }
        cd.write(0, 1, 0);
        cd.write(2, 0x1F, 0);
        cd.write(0, 0, 0);
        cd
    }

    /// Roda até a próxima interrupção; devolve o tipo, os bytes e o ciclo.
    fn next_irq(cd: &mut Cdrom, now: &mut u64) -> (u8, Vec<u8>) {
        loop {
            let at = cd.next_event().expect("há um evento pendente");
            *now = at;
            cd.sync(at);
            if cd.take_irq() {
                break;
            }
        }
        let response = (0..cd.response.len).map(|_| cd.read(1)).collect();
        let int = cd.irq_flag;
        cd.write(0, 1, *now);
        cd.write(3, 0x1F, *now);
        cd.write(0, 0, *now);
        (int, response)
    }

    fn send(cd: &mut Cdrom, now: u64, command: u8, params: &[u8]) {
        for &p in params {
            cd.write(2, p, now);
        }
        cd.write(1, command, now);
    }

    #[test]
    fn getstat_is_acknowledged_after_a_delay() {
        let mut cd = cdrom(None);
        send(&mut cd, 100, 0x01, &[]);
        assert_ne!(cd.status() & 0x80, 0, "ocupado");
        cd.sync(100 + ACK_TICKS - 1);
        assert!(!cd.take_irq());
        cd.sync(100 + ACK_TICKS);
        assert!(cd.take_irq());
        assert_eq!(cd.status() & 0xA0, 0x20, "resposta pronta, livre");
        assert_eq!(cd.read(1), 0);
        assert_eq!(cd.status() & 0x20, 0, "fila de respostas lida");
        assert_eq!(cd.read(3), 0xFF, "máscara no índice 0");
        cd.write(0, 1, 0);
        assert_eq!(cd.read(3), 0xE0 | INT_ACK, "flag no índice 1");
    }

    #[test]
    fn errors_for_unknown_commands_and_wrong_params() {
        let mut cd = cdrom(None);
        let mut now = 0;
        send(&mut cd, now, 0x50, &[]);
        assert_eq!(next_irq(&mut cd, &mut now), (INT_ERROR, vec![0x01, 0x40]));
        send(&mut cd, now, 0x0E, &[]);
        assert_eq!(next_irq(&mut cd, &mut now), (INT_ERROR, vec![0x01, 0x20]));
        send(&mut cd, now, 0x02, &[0x00, 0x7A, 0x00]);
        assert_eq!(next_irq(&mut cd, &mut now), (INT_ERROR, vec![0x01, 0x10]));
        send(&mut cd, now, 0x06, &[]);
        assert_eq!(next_irq(&mut cd, &mut now), (INT_ERROR, vec![0x01, 0x80]));
        assert_eq!(cd.status() & 0x08, 0x08, "parâmetros consumidos");
    }

    #[test]
    fn second_response_waits_for_the_first_to_be_acknowledged() {
        let mut cd = cdrom(Some(data_disc(8)));
        send(&mut cd, 0, 0x1A, &[]);
        let mut now = ACK_TICKS;
        cd.sync(now);
        assert!(cd.take_irq());
        // Sem reconhecer, a segunda resposta fica na fila.
        now += 10 * SECOND_TICKS;
        cd.sync(now);
        assert!(!cd.take_irq());
        assert_eq!(cd.irq_flag, INT_ACK);
        cd.write(0, 1, now);
        cd.write(3, 0x1F, now);
        cd.sync(now + DELIVER_TICKS);
        assert!(cd.take_irq());
        assert_eq!(cd.irq_flag, INT_COMPLETE);
        let id: Vec<u8> = (0..8).map(|_| cd.read(1)).collect();
        assert_eq!(id, [STAT_MOTOR, 0x00, 0x20, 0x00, b'S', b'C', b'E', b'A']);
    }

    #[test]
    fn get_id_without_disc_or_license() {
        let mut cd = cdrom(None);
        let mut now = 0;
        send(&mut cd, now, 0x1A, &[]);
        assert_eq!(next_irq(&mut cd, &mut now).0, INT_ACK);
        assert_eq!(
            next_irq(&mut cd, &mut now),
            (INT_ERROR, vec![0x08, 0x40, 0, 0, 0, 0, 0, 0])
        );
        let mut cd = cdrom(Some(data_disc(3)));
        send(&mut cd, now, 0x1A, &[]);
        next_irq(&mut cd, &mut now);
        assert_eq!(next_irq(&mut cd, &mut now).1[..2], [0x0A, 0x80]);
    }

    #[test]
    fn reads_sectors_into_the_data_fifo() {
        let mut cd = cdrom(Some(data_disc(20)));
        let mut now = 0;
        send(&mut cd, now, 0x0E, &[0x80]);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x02, &[0x00, 0x02, 0x05]);
        next_irq(&mut cd, &mut now);
        let start = now;
        send(&mut cd, now, 0x06, &[]);
        assert_eq!(next_irq(&mut cd, &mut now), (INT_ACK, vec![STAT_MOTOR]));
        for marker in 5..8 {
            assert_eq!(
                next_irq(&mut cd, &mut now),
                (INT_DATA, vec![STAT_MOTOR | STAT_READING])
            );
            cd.write(3, 0x80, now);
            assert_ne!(cd.status() & 0x40, 0, "dados na fila");
            assert_eq!(cd.dma_read() & 0xFF, marker);
            for _ in 1..DATA_SIZE / 4 {
                cd.dma_read();
            }
            assert_eq!(cd.status() & 0x40, 0, "fila esvaziada");
            cd.write(3, 0x00, now);
        }
        // Velocidade dupla: 150 setores por segundo, depois da busca.
        let elapsed = now - start;
        assert!(elapsed >= SEEK_TICKS + 3 * SECTOR_TICKS / 2);
        assert!(elapsed < SEEK_TICKS + 4 * SECTOR_TICKS / 2);

        send(&mut cd, now, 0x10, &[]);
        let (int, header) = next_irq(&mut cd, &mut now);
        assert_eq!(int, INT_ACK);
        assert_eq!(
            header[..4],
            [0x00, 0x02, 0x07, 0x02],
            "último lido: 00:02:07"
        );

        send(&mut cd, now, 0x09, &[]);
        // Um setor pode chegar antes da confirmação da pausa.
        let mut int = next_irq(&mut cd, &mut now).0;
        if int == INT_DATA {
            int = next_irq(&mut cd, &mut now).0;
        }
        assert_eq!(int, INT_ACK);
        assert_eq!(
            next_irq(&mut cd, &mut now),
            (INT_COMPLETE, vec![STAT_MOTOR])
        );
        assert_eq!(cd.next_event(), None, "parado");
    }

    #[test]
    fn whole_sector_mode_includes_header() {
        let mut cd = cdrom(Some(data_disc(4)));
        let mut now = 0;
        send(&mut cd, now, 0x0E, &[MODE_WHOLE_SECTOR]);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x06, &[]);
        next_irq(&mut cd, &mut now);
        assert_eq!(next_irq(&mut cd, &mut now).0, INT_DATA);
        cd.write(3, 0x80, now);
        let bytes: Vec<u8> = (0..WHOLE_SIZE).map(|_| cd.read(2)).collect();
        assert_eq!(bytes[..4], [0x00, 0x02, 0x00, 0x02]);
        assert_eq!(bytes[12], 0, "primeiro setor");
        assert_eq!(bytes[WHOLE_SIZE - 1], 0);
        assert_eq!(cd.read(2), 0, "além do fim");
    }

    #[test]
    fn reading_past_the_end_reports_int4() {
        let mut cd = cdrom(Some(data_disc(2)));
        let mut now = 0;
        send(&mut cd, now, 0x06, &[]);
        next_irq(&mut cd, &mut now);
        assert_eq!(next_irq(&mut cd, &mut now).0, INT_DATA);
        assert_eq!(next_irq(&mut cd, &mut now).0, INT_DATA);
        assert_eq!(next_irq(&mut cd, &mut now), (INT_END, vec![STAT_MOTOR]));
    }

    #[test]
    fn seek_and_locations() {
        let mut cd = cdrom(Some(data_disc(20)));
        let mut now = 0;
        send(&mut cd, now, 0x02, &[0x00, 0x02, 0x10]);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x15, &[]);
        next_irq(&mut cd, &mut now);
        assert_eq!(
            next_irq(&mut cd, &mut now),
            (INT_COMPLETE, vec![STAT_MOTOR])
        );
        send(&mut cd, now, 0x11, &[]);
        assert_eq!(
            next_irq(&mut cd, &mut now).1,
            [0x01, 0x01, 0x00, 0x00, 0x10, 0x00, 0x02, 0x10]
        );
        send(&mut cd, now, 0x10, &[]);
        assert_eq!(next_irq(&mut cd, &mut now).1[..4], [0x00, 0x02, 0x10, 0x02]);

        send(&mut cd, now, 0x13, &[]);
        assert_eq!(next_irq(&mut cd, &mut now).1, [STAT_MOTOR, 0x01, 0x01]);
        send(&mut cd, now, 0x14, &[0x00]);
        assert_eq!(next_irq(&mut cd, &mut now).1, [STAT_MOTOR, 0x00, 0x02]);
        send(&mut cd, now, 0x14, &[0x02]);
        assert_eq!(next_irq(&mut cd, &mut now).0, INT_ERROR);

        // Além do fim do disco: erro de busca e o motor para.
        send(&mut cd, now, 0x02, &[0x74, 0x30, 0x00]);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x15, &[]);
        next_irq(&mut cd, &mut now);
        assert_eq!(
            next_irq(&mut cd, &mut now),
            (INT_ERROR, vec![STAT_SEEK_ERROR, STAT_SEEK_ERROR])
        );
        send(&mut cd, now, 0x01, &[]);
        assert_eq!(next_irq(&mut cd, &mut now).1, [0x00]);
    }

    #[test]
    fn init_resets_mode_and_test_reports_version() {
        let mut cd = cdrom(None);
        let mut now = 0;
        send(&mut cd, now, 0x0A, &[]);
        assert_eq!(next_irq(&mut cd, &mut now).0, INT_ACK);
        assert_eq!(
            next_irq(&mut cd, &mut now),
            (INT_COMPLETE, vec![STAT_MOTOR])
        );
        send(&mut cd, now, 0x0F, &[]);
        assert_eq!(next_irq(&mut cd, &mut now).1[..2], [STAT_MOTOR, 0x20]);
        send(&mut cd, now, 0x19, &[0x20]);
        assert_eq!(next_irq(&mut cd, &mut now).1, [0x94, 0x09, 0x19, 0xC0]);
    }

    fn xa(file: u8, channel: u8) -> [u8; SECTOR_SIZE] {
        let mut sector = audio::tests::xa_sector(0x00, 0x00, 0x11);
        sector[16] = file;
        sector[17] = channel;
        sector
    }

    #[test]
    fn xa_sectors_go_to_the_spu_and_the_filter_selects_the_channel() {
        let data: [u8; SECTOR_SIZE] = crate::disc::tests::data_sector(FIRST_SECTOR + 2, 7)
            .try_into()
            .expect("setor");
        let disc = disc_with(&[xa(1, 0), xa(1, 1), data, xa(1, 0)]);
        let mut cd = cdrom(Some(disc));
        let mut now = 0;
        send(&mut cd, now, 0x0E, &[MODE_XA_ADPCM | MODE_XA_FILTER]);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x0D, &[1, 0]);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x1B, &[]);
        next_irq(&mut cd, &mut now);
        // Só o setor de dados chega à CPU; o do canal 1 é descartado.
        assert_eq!(next_irq(&mut cd, &mut now).0, INT_DATA);
        assert_eq!(cd.audio.len(), 4704, "um setor mono a 37,8 kHz");
        assert!(cd.audio.iter().any(|s| s[0] > 0x0E00 && s[0] == s[1]));
        assert_eq!(next_irq(&mut cd, &mut now).0, INT_END);
        assert_eq!(cd.audio.len(), 2 * 4704);

        // ADPMUTE: os setores continuam indo para o SPU, em silêncio.
        cd.audio.clear();
        cd.write(0, 3, now);
        cd.write(3, 0x01, now);
        cd.write(0, 0, now);
        cd.write(2, 0x00, now);
        cd.write(2, 0x02, now);
        cd.write(2, 0x00, now);
        cd.write(1, 0x02, now);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x1B, &[]);
        next_irq(&mut cd, &mut now);
        next_irq(&mut cd, &mut now);
        assert_eq!(cd.audio.len(), 4704);
        assert!(cd.audio.iter().all(|&s| s == [0, 0]));
    }

    /// Trilha de áudio com a mesma amostra em todos os setores.
    fn tone(sectors: usize, left: i16, right: i16) -> Vec<u8> {
        let frame = [left.to_le_bytes(), right.to_le_bytes()].concat();
        frame.repeat(sectors * SECTOR_SIZE / 4)
    }

    #[test]
    fn play_outputs_cd_audio_with_reports_and_autopause() {
        let disc = audio_disc(2, &[tone(20, 1000, -1000), tone(5, 1, 1)]);
        let mut cd = cdrom(Some(disc));
        let mut now = 0;
        send(&mut cd, now, 0x0E, &[MODE_REPORT | MODE_AUTOPAUSE]);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x03, &[0x02]);
        next_irq(&mut cd, &mut now);
        let playing = STAT_MOTOR | STAT_PLAYING;
        // A trilha 2 começa em 00:02:02; frame 10: tempo na trilha.
        assert_eq!(
            next_irq(&mut cd, &mut now),
            (
                INT_DATA,
                vec![playing, 0x02, 0x01, 0x00, 0x80, 0x08, 0xE8, 0x03]
            )
        );
        assert_eq!(cd.audio.front(), Some(&[1000, -1000]));
        // Frame 20: tempo absoluto.
        assert_eq!(
            next_irq(&mut cd, &mut now),
            (
                INT_DATA,
                vec![playing, 0x02, 0x01, 0x00, 0x02, 0x20, 0xE8, 0x03]
            )
        );
        assert_eq!(next_irq(&mut cd, &mut now), (INT_END, vec![STAT_MOTOR]));
        assert_eq!(cd.audio.len(), 20 * 588, "parou no fim da trilha");
        send(&mut cd, now, 0x11, &[]);
        // Parou no começo da trilha 3 (sem pausa).
        assert_eq!(next_irq(&mut cd, &mut now).1[..2], [0x03, 0x01]);
    }

    #[test]
    fn play_without_autopause_runs_to_the_end_and_volumes_apply() {
        let disc = audio_disc(2, &[tone(3, 1000, -1000), tone(2, 2000, 0)]);
        let mut cd = cdrom(Some(disc));
        let mut now = 0;
        // Esquerda e direita trocadas.
        cd.write(0, 2, now);
        cd.write(2, 0x00, now);
        cd.write(3, 0x80, now);
        cd.write(0, 3, now);
        cd.write(1, 0x00, now);
        cd.write(2, 0x80, now);
        cd.write(3, 0x20, now);
        cd.write(0, 0, now);
        send(&mut cd, now, 0x03, &[0x02]);
        next_irq(&mut cd, &mut now);
        assert_eq!(
            next_irq(&mut cd, &mut now),
            (INT_END, vec![0x00]),
            "motor parado"
        );
        let audio: Vec<_> = cd.audio.iter().copied().collect();
        assert_eq!(audio.len(), 5 * 588);
        assert_eq!(audio[0], [-1000, 1000]);
        assert_eq!(audio[3 * 588], [0, 2000], "seguiu para a trilha 3");

        // Mute: o disco toca, mas em silêncio.
        cd.audio.clear();
        send(&mut cd, now, 0x0B, &[]);
        next_irq(&mut cd, &mut now);
        send(&mut cd, now, 0x03, &[0x03]);
        next_irq(&mut cd, &mut now);
        next_irq(&mut cd, &mut now);
        assert_eq!(cd.audio.len(), 2 * 588);
        assert!(cd.audio.iter().all(|&s| s == [0, 0]));
    }

    #[test]
    fn interrupt_line_follows_the_enable_mask() {
        let mut cd = Cdrom::default();
        send(&mut cd, 0, 0x01, &[]);
        cd.sync(ACK_TICKS);
        assert!(!cd.take_irq(), "desabilitada");
        cd.write(0, 1, ACK_TICKS);
        cd.write(2, 0x1F, ACK_TICKS);
        assert!(cd.take_irq(), "sobe ao habilitar com flag pendente");
        assert!(!cd.take_irq());
    }
}
