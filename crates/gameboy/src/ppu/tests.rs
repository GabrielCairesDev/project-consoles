use super::*;

const WHITE: u32 = SHADES[0];
const BLACK: u32 = SHADES[3];

/// PPU com o tile 1 todo na cor 3 e o tile 2 com a metade esquerda na cor
/// 3; paletas identidade; LCD ligado com fundo, sprites e dados em 8000.
fn setup() -> Ppu {
    let mut ppu = Ppu::default();
    for row in 0..8 {
        ppu.vram[16 + row * 2] = 0xFF;
        ppu.vram[16 + row * 2 + 1] = 0xFF;
        ppu.vram[32 + row * 2] = 0xF0;
        ppu.vram[32 + row * 2 + 1] = 0xF0;
    }
    ppu.write(0xFF47, 0xE4);
    ppu.write(0xFF48, 0xE4);
    ppu.write(0xFF49, 0xE4);
    ppu.write(0xFF40, 0x00);
    ppu.write(0xFF40, 0x93);
    ppu
}

/// Roda até o próximo V-blank e devolve o quadro.
fn frame(ppu: &mut Ppu) -> Vec<u32> {
    for _ in 0..(154 * 456 / 4) * 2 {
        if ppu.tick().vblank {
            return ppu.frame().to_vec();
        }
    }
    panic!("sem V-blank");
}

fn pixel(frame: &[u32], x: usize, y: usize) -> u32 {
    frame[y * WIDTH + x]
}

fn sprite(ppu: &mut Ppu, index: usize, y: u8, x: u8, tile: u8, attrs: u8) {
    ppu.oam[index * 4..index * 4 + 4].copy_from_slice(&[y, x, tile, attrs]);
}

#[test]
fn background_tile_with_horizontal_scroll() {
    let mut ppu = setup();
    ppu.vram[0x1800] = 1; // mapa 9800, tile (0, 0)
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 7, 7), BLACK);
    assert_eq!(pixel(&f, 8, 0), WHITE);

    ppu.write(0xFF43, 4);
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 3, 0), BLACK);
    assert_eq!(pixel(&f, 4, 0), WHITE);
}

#[test]
fn signed_tile_data_mode_uses_9000() {
    let mut ppu = setup();
    // Tile 1 em 0x9010 (modo 8800 com índice 1).
    for i in 0..16 {
        ppu.vram[0x1010 + i] = 0xFF;
    }
    ppu.vram[0x1800] = 1;
    ppu.write(0xFF40, 0x83);
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 0, 0), BLACK);
}

#[test]
fn palette_maps_colors() {
    let mut ppu = setup();
    ppu.vram[0x1800] = 1;
    ppu.write(0xFF47, 0x00); // tudo vira cor 0
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 0, 0), WHITE);
}

#[test]
fn window_covers_from_wx_and_uses_its_own_line_counter() {
    let mut ppu = setup();
    ppu.vram[0x1C00] = 1; // mapa 9C00, linha 0 da janela
    ppu.write(0xFF4A, 10);
    ppu.write(0xFF4B, 7 + 80);
    ppu.write(0xFF40, 0x93 | 0x20 | 0x40);
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 80, 10), BLACK, "início da janela");
    assert_eq!(pixel(&f, 79, 10), WHITE);
    assert_eq!(pixel(&f, 80, 9), WHITE, "acima do WY");
    assert_eq!(pixel(&f, 80, 17), BLACK, "8 linhas de janela");
    assert_eq!(pixel(&f, 80, 18), WHITE);
}

#[test]
fn sprites_draw_with_flip_and_lower_x_wins() {
    let mut ppu = setup();
    // Tile 2 tem a metade esquerda escura; com X invertido, a direita.
    sprite(&mut ppu, 0, 16, 8, 2, 0x20);
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 0, 0), WHITE);
    assert_eq!(pixel(&f, 4, 0), BLACK);

    // Dois sprites sobrepostos: o de menor X fica por cima.
    let mut ppu = setup();
    ppu.write(0xFF48, 0x40); // OBP0: cor 3 → tom 1
    sprite(&mut ppu, 0, 16, 12, 1, 0x10); // OBP1 (cor 3 → preto)
    sprite(&mut ppu, 1, 16, 10, 1, 0x00); // OBP0, X menor
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 4, 0), SHADES[1]);
}

#[test]
fn sprite_behind_background_only_over_color_zero() {
    let mut ppu = setup();
    ppu.vram[0x1800] = 2; // fundo: metade esquerda escura
    sprite(&mut ppu, 0, 16, 8, 1, 0x80);
    ppu.write(0xFF48, 0x40);
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 0, 0), BLACK, "fundo não-zero fica na frente");
    assert_eq!(pixel(&f, 6, 0), SHADES[1], "fundo zero deixa o sprite");
}

#[test]
fn only_ten_sprites_per_line() {
    let mut ppu = setup();
    for i in 0..12 {
        sprite(&mut ppu, i, 16, 8 + i as u8 * 8, 1, 0);
    }
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 9 * 8, 0), BLACK);
    assert_eq!(pixel(&f, 10 * 8, 0), WHITE, "o 11º sprite não aparece");
}

#[test]
fn tall_sprites_ignore_low_tile_bit() {
    let mut ppu = setup();
    ppu.write(0xFF40, 0x93 | 0x04);
    sprite(&mut ppu, 0, 16, 8, 3, 0); // 3 & FE = 2 em cima, 3 embaixo
    let f = frame(&mut ppu);
    assert_eq!(pixel(&f, 0, 0), BLACK, "tile 2");
    assert_eq!(pixel(&f, 4, 0), WHITE);
    assert_eq!(pixel(&f, 0, 8), WHITE, "tile 3 vazio");
}

#[test]
fn lyc_match_raises_stat_interrupt_once() {
    let mut ppu = setup();
    ppu.write(0xFF45, 5);
    ppu.write(0xFF41, 0x40);
    let mut hits = Vec::new();
    for _ in 0..154 * 456 / 4 {
        if ppu.tick().stat {
            hits.push(ppu.read(0xFF44));
        }
    }
    assert_eq!(hits, vec![5]);
}

#[test]
fn draw_length_grows_with_scx_and_sprites() {
    let mut ppu = setup();
    assert_eq!(ppu.draw_length(), 172);
    ppu.write(0xFF43, 3);
    assert_eq!(ppu.draw_length(), 175);
    ppu.write(0xFF43, 0);
    sprite(&mut ppu, 0, 16, 0, 1, 0);
    sprite(&mut ppu, 1, 16, 0, 1, 0);
    // O primeiro na coluna paga 6 + 5; o segundo, só 6.
    assert_eq!(ppu.draw_length(), 172 + 11 + 6);
    // Fora da tela à direita não atrasa.
    sprite(&mut ppu, 2, 16, 170, 1, 0);
    assert_eq!(ppu.draw_length(), 172 + 11 + 6);
}

#[test]
fn lcd_off_blanks_and_resets_line() {
    let mut ppu = setup();
    ppu.vram[0x1800] = 1;
    frame(&mut ppu);
    for _ in 0..1000 {
        ppu.tick();
    }
    ppu.write(0xFF40, 0x13);
    assert_eq!((ppu.read(0xFF44), ppu.read(0xFF41) & 3), (0, 0));
    assert!(ppu.frame().iter().all(|&p| p == WHITE));
    assert!(ppu.vram_accessible() && ppu.oam_accessible());
}
