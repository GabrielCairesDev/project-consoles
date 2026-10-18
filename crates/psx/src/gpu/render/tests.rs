use super::*;

const WHITE: u16 = 0x7FFF;

/// Renderizador com a área de desenho na VRAM inteira.
fn renderer() -> Renderer {
    let mut r = Renderer::default();
    r.gp0(0xE300_0000);
    r.gp0(0xE400_0000 | 1023 | (511 << 10));
    r
}

fn send(r: &mut Renderer, words: &[u32]) {
    for &w in words {
        r.gp0(w);
    }
}

fn xy(x: i32, y: i32) -> u32 {
    (x as u32 & 0xFFFF) | ((y as u32) << 16)
}

fn count(r: &Renderer, value: u16) -> usize {
    r.vram.iter().filter(|&&p| p == value).count()
}

#[test]
fn flat_triangle_follows_top_left_rule() {
    let mut r = renderer();
    // Triângulo retângulo de catetos 4: 4 + 3 + 2 + 1 = 10 pixels.
    send(&mut r, &[0x20FF_FFFF, xy(0, 0), xy(4, 0), xy(0, 4)]);
    assert_eq!(count(&r, WHITE), 10);
    assert_eq!(r.pixel(0, 0), WHITE);
    assert_eq!(r.pixel(3, 0), WHITE);
    assert_eq!(r.pixel(4, 0), 0, "borda direita fica de fora");
    assert_eq!(r.pixel(0, 4), 0, "borda de baixo fica de fora");
}

#[test]
fn quad_covers_rectangle_without_gaps_or_overlap() {
    let mut r = renderer();
    // Semitransparente aditivo: sobreposição apareceria mais clara.
    send(&mut r, &[0xE100_0020]); // modo 1 (B + F)
    send(
        &mut r,
        &[0x2A08_0808, xy(0, 0), xy(10, 0), xy(0, 6), xy(10, 6)],
    );
    assert_eq!(count(&r, 0x0421), 60);
}

#[test]
fn drawing_offset_and_area_clip() {
    let mut r = Renderer::default();
    r.gp0(0xE300_0000 | 2 | (2 << 10));
    r.gp0(0xE400_0000 | 5 | (5 << 10));
    r.gp0(0xE500_0000 | 1 | (1 << 11));
    send(&mut r, &[0x6000_FFFF, xy(0, 0), xy(10, 10)]);
    // Retângulo de (1, 1) a (10, 10), cortado para (2..=5, 2..=5).
    assert_eq!(count(&r, 0x03FF), 16);
}

#[test]
fn gouraud_interpolates_color() {
    let mut r = renderer();
    // Vermelho → preto ao longo de x.
    send(
        &mut r,
        &[0x30FF_0000 | 0xFF, xy(0, 0), 0, xy(64, 0), 0, xy(0, 64)],
    );
    let left = r.pixel(0, 0) & 0x1F;
    let mid = r.pixel(32, 0) & 0x1F;
    assert_eq!(left, 31);
    assert!((14..=17).contains(&mid), "{mid}");
}

#[test]
fn textured_rectangle_with_clut_and_transparency() {
    let mut r = renderer();
    // CLUT em (0, 256): índice 1 = branco, índice 0 = transparente (0).
    r.vram[256 * VRAM_WIDTH + 1] = WHITE;
    // Página 4 bits em x = 64: primeiro halfword com índices 1, 0, 1, 0.
    r.vram[64] = 0x0101;
    send(&mut r, &[0xE100_0001]); // página x = 64, 4 bits
    send(
        &mut r,
        &[0x6580_8080, xy(100, 100), (256 << 6) << 16, (1 << 16) | 4],
    );
    assert_eq!(r.pixel(100, 100), WHITE);
    assert_eq!(r.pixel(101, 100), 0, "texel 0 é transparente");
    assert_eq!(r.pixel(102, 100), WHITE);
}

#[test]
fn texture_modulation_halves_with_color_64() {
    let mut r = renderer();
    r.vram[64] = 0x7FFF; // 15 bits, branco
    send(&mut r, &[0xE100_0101]); // página x = 64, 15 bits
    send(&mut r, &[0x6440_4040, xy(0, 0), 0, (1 << 16) | 1]);
    assert_eq!(r.pixel(0, 0), 0x3DEF, "31 × 64 / 128 = 15");
}

#[test]
fn mask_bit_protects_pixels() {
    let mut r = renderer();
    send(&mut r, &[0xE600_0001]); // força o bit 15
    send(&mut r, &[0x6000_00FF, xy(0, 0), (1 << 16) | 1]);
    assert_eq!(r.pixel(0, 0), 0x801F);
    send(&mut r, &[0xE600_0002]); // respeita o bit 15
    send(&mut r, &[0x60FF_0000, xy(0, 0), (1 << 16) | 2]);
    assert_eq!(r.pixel(0, 0), 0x801F, "protegido");
    assert_eq!(r.pixel(1, 0), 0x7C00, "sem máscara: escrito");
}

#[test]
fn cpu_to_vram_and_back() {
    let mut r = renderer();
    send(
        &mut r,
        &[0xA000_0000, xy(10, 20), xy(3, 1), 0x2222_1111, 0x0000_3333],
    );
    assert_eq!(
        (r.pixel(10, 20), r.pixel(11, 20), r.pixel(12, 20)),
        (0x1111, 0x2222, 0x3333)
    );
    send(&mut r, &[0xC000_0000, xy(10, 20), xy(2, 1)]);
    assert!(r.has_read_data());
    assert_eq!(r.read(), 0x2222_1111);
    assert!(!r.has_read_data());
}

#[test]
fn fill_ignores_clip_and_aligns_width() {
    let mut r = Renderer::default();
    send(&mut r, &[0x0200_00FF, xy(0x13, 0), xy(1, 2)]);
    // x alinhado a 16 (0x10) e largura arredondada para 16.
    assert_eq!(count(&r, 0x001F), 32);
    assert_eq!(r.pixel(0x10, 1), 0x001F);
}

#[test]
fn vram_to_vram_copy() {
    let mut r = renderer();
    r.vram[5] = 0x1234;
    send(&mut r, &[0x8000_0000, xy(5, 0), xy(100, 50), xy(1, 1)]);
    assert_eq!(r.pixel(100, 50), 0x1234);
}

#[test]
fn polyline_until_terminator() {
    let mut r = renderer();
    send(
        &mut r,
        &[0x4800_00FF, xy(0, 0), xy(4, 0), xy(4, 4), 0x5555_5555],
    );
    // 5 pixels por segmento, o canto compartilhado: 9.
    assert_eq!(count(&r, 0x001F), 9);
    // Depois do terminador, um comando novo funciona.
    send(&mut r, &[0x6000_FF00, xy(50, 50), (1 << 16) | 1]);
    assert_eq!(r.pixel(50, 50), 0x03E0);
}

#[test]
fn oversized_polygon_is_dropped() {
    let mut r = renderer();
    send(&mut r, &[0x20FF_FFFF, xy(0, 0), xy(1023, 0), xy(0, 10)]);
    assert!(count(&r, WHITE) > 0);
    let mut r = renderer();
    send(&mut r, &[0x20FF_FFFF, xy(-1, 0), xy(1023, 0), xy(0, 10)]);
    assert_eq!(count(&r, WHITE), 0);
}

#[test]
fn blend_modes() {
    let b = 0x0010; // R = 16
    let f = 0x0008; // R = 8
    assert_eq!(blend(b, f, 0), 12);
    assert_eq!(blend(b, f, 1), 24);
    assert_eq!(blend(b, f, 2), 8);
    assert_eq!(blend(b, f, 3), 18);
    assert_eq!(blend(0x1F, 0x1F, 1), 0x1F, "satura");
}

#[test]
fn flipped_rectangle_starts_at_odd_texel() {
    let mut r = renderer();
    // Página 15 bits em x = 64; texels u = 0..4 com valores 1..5.
    for u in 0..4 {
        r.vram[64 + u] = 1 + u as u16;
    }
    send(&mut r, &[0xE100_1101]); // 15 bits, espelhado em X
    send(&mut r, &[0x6580_8080, xy(10, 0), 0, (1 << 16) | 2]);
    // Começa em u0 | 1 = 1 e anda para trás.
    assert_eq!((r.pixel(10, 0), r.pixel(11, 0)), (2, 1));
}

#[test]
fn line_steps_like_the_chip() {
    let mut r = renderer();
    // Desce 1 pixel em 80: o degrau cai exatamente na metade (x = 40).
    send(&mut r, &[0x4000_00FF, xy(0, 0), xy(80, 1)]);
    assert_eq!((r.pixel(39, 0), r.pixel(40, 1)), (0x1F, 0x1F));
    // Anda 1 pixel em x ao longo de 80 linhas: o degrau fica para depois.
    send(&mut r, &[0x4000_00FF, xy(100, 0), xy(101, 80)]);
    assert_eq!((r.pixel(100, 40), r.pixel(101, 41)), (0x1F, 0x1F));
}

#[test]
fn flat_lines_are_dithered() {
    let mut r = renderer();
    send(&mut r, &[0xE100_0200]); // dithering ligado
    // 170 - 4 (matriz em (0, 0)) = 166 → 20 em 5 bits; sem dithering, 21.
    send(&mut r, &[0x4000_00AA, xy(0, 0), xy(0, 0)]);
    assert_eq!(r.pixel(0, 0), 20);
}

#[test]
fn overlapping_copy_to_the_right_goes_backwards() {
    let mut r = renderer();
    for x in 0..4 {
        r.vram[x] = 1 + x as u16;
    }
    // Copiar 4 pixels uma posição à direita preserva a sequência.
    send(&mut r, &[0x8000_0000, xy(0, 0), xy(1, 0), xy(4, 1)]);
    assert_eq!(&r.vram[..5], &[1, 1, 2, 3, 4]);
}

/// CLUT de 8 bits em (0, 256) com cor = índice e uma textura de 8 bits em
/// x = 64 com os índices 0..=3 (índice 0 dá cor 0, transparente).
fn clut_scene() -> Renderer {
    let mut r = renderer();
    for i in 0..256 {
        r.vram[256 * VRAM_WIDTH + i] = i as u16;
    }
    r.vram[64] = 0x0100 | 2; // índices 2, 1
    r.vram[65] = 0x0003; // índices 3, 0
    r
}

fn draw_clut_row(r: &mut Renderer, page: u32, y: i32) {
    send(r, &[0xE100_0000 | page]);
    send(r, &[0x6580_8080, xy(0, y), (256 << 6) << 16, (1 << 16) | 4]);
}

#[test]
fn clut_cache_ignores_vram_writes_until_cleared() {
    let mut r = clut_scene();
    draw_clut_row(&mut r, 0x81, 10); // 8 bits
    assert_eq!(r.pixel(0, 10), 2);
    r.vram[256 * VRAM_WIDTH + 2] = WHITE;
    draw_clut_row(&mut r, 0x81, 11);
    assert_eq!(r.pixel(0, 11), 2, "cache antigo");
    send(&mut r, &[0x0100_0000]); // limpa o cache
    draw_clut_row(&mut r, 0x81, 12);
    assert_eq!(r.pixel(0, 12), WHITE);
}

#[test]
fn clut_cache_reloads_only_when_more_colors_are_needed() {
    // 8 bits → 4 bits: as 256 cores já estão no cache.
    let mut r = clut_scene();
    draw_clut_row(&mut r, 0x81, 10);
    r.vram[256 * VRAM_WIDTH + 2] = WHITE;
    draw_clut_row(&mut r, 0x01, 11);
    assert_eq!(r.pixel(0, 11), 2);

    // 4 bits → 8 bits: só 16 cores estavam no cache, recarrega.
    let mut r = clut_scene();
    draw_clut_row(&mut r, 0x01, 10);
    r.vram[256 * VRAM_WIDTH + 2] = WHITE;
    draw_clut_row(&mut r, 0x81, 11);
    assert_eq!(r.pixel(0, 11), WHITE);
}

/// Gerador congruencial: cena determinística sem dependências.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    fn coord(&mut self) -> u32 {
        let x = self.below(420) as i32 - 40;
        let y = self.below(300) as i32 - 30;
        xy(x, y)
    }
}

/// Comandos variados do GP0 (todos os primitivos, modos, máscara, CLUT,
/// janela de textura, transferências e cópias); devolve as palavras lidas
/// pelo GPUREAD no caminho.
fn random_scene(r: &mut Renderer, seed: u64, commands: usize) -> Vec<u32> {
    let mut rng = Lcg(seed);
    let mut reads = Vec::new();
    // Texturas e CLUTs aleatórias na metade de baixo da VRAM.
    send(r, &[0xA000_0000, xy(512, 256), xy(256, 64)]);
    for _ in 0..(256 * 64 / 2) {
        r.gp0(rng.next() | (rng.next() << 16));
    }
    for _ in 0..commands {
        match rng.below(16) {
            0 => r.gp0(0xE100_0000 | rng.below(0x4000)),
            1 => r.gp0(0xE200_0000 | (rng.below(4) * 0x21) | (rng.below(4) << 10)),
            2 => {
                r.gp0(0xE300_0000 | rng.below(8) | (rng.below(8) << 10));
                r.gp0(0xE400_0000 | (380 + rng.below(40)) | ((250 + rng.below(30)) << 10));
            }
            3 => r.gp0(0xE500_0000 | (rng.below(16) | (rng.below(16) << 11))),
            4 => r.gp0(0xE600_0000 | rng.below(4)),
            5 => r.gp0(0x0100_0000),
            6 => send(
                r,
                &[
                    0x0200_0000 | rng.next() & 0xFF_FFFF,
                    xy(rng.below(400) as i32, rng.below(260) as i32),
                    xy(rng.below(64) as i32, rng.below(48) as i32),
                ],
            ),
            7 => send(
                r,
                &[
                    0x8000_0000,
                    xy(rng.below(400) as i32, rng.below(260) as i32),
                    xy(rng.below(400) as i32, rng.below(260) as i32),
                    xy(1 + rng.below(48) as i32, 1 + rng.below(32) as i32),
                ],
            ),
            8 => {
                send(r, &[0xC000_0000, xy(rng.below(400) as i32, 0), xy(4, 2)]);
                for _ in 0..4 {
                    reads.push(r.read());
                }
            }
            9 => {
                // Polilinha com terminador.
                let command = 0x48 | (rng.below(2) << 4) | (rng.below(2) << 1);
                r.gp0((command << 24) | rng.next() & 0xFF_FFFF);
                for n in 0..2 + rng.below(3) {
                    if command & 0x10 != 0 && n > 0 {
                        r.gp0(rng.next() & 0xFF_FFFF);
                    }
                    r.gp0(rng.coord());
                }
                r.gp0(0x5555_5555);
            }
            _ => {
                // Polígono, linha simples ou retângulo, com os dados certos.
                let command = match rng.below(3) {
                    0 | 1 => 0x20 + rng.below(32),
                    _ => 0x60 + rng.below(32),
                };
                let command = if rng.below(6) == 0 {
                    0x40 | (command & 0x12)
                } else {
                    command
                };
                let words = command_length(command).unwrap_or(1);
                let textured = command & 0x04 != 0 && command & 0xE0 != 0x40;
                let mut out = vec![(command << 24) | rng.next() & 0xFF_FFFF];
                let gouraud = command & 0x10 != 0 && command < 0x60;
                // Ordem das palavras: [cor] vértice [uv] por vértice.
                let mut n = 0;
                while out.len() < words {
                    if command >= 0x60 {
                        out.push(rng.coord());
                        if textured {
                            let clut = (rng.below(16) + 32) | ((256 + rng.below(64)) << 6);
                            out.push(rng.below(0x1_0000) | (clut << 16));
                        }
                        if out.len() < words {
                            out.push(xy(rng.below(80) as i32, rng.below(80) as i32));
                        }
                        continue;
                    }
                    if gouraud && n > 0 {
                        out.push(rng.next() & 0xFF_FFFF);
                    }
                    out.push(rng.coord());
                    if textured {
                        let high = match n {
                            0 => (rng.below(16) + 32) | ((256 + rng.below(64)) << 6),
                            1 => {
                                (8 + rng.below(8))
                                    | 0x10
                                    | (rng.below(4) << 5)
                                    | (rng.below(3) << 7)
                            }
                            _ => 0,
                        };
                        out.push(rng.below(0x1_0000) | (high << 16));
                    }
                    n += 1;
                }
                out.truncate(words);
                send(r, &out);
            }
        }
    }
    reads
}

/// FNV-1a da VRAM e das leituras.
fn fingerprint(vram: &[u16], reads: &[u32]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    let bytes = vram
        .iter()
        .flat_map(|p| p.to_le_bytes())
        .chain(reads.iter().flat_map(|w| w.to_le_bytes()));
    for b in bytes {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01B3);
    }
    hash
}

/// Hash da VRAM e das leituras de cada cena com o rasterizador de antes da
/// resolução interna (commit 501381a): a imagem 1× não pode mudar.
const GOLDEN: [(u64, u64); 3] = [
    (1, 0xfa31_69ad_ec17_9c4f),
    (2, 0x7cb4_3767_b2e0_74e9),
    (3, 0xf217_e4a3_5c75_4feb),
];

#[test]
fn native_rendering_is_unchanged() {
    for (seed, expected) in GOLDEN {
        let mut r = renderer();
        let reads = random_scene(&mut r, seed, 3000);
        assert_eq!(fingerprint(&r.vram, &reads), expected, "cena {seed}");
    }
}

#[test]
fn internal_resolution_does_not_touch_native_vram() {
    for (seed, expected) in GOLDEN {
        for scale in 2..=InternalResolution::X4.scale() {
            let mut r = renderer();
            r.set_enhancements(at_scale(scale));
            let reads = random_scene(&mut r, seed, 3000);
            assert_eq!(
                fingerprint(&r.vram, &reads),
                expected,
                "cena {seed} em {scale}×"
            );
        }
    }
}

fn at_scale(scale: usize) -> Enhancements {
    Enhancements {
        resolution: InternalResolution::from_scale(scale).expect("1 a 4"),
        ..Enhancements::default()
    }
}

/// Pixels da VRAM ampliada, em 15 bits (exato sem a cor de 24 bits).
fn upscaled(r: &Renderer) -> Vec<u16> {
    let up = r.enhanced().expect("cópia melhorada");
    up.pixels.iter().map(|&p| compress(p)).collect()
}

fn hi_pixel(r: &Renderer, x: usize, y: usize) -> u16 {
    compress(hi_pixel_24(r, x, y))
}

fn hi_pixel_24(r: &Renderer, x: usize, y: usize) -> u32 {
    r.enhanced().expect("cópia melhorada").row(y)[x]
}

/// A VRAM ampliada é exatamente a 1× com cada pixel repetido.
fn assert_blocks_of_native(r: &Renderer) {
    let up = r.enhanced().expect("cópia melhorada");
    let s = up.scale();
    for y in 0..up.height() {
        for (x, &p) in up.row(y).iter().enumerate() {
            assert_eq!(p, expand(r.pixel(x / s, y / s)), "({x}, {y}) em {s}×");
        }
    }
}

#[test]
fn scale_one_has_no_enhanced_copy() {
    let mut r = renderer();
    r.set_enhancements(at_scale(1));
    assert!(r.enhanced().is_none());
    assert_eq!(r.enhancements(), Enhancements::default());
    r.set_enhancements(Enhancements {
        precise_geometry: true,
        ..at_scale(1)
    });
    assert!(r.enhanced().is_none(), "geometria precisa só vale ampliada");
}

#[test]
fn expand_and_compress_round_trip_every_15_bit_color() {
    for pixel in 0..=u16::MAX {
        assert_eq!(compress(expand(pixel)), pixel);
    }
    assert_eq!(expand(0x7FFF), 0x00FF_FFFF);
    assert_eq!(
        expand(0x801F),
        MASK_24 | 0x00FF_0000,
        "vermelho nos bits 0–4"
    );
}

#[test]
fn enabling_the_scale_copies_what_is_already_in_vram() {
    let mut r = renderer();
    random_scene(&mut r, 7, 300);
    r.set_enhancements(at_scale(3));
    assert_blocks_of_native(&r);
}

#[test]
fn transfers_fills_rectangles_and_lines_are_blocks_of_native() {
    // Sem polígonos tudo cai na grade 1×: a cópia é a 1× ampliada.
    for scale in 2..=InternalResolution::X4.scale() {
        let mut r = renderer();
        r.set_enhancements(at_scale(scale));
        send(&mut r, &[0x0200_1F00, xy(16, 8), xy(40, 20)]);
        send(
            &mut r,
            &[
                0xA000_0000,
                xy(1020, 30),
                xy(6, 1),
                0x2222_1111,
                0x4444_3333,
                0x6666_5555,
            ],
        );
        send(&mut r, &[0x6200_7C00, xy(30, 12), xy(9, 5)]); // semitransparente
        send(&mut r, &[0x5000_00FF, xy(2, 2), 0x0000_FF00, xy(60, 37)]);
        send(&mut r, &[0x8000_0000, xy(16, 8), xy(100, 100), xy(30, 20)]);
        send(&mut r, &[0xE600_0001]);
        send(&mut r, &[0x6000_03E0, xy(101, 101), (2 << 16) | 2]);
        send(&mut r, &[0xE600_0002]);
        send(&mut r, &[0x6000_7FFF, xy(100, 100), (4 << 16) | 4]);
        assert_blocks_of_native(&r);
    }
}

#[test]
fn upscaled_triangle_has_finer_edges() {
    let mut r = renderer();
    r.set_enhancements(at_scale(2));
    // Catetos 4 viram 8 na cópia: 8 + 7 + … + 1 = 36 pixels (1×: 10).
    send(&mut r, &[0x20FF_FFFF, xy(0, 0), xy(4, 0), xy(0, 4)]);
    assert_eq!(count(&r, WHITE), 10);
    assert_eq!(upscaled(&r).iter().filter(|&&p| p == WHITE).count(), 36);
    assert_eq!(hi_pixel(&r, 7, 0), WHITE);
    assert_eq!(hi_pixel(&r, 8, 0), 0, "borda direita fica de fora");
    assert_eq!(hi_pixel(&r, 0, 7), WHITE);
}

#[test]
fn true_color_and_precise_geometry_do_not_touch_native_vram() {
    for (seed, expected) in GOLDEN {
        for scale in [1, 2] {
            let mut r = renderer();
            r.set_enhancements(Enhancements {
                true_color: true,
                precise_geometry: true,
                ..at_scale(scale)
            });
            // Frações para todos os vértices possíveis da cena não mudam a
            // VRAM 1×.
            for word in 0..2048u32 {
                r.remember_vertices(&[Projected {
                    word: word | (word << 16),
                    fraction: (0x8000, 0x4000),
                }]);
            }
            let reads = random_scene(&mut r, seed, 3000);
            assert_eq!(
                fingerprint(&r.vram, &reads),
                expected,
                "cena {seed} em {scale}× com 24 bits"
            );
        }
    }
}

/// Valores distintos de 24 bits nos pixels pintados da cópia melhorada.
fn distinct_24(r: &Renderer) -> std::collections::BTreeSet<u32> {
    let up = r.enhanced().expect("cópia melhorada");
    up.pixels.iter().copied().filter(|&p| p != 0).collect()
}

#[test]
fn true_color_gouraud_has_no_dithering() {
    let mut r = renderer();
    r.set_enhancements(Enhancements {
        true_color: true,
        ..at_scale(1)
    });
    send(&mut r, &[0xE100_0200]); // dithering ligado
    let color = 0x009B_5713; // R = 0x13, G = 0x57, B = 0x9B
    send(
        &mut r,
        &[
            0x3000_0000 | color,
            xy(0, 0),
            color,
            xy(16, 0),
            color,
            xy(0, 16),
        ],
    );
    let native: std::collections::BTreeSet<u16> =
        r.vram.iter().copied().filter(|&p| p != 0).collect();
    assert!(native.len() > 1, "1× tem o pontilhado");
    assert_eq!(distinct_24(&r), [0x0013_579B].into());
}

#[test]
fn without_true_color_the_enhanced_copy_keeps_15_bits() {
    let mut r = renderer();
    r.set_enhancements(at_scale(2));
    send(&mut r, &[0xE100_0200]);
    let color = 0x009B_5713;
    send(
        &mut r,
        &[
            0x3000_0000 | color,
            xy(0, 0),
            color,
            xy(16, 0),
            color,
            xy(0, 16),
        ],
    );
    for p in distinct_24(&r) {
        assert_eq!(expand(compress(p)), p, "{p:06X} não é 15 bits");
    }
}

#[test]
fn true_color_blends_in_8_bits() {
    let mut r = renderer();
    r.set_enhancements(Enhancements {
        true_color: true,
        ..at_scale(1)
    });
    // Fundo preto e retângulo vermelho semitransparente (B/2 + F/2).
    send(&mut r, &[0x6200_00FF, xy(0, 0), (1 << 16) | 1]);
    assert_eq!(hi_pixel_24(&r, 0, 0), 0x007F_0000);
    assert_eq!(r.pixel(0, 0), 0x000F, "1× mistura em 5 bits");
}

#[test]
fn true_color_modulates_textures_in_8_bits() {
    let mut r = renderer();
    r.set_enhancements(Enhancements {
        true_color: true,
        ..at_scale(1)
    });
    r.vram[64] = 0x801F; // texel vermelho com o bit 15
    send(&mut r, &[0xE100_0000 | 1 | (2 << 7)]); // página x = 64, 15 bits
    // Retângulo texturizado 1×1 modulado pela metade (0x40).
    send(&mut r, &[0x6400_4040, xy(0, 0), 0, (1 << 16) | 1]);
    assert_eq!(hi_pixel_24(&r, 0, 0), MASK_24 | 0x007F_0000);
    assert_eq!(r.pixel(0, 0), 0x800F);
}

#[test]
fn precise_geometry_moves_upscaled_vertices_only() {
    let triangle = [0x20FF_FFFF, xy(0, 0), xy(4, 0), xy(0, 4)];
    let draw = |precise_geometry: bool, remember: bool| {
        let mut r = renderer();
        r.set_enhancements(Enhancements {
            precise_geometry,
            ..at_scale(2)
        });
        if remember {
            // O vértice (4, 0) estava em 4,5 antes de o GTE truncar.
            r.remember_vertices(&[Projected {
                word: xy(4, 0),
                fraction: (0x8000, 0),
            }]);
        }
        send(&mut r, &triangle);
        r
    };
    for r in [draw(false, true), draw(true, false)] {
        assert_eq!(count(&r, WHITE), 10);
        assert_eq!(upscaled(&r).iter().filter(|&&p| p == WHITE).count(), 36);
    }
    let precise = draw(true, true);
    assert_eq!(count(&precise, WHITE), 10, "1× fica como no hardware");
    assert_eq!(hi_pixel(&precise, 8, 0), WHITE, "a borda foi para 9");
    assert!(upscaled(&precise).iter().filter(|&&p| p == WHITE).count() > 36);
}

#[test]
fn upscaled_triangle_respects_draw_area_in_blocks() {
    let mut r = Renderer::default();
    r.set_enhancements(at_scale(2));
    r.gp0(0xE300_0000 | 2 | (2 << 10));
    r.gp0(0xE400_0000 | 5 | (5 << 10));
    send(
        &mut r,
        &[0x28FF_FFFF, xy(0, 0), xy(20, 0), xy(0, 20), xy(20, 20)],
    );
    // Área 1× (2..=5, 2..=5) = 16 pixels; ampliada, (4..=11)² = 64.
    assert_eq!(count(&r, WHITE), 16);
    assert_eq!(upscaled(&r).iter().filter(|&&p| p == WHITE).count(), 64);
    assert_eq!(hi_pixel(&r, 4, 4), WHITE);
    assert_eq!(hi_pixel(&r, 11, 11), WHITE);
    assert_eq!(hi_pixel(&r, 12, 11), 0);
}

#[test]
fn upscaled_texture_has_more_detail() {
    let mut r = renderer();
    r.set_enhancements(at_scale(2));
    // Textura 15 bits em x = 64: texels 0..4 com cores diferentes.
    for (i, color) in [0x001F, 0x03E0, 0x7C00, 0x7FFF].into_iter().enumerate() {
        r.vram[64 + i] = color;
    }
    // Triângulo (raw) que reduz 4 texels a 2 pixels de largura: 1× mostra
    // texels 0 e 2; a cópia 2× também mostra 1 e 3.
    let page = (1 | (2 << 7)) << 16;
    send(
        &mut r,
        &[
            0x2580_8080,
            xy(0, 0),
            0,
            xy(2, 0),
            page | 4,
            xy(0, 8),
            (8 << 8),
        ],
    );
    assert_eq!((r.pixel(0, 0), r.pixel(1, 0)), (0x001F, 0x7C00));
    let row: Vec<u16> = (0..4).map(|x| hi_pixel(&r, x, 0)).collect();
    assert_eq!(row, [0x001F, 0x03E0, 0x7C00, 0x7FFF]);
}

#[test]
fn copy_keeps_upscaled_detail() {
    let mut r = renderer();
    r.set_enhancements(at_scale(2));
    send(&mut r, &[0x20FF_FFFF, xy(0, 0), xy(4, 0), xy(0, 4)]);
    send(&mut r, &[0x8000_0000, xy(0, 0), xy(200, 100), xy(4, 4)]);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                hi_pixel(&r, 400 + x, 200 + y),
                hi_pixel(&r, x, y),
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn upscaled_mask_check_protects_subpixels() {
    let mut r = renderer();
    r.set_enhancements(at_scale(2));
    send(&mut r, &[0xE600_0001]);
    send(&mut r, &[0x20FF_FFFF, xy(0, 0), xy(4, 0), xy(0, 4)]);
    send(&mut r, &[0xE600_0002]);
    send(&mut r, &[0x6000_00FF, xy(0, 0), (4 << 16) | 4]);
    assert_eq!(hi_pixel(&r, 0, 0), 0xFFFF, "protegido");
    assert_eq!(hi_pixel(&r, 7, 7), 0x001F, "fora do triângulo: escrito");
}

#[test]
fn reset_keeps_the_upscaled_copy() {
    let mut r = renderer();
    r.set_enhancements(at_scale(2));
    send(&mut r, &[0x0200_1F00, xy(0, 0), xy(16, 1)]);
    r.reset();
    assert_eq!(r.enhancements(), at_scale(2));
    assert_blocks_of_native(&r);
}
