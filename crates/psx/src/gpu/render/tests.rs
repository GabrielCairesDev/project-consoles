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
