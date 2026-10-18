//! GTE (Geometry Transformation Engine), o COP2: transformação de vértices
//! com perspectiva, iluminação e cor em ponto fixo.
//!
//! A aritmética segue o hardware: os acumuladores MAC1–3 têm 44 bits e o
//! estouro é conferido a cada soma; IR1–3, cores, SZ e SXY saturam com as
//! flags do registrador FLAG; a divisão da perspectiva usa o método UNR do
//! chip (tabela + duas iterações de Newton–Raphson), que dá resultados
//! diferentes de uma divisão exata.

/// Tabela UNR da divisão (o chip a tem em ROM).
const UNR: [u8; 257] = {
    let mut table = [0; 257];
    let mut i = 0;
    while i < 257 {
        let v = (0x40000 / (i as i32 + 0x100) + 1) / 2 - 0x101;
        table[i] = if v > 0 { v as u8 } else { 0 };
        i += 1;
    }
    table
};

// Bits do FLAG.
const MAC_POSITIVE: [u32; 4] = [1 << 16, 1 << 30, 1 << 29, 1 << 28];
const MAC_NEGATIVE: [u32; 4] = [1 << 15, 1 << 27, 1 << 26, 1 << 25];
const IR_SATURATED: [u32; 4] = [1 << 12, 1 << 24, 1 << 23, 1 << 22];
const COLOR_SATURATED: [u32; 3] = [1 << 21, 1 << 20, 1 << 19];
const SZ_SATURATED: u32 = 1 << 18;
const DIVIDE_OVERFLOW: u32 = 1 << 17;
const SX_SATURATED: u32 = 1 << 14;
const SY_SATURATED: u32 = 1 << 13;
/// Bits que entram no bit 31 (erro).
const ERROR_MASK: u32 = 0x7F87_E000;

type Matrix = [[i16; 3]; 3];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Translation {
    Tr,
    Bk,
    Fc,
    None,
}

/// Vértice projetado pelo RTPS/RTPT: a palavra SXY que o jogo vai ler e a
/// fração (16 bits) de x e y que o hardware descarta. Só a imagem usa
/// (geometria precisa); não faz parte do estado do GTE.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Projected {
    pub(crate) word: u32,
    pub(crate) fraction: (u16, u16),
}

#[derive(Clone, Default)]
pub(crate) struct Gte {
    // Registradores de dados.
    v: [[i16; 3]; 3],
    rgbc: [u8; 4],
    otz: u16,
    ir: [i16; 4],
    sxy: [[i16; 2]; 3],
    sz: [u16; 4],
    rgb: [[u8; 4]; 3],
    res1: u32,
    mac: [i32; 4],
    lzcs: u32,
    lzcr: u32,
    // Registradores de controle.
    rt: Matrix,
    tr: [i32; 3],
    llm: Matrix,
    bk: [i32; 3],
    lcm: Matrix,
    fc: [i32; 3],
    ofx: i32,
    ofy: i32,
    h: u16,
    dqa: i16,
    dqb: i32,
    zsf3: i16,
    zsf4: i16,
    flag: u32,
    /// Vértices projetados pelo último comando (até 3, no RTPT).
    projected: [Projected; 3],
    projected_len: usize,
}

fn pack(low: i16, high: i16) -> u32 {
    u32::from(low as u16) | (u32::from(high as u16) << 16)
}

fn low(value: u32) -> i16 {
    value as i16
}

fn high(value: u32) -> i16 {
    (value >> 16) as i16
}

/// Elemento `index` (0..9) de uma matriz guardada em 5 registradores.
fn matrix_read(m: &Matrix, reg: usize) -> u32 {
    let at = |n: usize| m[n / 3][n % 3];
    match reg {
        4 => at(8) as i32 as u32,
        _ => pack(at(reg * 2), at(reg * 2 + 1)),
    }
}

fn matrix_write(m: &mut Matrix, reg: usize, value: u32) {
    let mut set = |n: usize, v: i16| m[n / 3][n % 3] = v;
    if reg == 4 {
        set(8, low(value));
    } else {
        set(reg * 2, low(value));
        set(reg * 2 + 1, high(value));
    }
}

impl Gte {
    pub(crate) fn read_data(&self, r: usize) -> u32 {
        match r {
            0 | 2 | 4 => pack(self.v[r / 2][0], self.v[r / 2][1]),
            1 | 3 | 5 => self.v[r / 2][2] as i32 as u32,
            6 => u32::from_le_bytes(self.rgbc),
            7 => u32::from(self.otz),
            8..=11 => self.ir[r - 8] as i32 as u32,
            12..=14 => pack(self.sxy[r - 12][0], self.sxy[r - 12][1]),
            15 => pack(self.sxy[2][0], self.sxy[2][1]),
            16..=19 => u32::from(self.sz[r - 16]),
            20..=22 => u32::from_le_bytes(self.rgb[r - 20]),
            23 => self.res1,
            24..=27 => self.mac[r - 24] as u32,
            28 | 29 => {
                let c = |ir: i16| (i32::from(ir) >> 7).clamp(0, 0x1F) as u32;
                c(self.ir[1]) | (c(self.ir[2]) << 5) | (c(self.ir[3]) << 10)
            }
            30 => self.lzcs,
            _ => self.lzcr,
        }
    }

    pub(crate) fn write_data(&mut self, r: usize, value: u32) {
        match r {
            0 | 2 | 4 => {
                self.v[r / 2][0] = low(value);
                self.v[r / 2][1] = high(value);
            }
            1 | 3 | 5 => self.v[r / 2][2] = low(value),
            6 => self.rgbc = value.to_le_bytes(),
            7 => self.otz = value as u16,
            8..=11 => self.ir[r - 8] = low(value),
            12..=14 => self.sxy[r - 12] = [low(value), high(value)],
            15 => {
                self.sxy[0] = self.sxy[1];
                self.sxy[1] = self.sxy[2];
                self.sxy[2] = [low(value), high(value)];
            }
            16..=19 => self.sz[r - 16] = value as u16,
            20..=22 => self.rgb[r - 20] = value.to_le_bytes(),
            23 => self.res1 = value,
            24..=27 => self.mac[r - 24] = value as i32,
            28 => {
                self.ir[1] = ((value & 0x1F) * 0x80) as i16;
                self.ir[2] = (((value >> 5) & 0x1F) * 0x80) as i16;
                self.ir[3] = (((value >> 10) & 0x1F) * 0x80) as i16;
            }
            30 => {
                self.lzcs = value;
                let v = value as i32;
                self.lzcr = if v < 0 {
                    v.leading_ones()
                } else {
                    v.leading_zeros()
                };
            }
            // ORGB e LZCR só leem.
            _ => {}
        }
    }

    pub(crate) fn read_control(&self, r: usize) -> u32 {
        match r {
            0..=4 => matrix_read(&self.rt, r),
            5..=7 => self.tr[r - 5] as u32,
            8..=12 => matrix_read(&self.llm, r - 8),
            13..=15 => self.bk[r - 13] as u32,
            16..=20 => matrix_read(&self.lcm, r - 16),
            21..=23 => self.fc[r - 21] as u32,
            24 => self.ofx as u32,
            25 => self.ofy as u32,
            // H é sem sinal, mas a leitura estende o sinal (erro do chip).
            26 => self.h as i16 as i32 as u32,
            27 => self.dqa as i32 as u32,
            28 => self.dqb as u32,
            29 => self.zsf3 as i32 as u32,
            30 => self.zsf4 as i32 as u32,
            _ => self.flag,
        }
    }

    pub(crate) fn write_control(&mut self, r: usize, value: u32) {
        match r {
            0..=4 => matrix_write(&mut self.rt, r, value),
            5..=7 => self.tr[r - 5] = value as i32,
            8..=12 => matrix_write(&mut self.llm, r - 8, value),
            13..=15 => self.bk[r - 13] = value as i32,
            16..=20 => matrix_write(&mut self.lcm, r - 16, value),
            21..=23 => self.fc[r - 21] = value as i32,
            24 => self.ofx = value as i32,
            25 => self.ofy = value as i32,
            26 => self.h = value as u16,
            27 => self.dqa = low(value),
            28 => self.dqb = value as i32,
            29 => self.zsf3 = low(value),
            30 => self.zsf4 = low(value),
            _ => {
                self.flag = value & 0x7FFF_F000;
                self.update_error_bit();
            }
        }
    }

    fn update_error_bit(&mut self) {
        if self.flag & ERROR_MASK != 0 {
            self.flag |= 1 << 31;
        }
    }

    /// Executa um comando; devolve a duração em ciclos.
    /// Vértices projetados pelo último comando, com a fração.
    pub(crate) fn projected(&self) -> &[Projected] {
        &self.projected[..self.projected_len]
    }

    pub(crate) fn command(&mut self, instruction: u32) -> u64 {
        let sf = instruction & (1 << 19) != 0;
        let shift = if sf { 12 } else { 0 };
        let lm = instruction & (1 << 10) != 0;
        self.flag = 0;
        self.projected_len = 0;
        let cycles = match instruction & 0x3F {
            0x01 => {
                self.rtp(0, shift, lm, true);
                15
            }
            0x06 => {
                self.nclip();
                8
            }
            0x0C => {
                self.op(shift, lm);
                6
            }
            0x10 => {
                let c = self.rgbc;
                self.dpc(c, shift, lm);
                8
            }
            0x11 => {
                self.intpl(shift, lm);
                8
            }
            0x12 => {
                self.mvmva(instruction, shift, lm);
                8
            }
            0x13 => {
                self.ncd(0, shift, lm);
                19
            }
            0x14 => {
                self.cdp(shift, lm);
                13
            }
            0x16 => {
                for n in 0..3 {
                    self.ncd(n, shift, lm);
                }
                44
            }
            0x1B => {
                self.ncc(0, shift, lm);
                17
            }
            0x1C => {
                self.cc(shift, lm);
                11
            }
            0x1E => {
                self.nc(0, shift, lm);
                14
            }
            0x20 => {
                for n in 0..3 {
                    self.nc(n, shift, lm);
                }
                30
            }
            0x28 => {
                self.sqr(shift, lm);
                5
            }
            0x29 => {
                self.dcpl(shift, lm);
                8
            }
            0x2A => {
                for _ in 0..3 {
                    let c = self.rgb[0];
                    self.dpc(c, shift, lm);
                }
                17
            }
            0x2D => {
                self.avsz(i64::from(self.zsf3), 1);
                5
            }
            0x2E => {
                self.avsz(i64::from(self.zsf4), 0);
                6
            }
            0x30 => {
                for n in 0..3 {
                    self.rtp(n, shift, lm, n == 2);
                }
                23
            }
            0x3D => {
                self.gpf(shift, lm);
                5
            }
            0x3E => {
                self.gpl(shift, lm);
                5
            }
            0x3F => {
                for n in 0..3 {
                    self.ncc(n, shift, lm);
                }
                39
            }
            _ => 1,
        };
        self.update_error_bit();
        cycles
    }

    // --- Aritmética com flags ---

    /// Confere o estouro de 44 bits do MAC `i` (1–3).
    fn check_mac(&mut self, i: usize, value: i64) -> i64 {
        if value > 0x7FF_FFFF_FFFF {
            self.flag |= MAC_POSITIVE[i];
        } else if value < -0x800_0000_0000 {
            self.flag |= MAC_NEGATIVE[i];
        }
        value
    }

    /// Resultado intermediário: confere o estouro e fica com 44 bits.
    fn extend(&mut self, i: usize, value: i64) -> i64 {
        (self.check_mac(i, value) << 20) >> 20
    }

    fn set_mac(&mut self, i: usize, value: i64, shift: u32) {
        self.check_mac(i, value);
        self.mac[i] = (value >> shift) as i32;
    }

    fn set_ir(&mut self, i: usize, value: i32, lm: bool) {
        let min = if lm { 0 } else { -0x8000 };
        self.ir[i] = if value < min {
            self.flag |= IR_SATURATED[i];
            min as i16
        } else if value > 0x7FFF {
            self.flag |= IR_SATURATED[i];
            0x7FFF
        } else {
            value as i16
        };
    }

    fn set_mac_ir(&mut self, i: usize, value: i64, shift: u32, lm: bool) {
        self.set_mac(i, value, shift);
        self.set_ir(i, self.mac[i], lm);
    }

    fn check_mac0(&mut self, value: i64) -> i64 {
        if value > 0x7FFF_FFFF {
            self.flag |= MAC_POSITIVE[0];
        } else if value < -0x8000_0000 {
            self.flag |= MAC_NEGATIVE[0];
        }
        value
    }

    fn set_mac0(&mut self, value: i64) {
        self.check_mac0(value);
        self.mac[0] = value as i32;
    }

    fn set_ir0(&mut self, value: i64) {
        self.ir[0] = if value < 0 {
            self.flag |= IR_SATURATED[0];
            0
        } else if value > 0x1000 {
            self.flag |= IR_SATURATED[0];
            0x1000
        } else {
            value as i16
        };
    }

    fn push_sz(&mut self, value: i64) {
        let z = if value < 0 {
            self.flag |= SZ_SATURATED;
            0
        } else if value > 0xFFFF {
            self.flag |= SZ_SATURATED;
            0xFFFF
        } else {
            value as u16
        };
        self.sz = [self.sz[1], self.sz[2], self.sz[3], z];
    }

    fn push_sxy(&mut self, x: i64, y: i64) {
        let clamp = |v: i64, flag: u32, flags: &mut u32| -> i16 {
            if v < -0x400 {
                *flags |= flag;
                -0x400
            } else if v > 0x3FF {
                *flags |= flag;
                0x3FF
            } else {
                v as i16
            }
        };
        let x = clamp(x, SX_SATURATED, &mut self.flag);
        let y = clamp(y, SY_SATURATED, &mut self.flag);
        self.sxy = [self.sxy[1], self.sxy[2], [x, y]];
    }

    /// Empilha a cor de MAC1–3 / 16 na FIFO de cores, com o CODE do RGBC.
    fn push_color(&mut self) {
        let mut color = [0; 4];
        for (i, out) in color.iter_mut().take(3).enumerate() {
            let c = self.mac[i + 1] >> 4;
            *out = if c < 0 {
                self.flag |= COLOR_SATURATED[i];
                0
            } else if c > 0xFF {
                self.flag |= COLOR_SATURATED[i];
                0xFF
            } else {
                c as u8
            };
        }
        color[3] = self.rgbc[3];
        self.rgb = [self.rgb[1], self.rgb[2], color];
    }

    fn ir_vector(&self) -> [i16; 3] {
        [self.ir[1], self.ir[2], self.ir[3]]
    }

    /// MAC/IR = T × 1000h + M × V.
    fn multiply(&mut self, m: &Matrix, v: [i16; 3], t: [i32; 3], shift: u32, lm: bool) {
        for i in 0..3 {
            let mut x = self.extend(
                i + 1,
                (i64::from(t[i]) << 12) + i64::from(m[i][0]) * i64::from(v[0]),
            );
            x = self.extend(i + 1, x + i64::from(m[i][1]) * i64::from(v[1]));
            x = self.extend(i + 1, x + i64::from(m[i][2]) * i64::from(v[2]));
            self.set_mac_ir(i + 1, x, shift, lm);
        }
    }

    /// Divisão da perspectiva H / SZ3 pelo método UNR (17 bits).
    fn divide(&mut self) -> i64 {
        let h = u64::from(self.h);
        let z = u64::from(self.sz[3]);
        if h >= z * 2 {
            self.flag |= DIVIDE_OVERFLOW;
            return 0x1FFFF;
        }
        let shift = (z as u16).leading_zeros();
        let n = h << shift;
        let d = z << shift;
        let u = u64::from(UNR[((d - 0x7FC0) >> 7) as usize]) + 0x101;
        let d = (0x200_0080 - d * u) >> 8;
        let d = (0x000_0080 + d * u) >> 8;
        (((n * d) + 0x8000) >> 16).min(0x1FFFF) as i64
    }

    // --- Comandos ---

    /// RTPS / RTPT: transforma o vértice `n` e projeta na tela.
    fn rtp(&mut self, n: usize, shift: u32, lm: bool, last: bool) {
        let v = self.v[n];
        let mut z_raw = 0;
        for i in 0..3 {
            let mut x = self.extend(
                i + 1,
                (i64::from(self.tr[i]) << 12) + i64::from(self.rt[i][0]) * i64::from(v[0]),
            );
            x = self.extend(i + 1, x + i64::from(self.rt[i][1]) * i64::from(v[1]));
            x = self.extend(i + 1, x + i64::from(self.rt[i][2]) * i64::from(v[2]));
            self.set_mac(i + 1, x, shift);
            if i == 2 {
                z_raw = x;
            }
        }
        self.set_ir(1, self.mac[1], lm);
        self.set_ir(2, self.mac[2], lm);
        // Com sf = 0, a flag do IR3 olha MAC3 >> 12, embora o valor sature
        // pelo MAC3.
        let z = z_raw >> 12;
        let flags_before = self.flag;
        self.set_ir(3, self.mac[3], lm);
        self.flag = flags_before;
        if !(-0x8000..=0x7FFF).contains(&z) {
            self.flag |= IR_SATURATED[3];
        }
        self.push_sz(z);

        let div = self.divide();
        let x = self.check_mac0(i64::from(self.ofx) + i64::from(self.ir[1]) * div);
        let y = self.check_mac0(i64::from(self.ofy) + i64::from(self.ir[2]) * div);
        let (sx, sy) = (x >> 16, y >> 16);
        self.push_sxy(sx, sy);
        // Saturado, o vértice não é o da conta: fica sem fração.
        let range = -0x400..=0x3FF;
        if range.contains(&sx) && range.contains(&sy) {
            self.projected[self.projected_len] = Projected {
                word: pack(sx as i16, sy as i16),
                fraction: ((x & 0xFFFF) as u16, (y & 0xFFFF) as u16),
            };
            self.projected_len += 1;
        }
        if last {
            let depth = i64::from(self.dqb) + i64::from(self.dqa) * div;
            self.set_mac0(depth);
            self.set_ir0(depth >> 12);
        }
    }

    fn nclip(&mut self) {
        let [a, b, c] = self.sxy.map(|p| p.map(i64::from));
        let value =
            a[0] * b[1] + b[0] * c[1] + c[0] * a[1] - a[0] * c[1] - b[0] * a[1] - c[0] * b[1];
        self.set_mac0(value);
    }

    fn op(&mut self, shift: u32, lm: bool) {
        let d = [self.rt[0][0], self.rt[1][1], self.rt[2][2]].map(i64::from);
        let ir = self.ir_vector().map(i64::from);
        self.set_mac_ir(1, ir[2] * d[1] - ir[1] * d[2], shift, lm);
        self.set_mac_ir(2, ir[0] * d[2] - ir[2] * d[0], shift, lm);
        self.set_mac_ir(3, ir[1] * d[0] - ir[0] * d[1], shift, lm);
    }

    /// MAC = MAC + (FC − MAC) × IR0, partindo de `mac` (já em 12 bits de
    /// fração).
    fn interpolate(&mut self, mac: [i64; 3], shift: u32, lm: bool) {
        for (i, &m) in mac.iter().enumerate() {
            self.set_mac_ir(i + 1, (i64::from(self.fc[i]) << 12) - m, shift, false);
        }
        let ir0 = i64::from(self.ir[0]);
        for (i, &m) in mac.iter().enumerate() {
            let value = i64::from(self.ir[i + 1]) * ir0 + m;
            self.set_mac_ir(i + 1, value, shift, lm);
        }
        self.push_color();
    }

    fn dpc(&mut self, color: [u8; 4], shift: u32, lm: bool) {
        self.interpolate([0, 1, 2].map(|i| i64::from(color[i]) << 16), shift, lm);
    }

    fn intpl(&mut self, shift: u32, lm: bool) {
        let ir = self.ir_vector();
        self.interpolate(ir.map(|v| i64::from(v) << 12), shift, lm);
    }

    fn dcpl(&mut self, shift: u32, lm: bool) {
        let mac = self.color_times_ir();
        self.interpolate(mac, shift, lm);
    }

    /// [R × IR1, G × IR2, B × IR3] << 4.
    fn color_times_ir(&self) -> [i64; 3] {
        [0, 1, 2].map(|i| (i64::from(self.rgbc[i]) * i64::from(self.ir[i + 1])) << 4)
    }

    fn mvmva(&mut self, instruction: u32, shift: u32, lm: bool) {
        let matrix = match (instruction >> 17) & 3 {
            0 => self.rt,
            1 => self.llm,
            2 => self.lcm,
            // Matriz inválida: o chip usa valores fixos e de registradores.
            _ => {
                let r = i16::from(self.rgbc[0]) << 4;
                [[-r, r, self.ir[0]], [self.rt[0][2]; 3], [self.rt[1][1]; 3]]
            }
        };
        let vector = match (instruction >> 15) & 3 {
            n @ 0..=2 => self.v[n as usize],
            _ => self.ir_vector(),
        };
        let translation = match (instruction >> 13) & 3 {
            0 => Translation::Tr,
            1 => Translation::Bk,
            2 => Translation::Fc,
            _ => Translation::None,
        };
        let t = match translation {
            Translation::Tr => self.tr,
            Translation::Bk => self.bk,
            Translation::Fc => self.fc,
            Translation::None => [0; 3],
        };
        if translation == Translation::Fc {
            // Erro do chip: com FC, só a primeira coluna soma com a
            // translação (e só afeta as flags); o resultado usa as outras.
            for i in 0..3 {
                let partial = self.extend(
                    i + 1,
                    (i64::from(t[i]) << 12) + i64::from(matrix[i][0]) * i64::from(vector[0]),
                );
                self.set_ir(i + 1, (partial >> shift) as i32, false);
                let mut x = self.extend(i + 1, i64::from(matrix[i][1]) * i64::from(vector[1]));
                x = self.extend(i + 1, x + i64::from(matrix[i][2]) * i64::from(vector[2]));
                self.set_mac_ir(i + 1, x, shift, lm);
            }
        } else {
            self.multiply(&matrix, vector, t, shift, lm);
        }
    }

    /// Luz: IR = LLM × V; IR = BK + LCM × IR.
    fn light(&mut self, n: usize, shift: u32, lm: bool) {
        let llm = self.llm;
        let lcm = self.lcm;
        self.multiply(&llm, self.v[n], [0; 3], shift, lm);
        self.multiply(&lcm, self.ir_vector(), self.bk, shift, lm);
    }

    fn nc(&mut self, n: usize, shift: u32, lm: bool) {
        self.light(n, shift, lm);
        self.push_color();
    }

    fn ncc(&mut self, n: usize, shift: u32, lm: bool) {
        self.light(n, shift, lm);
        self.color_multiply(shift, lm);
    }

    fn ncd(&mut self, n: usize, shift: u32, lm: bool) {
        self.light(n, shift, lm);
        let mac = self.color_times_ir();
        self.interpolate(mac, shift, lm);
    }

    fn cc(&mut self, shift: u32, lm: bool) {
        let lcm = self.lcm;
        self.multiply(&lcm, self.ir_vector(), self.bk, shift, lm);
        self.color_multiply(shift, lm);
    }

    fn cdp(&mut self, shift: u32, lm: bool) {
        let lcm = self.lcm;
        self.multiply(&lcm, self.ir_vector(), self.bk, shift, lm);
        let mac = self.color_times_ir();
        self.interpolate(mac, shift, lm);
    }

    fn color_multiply(&mut self, shift: u32, lm: bool) {
        let mac = self.color_times_ir();
        for (i, &m) in mac.iter().enumerate() {
            self.set_mac_ir(i + 1, m, shift, lm);
        }
        self.push_color();
    }

    fn sqr(&mut self, shift: u32, lm: bool) {
        for i in 1..=3 {
            let v = i64::from(self.ir[i]);
            self.set_mac_ir(i, v * v, shift, lm);
        }
    }

    fn avsz(&mut self, factor: i64, first: usize) {
        let sum: i64 = self.sz[first..].iter().map(|&z| i64::from(z)).sum();
        let value = factor * sum;
        self.set_mac0(value);
        let otz = value >> 12;
        self.otz = if otz < 0 {
            self.flag |= SZ_SATURATED;
            0
        } else if otz > 0xFFFF {
            self.flag |= SZ_SATURATED;
            0xFFFF
        } else {
            otz as u16
        };
    }

    fn gpf(&mut self, shift: u32, lm: bool) {
        let ir0 = i64::from(self.ir[0]);
        for i in 1..=3 {
            let value = ir0 * i64::from(self.ir[i]);
            self.set_mac_ir(i, value, shift, lm);
        }
        self.push_color();
    }

    fn gpl(&mut self, shift: u32, lm: bool) {
        let ir0 = i64::from(self.ir[0]);
        for i in 1..=3 {
            let value = (i64::from(self.mac[i]) << shift) + ir0 * i64::from(self.ir[i]);
            let value = self.extend(i, value);
            self.set_mac_ir(i, value, shift, lm);
        }
        self.push_color();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unr_table_matches_known_entries() {
        assert_eq!(UNR[0], 0xFF);
        assert_eq!(UNR[1], 0xFD);
        assert_eq!(UNR[256], 0x00);
    }

    #[test]
    fn rtps_projects_with_identity_matrix() {
        let mut gte = Gte::default();
        // RT = identidade (1.0 = 0x1000), H = 0x100, Z = 0x200.
        gte.write_control(0, 0x1000);
        gte.write_control(2, 0x1000);
        gte.write_control(4, 0x1000);
        gte.write_control(26, 0x100);
        gte.write_data(0, pack(0x40, -0x20));
        gte.write_data(1, 0x200);
        gte.command(0x0008_0001); // RTPS, sf = 1
        assert_eq!(gte.read_data(19), 0x200, "SZ3");
        // x = 0x40 × 0x100 / 0x200 = 0x20; y = −0x10.
        assert_eq!(gte.read_data(14), pack(0x20, -0x10));
        assert_eq!(gte.read_control(31), 0);
    }

    #[test]
    fn rtps_keeps_the_fraction_the_hardware_drops() {
        let mut gte = Gte::default();
        gte.write_control(0, 0x1000);
        gte.write_control(2, 0x1000);
        gte.write_control(4, 0x1000);
        gte.write_control(26, 0x100);
        gte.write_data(0, pack(0x41, -0x20));
        gte.write_data(1, 0x200);
        gte.command(0x0008_0001); // RTPS, sf = 1
        // x = 0x41 × 0x100 / 0x200 = 0x20,8: o SXY guarda 0x20 e a fração
        // meio pixel.
        assert_eq!(gte.read_data(14), pack(0x20, -0x10));
        assert_eq!(
            gte.projected(),
            [Projected {
                word: pack(0x20, -0x10),
                fraction: (0x8000, 0),
            }]
        );
        gte.write_control(24, 0x7FFF_0000); // OFX enorme: SX satura
        gte.command(0x0008_0001);
        assert!(
            gte.projected().is_empty(),
            "vértice saturado fica sem fração"
        );
        gte.command(0x06); // NCLIP não projeta
        assert!(gte.projected().is_empty());
    }

    #[test]
    fn rtpt_projects_three_vertices() {
        let mut gte = Gte::default();
        gte.write_control(0, 0x1000);
        gte.write_control(2, 0x1000);
        gte.write_control(4, 0x1000);
        gte.write_control(26, 0x100);
        for (i, x) in [0x40, 0x41, 0x42].into_iter().enumerate() {
            gte.write_data(i * 2, pack(x, 0));
            gte.write_data(i * 2 + 1, 0x200);
        }
        gte.command(0x0008_0030); // RTPT, sf = 1
        let fractions: Vec<_> = gte.projected().iter().map(|p| p.fraction.0).collect();
        assert_eq!(fractions, [0, 0x8000, 0]);
        assert_eq!(gte.projected()[2].word, gte.read_data(14));
    }

    #[test]
    fn divide_overflow_sets_flag() {
        let mut gte = Gte::default();
        gte.write_control(26, 0x1000);
        gte.command(0x0008_0001); // vértice e SZ3 = 0
        assert_ne!(gte.read_control(31) & (DIVIDE_OVERFLOW | (1 << 31)), 0);
    }

    #[test]
    fn nclip_gives_winding() {
        let mut gte = Gte::default();
        gte.write_data(12, pack(0, 0));
        gte.write_data(13, pack(10, 0));
        gte.write_data(14, pack(0, 10));
        gte.command(0x06);
        assert_eq!(gte.read_data(24), 100);
    }

    #[test]
    fn avsz3_averages_depth() {
        let mut gte = Gte::default();
        gte.write_control(29, 0x555);
        for (r, z) in [(17, 300), (18, 300), (19, 300)] {
            gte.write_data(r, z);
        }
        gte.command(0x2D);
        assert_eq!(gte.read_data(7), (0x555 * 900) >> 12);
    }

    #[test]
    fn irgb_orgb_and_lzcr() {
        let mut gte = Gte::default();
        gte.write_data(28, 0x7FFF);
        assert_eq!(gte.read_data(9), 0x1F * 0x80);
        assert_eq!(gte.read_data(29), 0x7FFF);
        gte.write_data(30, 0x0000_FFFF);
        assert_eq!(gte.read_data(31), 16);
        gte.write_data(30, 0xFFF0_0000);
        assert_eq!(gte.read_data(31), 12);
    }

    #[test]
    fn ir_saturation_sets_error_flag() {
        let mut gte = Gte::default();
        gte.write_data(9, 0x7000);
        gte.write_data(10, 0x7000);
        gte.write_data(11, 0x7000);
        gte.command(0x28); // SQR, sf = 0
        assert_eq!(gte.read_data(9), 0x7FFF);
        assert_ne!(gte.read_control(31) & IR_SATURATED[1], 0);
        assert_ne!(gte.read_control(31) >> 31, 0);
    }
}
