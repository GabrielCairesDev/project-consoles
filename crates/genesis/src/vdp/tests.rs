use super::*;

fn set_reg(vdp: &mut Vdp, reg: u8, value: u8) {
    vdp.write_control(0x8000 | u16::from(reg) << 8 | u16::from(value));
}

/// Comando de duas palavras: `code` de 6 bits e endereço de 16 bits.
fn command(vdp: &mut Vdp, code: u8, addr: u16) {
    let code = u16::from(code);
    vdp.write_control(((code & 3) << 14) | (addr & 0x3FFF));
    vdp.write_control(((code & 0x3C) << 2) | (addr >> 14));
}

const VRAM_WRITE: u8 = 0x01;
const CRAM_WRITE: u8 = 0x03;
const VSRAM_WRITE: u8 = 0x05;
const VRAM_READ: u8 = 0x00;

fn write_vram(vdp: &mut Vdp, addr: u16, words: &[u16]) {
    command(vdp, VRAM_WRITE, addr);
    for &w in words {
        vdp.write_data(w);
    }
}

fn write_cram(vdp: &mut Vdp, index: u16, color: u16) {
    command(vdp, CRAM_WRITE, index * 2);
    vdp.write_data(color);
}

/// Modo 5, H40, tela ligada, auto-incremento 2, planos 32×32: A em
/// 0xC000, B em 0xE000, sprites em 0xF000 (sem sprites), hscroll em 0xFC00.
fn display_on() -> Vdp {
    let mut vdp = Vdp::new();
    for (reg, value) in [
        (0, 0x04),
        (1, 0x44),
        (2, 0x30),
        (4, 0x07),
        (5, 0x78),
        (12, 0x81),
        (13, 0x3F),
        (15, 0x02),
        (16, 0x00),
    ] {
        set_reg(&mut vdp, reg, value);
    }
    vdp
}

fn render(vdp: &mut Vdp, line: u16) -> Vec<u32> {
    vdp.begin_line(line);
    vdp.render_line();
    let start = usize::from(line) * MAX_WIDTH;
    vdp.framebuffer()[start..start + vdp.width()].to_vec()
}

/// Tile `tile` com a primeira linha toda na cor `color` (0–15).
fn solid_row_tile(vdp: &mut Vdp, tile: u16, color: u16) {
    let nibbles = color * 0x1111;
    write_vram(vdp, tile * 32, &[nibbles, nibbles]);
}

#[test]
fn registers_and_two_word_commands() {
    let mut vdp = Vdp::new();
    set_reg(&mut vdp, 15, 2);
    assert_eq!(vdp.regs[15], 2);
    write_vram(&mut vdp, 0x1234, &[0xABCD, 0x5678]);
    assert_eq!(&vdp.vram[0x1234..0x1238], &[0xAB, 0xCD, 0x56, 0x78]);
    command(&mut vdp, VRAM_READ, 0x1234);
    assert_eq!(vdp.read_data(), 0xABCD);
    assert_eq!(vdp.read_data(), 0x5678);
}

#[test]
fn odd_vram_address_swaps_bytes() {
    let mut vdp = Vdp::new();
    set_reg(&mut vdp, 15, 2);
    write_vram(&mut vdp, 0x0101, &[0x1122]);
    assert_eq!(vdp.vram[0x0100], 0x22);
    assert_eq!(vdp.vram[0x0101], 0x11);
}

#[test]
fn cram_and_vsram_masks() {
    let mut vdp = Vdp::new();
    set_reg(&mut vdp, 15, 2);
    write_cram(&mut vdp, 3, 0xFFFF);
    assert_eq!(vdp.cram[3], 0x0EEE);
    command(&mut vdp, VSRAM_WRITE, 2);
    vdp.write_data(0xFFFF);
    assert_eq!(vdp.vsram[1], 0x03FF);
}

#[test]
fn dma_fill_writes_high_byte() {
    let mut vdp = Vdp::new();
    set_reg(&mut vdp, 1, 0x14); // DMA ligado
    set_reg(&mut vdp, 15, 1);
    set_reg(&mut vdp, 19, 4);
    set_reg(&mut vdp, 20, 0);
    set_reg(&mut vdp, 23, 0x80);
    command(&mut vdp, VRAM_WRITE | 0x20, 0x2000);
    vdp.write_data(0x55AA);
    // A primeira escrita é normal (55 AA); depois, 4 bytes com o byte alto
    // em endereço ^ 1: 2000, 2003, 2002 e 2005.
    assert_eq!(
        &vdp.vram[0x2000..0x2006],
        &[0x55, 0xAA, 0x55, 0x55, 0x00, 0x55]
    );
}

#[test]
fn dma_copy_moves_vram_bytes() {
    let mut vdp = Vdp::new();
    set_reg(&mut vdp, 15, 2);
    write_vram(&mut vdp, 0x1000, &[0x0102, 0x0304]);
    set_reg(&mut vdp, 1, 0x14);
    set_reg(&mut vdp, 15, 1);
    set_reg(&mut vdp, 19, 4);
    set_reg(&mut vdp, 20, 0);
    set_reg(&mut vdp, 21, 0x00);
    set_reg(&mut vdp, 22, 0x10);
    set_reg(&mut vdp, 23, 0xC0);
    command(&mut vdp, 0x30, 0x3000);
    assert_eq!(&vdp.vram[0x3000..0x3004], &[0x02, 0x01, 0x04, 0x03]);
}

#[test]
fn dma_from_68000_is_requested_to_the_bus() {
    let mut vdp = Vdp::new();
    set_reg(&mut vdp, 1, 0x14);
    set_reg(&mut vdp, 15, 2);
    set_reg(&mut vdp, 19, 0x10);
    set_reg(&mut vdp, 20, 0);
    set_reg(&mut vdp, 21, 0x00);
    set_reg(&mut vdp, 22, 0x80);
    set_reg(&mut vdp, 23, 0x7F);
    command(&mut vdp, VRAM_WRITE | 0x20, 0);
    assert_eq!(
        vdp.take_dma_request(),
        Some(DmaRequest {
            source: 0xFF_0000,
            words: 0x10
        })
    );
    assert_eq!(vdp.take_dma_request(), None);
}

#[test]
fn background_color_when_display_off_or_empty() {
    let mut vdp = display_on();
    set_reg(&mut vdp, 7, 0x05);
    write_cram(&mut vdp, 5, 0x000E); // vermelho máximo
    assert!(render(&mut vdp, 0).iter().all(|&p| p == 0xFF0000));
    set_reg(&mut vdp, 1, 0x04); // tela desligada
    assert!(render(&mut vdp, 0).iter().all(|&p| p == 0xFF0000));
}

#[test]
fn plane_a_tile_uses_its_palette() {
    let mut vdp = display_on();
    solid_row_tile(&mut vdp, 1, 3);
    // Name table de A: célula (0,0) = tile 1, paleta 2.
    write_vram(&mut vdp, 0xC000, &[0x4001]);
    write_cram(&mut vdp, 2 * 16 + 3, 0x00E0); // verde
    let row = render(&mut vdp, 0);
    assert_eq!(&row[..8], &[0x00FF00; 8]);
    assert_eq!(row[8], 0);
}

#[test]
fn horizontal_scroll_moves_plane() {
    let mut vdp = display_on();
    solid_row_tile(&mut vdp, 1, 1);
    write_vram(&mut vdp, 0xC000, &[0x0001]);
    write_vram(&mut vdp, 0xFC00, &[4]); // hscroll do plano A
    write_cram(&mut vdp, 1, 0x0E00); // azul
    let row = render(&mut vdp, 0);
    assert_eq!(row[3], 0);
    assert_eq!(&row[4..12], &[0x0000FF; 8]);
}

#[test]
fn high_priority_plane_b_covers_low_priority_a() {
    let mut vdp = display_on();
    solid_row_tile(&mut vdp, 1, 1);
    solid_row_tile(&mut vdp, 2, 2);
    write_vram(&mut vdp, 0xC000, &[0x0001]); // A, prioridade baixa
    write_vram(&mut vdp, 0xE000, &[0x8002]); // B, prioridade alta
    write_cram(&mut vdp, 1, 0x000E);
    write_cram(&mut vdp, 2, 0x00E0);
    assert_eq!(render(&mut vdp, 0)[0], 0x00FF00);
    // Sem prioridade em B, A fica na frente.
    write_vram(&mut vdp, 0xE000, &[0x0002]);
    assert_eq!(render(&mut vdp, 0)[0], 0xFF0000);
}

#[test]
fn sprite_is_drawn_and_low_priority_goes_under_high_plane() {
    let mut vdp = display_on();
    solid_row_tile(&mut vdp, 4, 6);
    // Sprite 0: Y = 128 (linha 0), 1×1 célula, link 0, tile 4, X = 128 + 16.
    write_vram(&mut vdp, 0xF000, &[128, 0x0000, 0x0004, 128 + 16]);
    write_cram(&mut vdp, 6, 0x0EEE);
    let row = render(&mut vdp, 0);
    assert_eq!(row[15], 0);
    assert_eq!(&row[16..24], &[0xFFFFFF; 8]);

    // Plano A de prioridade alta no mesmo lugar cobre o sprite.
    solid_row_tile(&mut vdp, 1, 1);
    write_vram(&mut vdp, 0xC000 + 2 * 2, &[0x8001]);
    write_cram(&mut vdp, 1, 0x000E);
    assert_eq!(render(&mut vdp, 0)[16], 0xFF0000);
}

#[test]
fn sprite_link_list_and_per_line_overflow() {
    let mut vdp = display_on();
    // 21 sprites 1×1 na linha 0, encadeados.
    let mut table = Vec::new();
    for i in 0..21u16 {
        table.extend([128, (i + 1) % 21, 0, 128 + i * 8]);
    }
    write_vram(&mut vdp, 0xF000, &table);
    render(&mut vdp, 0);
    assert_ne!(vdp.read_status() & STATUS_OVERFLOW, 0);
}

#[test]
fn window_replaces_plane_a_on_top_rows() {
    let mut vdp = display_on();
    solid_row_tile(&mut vdp, 1, 1);
    set_reg(&mut vdp, 3, 0x2C); // janela em 0xB000 (H40)
    set_reg(&mut vdp, 18, 0x01); // 8 primeiras linhas
    write_vram(&mut vdp, 0xB000, &[0x0001]);
    write_cram(&mut vdp, 1, 0x000E);
    assert_eq!(render(&mut vdp, 0)[0], 0xFF0000);
    // Na linha 8 já não é janela: plano A vazio.
    assert_eq!(render(&mut vdp, 8)[0], 0);
}

#[test]
fn h32_and_h40_widths() {
    let mut vdp = display_on();
    vdp.begin_line(0);
    assert_eq!(vdp.frame_size(), (320, 224));
    set_reg(&mut vdp, 12, 0x00);
    vdp.begin_line(0);
    assert_eq!(vdp.frame_size(), (256, 224));
}

#[test]
fn hint_counter_fires_every_n_plus_one_lines() {
    let mut vdp = display_on();
    set_reg(&mut vdp, 0, 0x14); // IE1
    set_reg(&mut vdp, 10, 2);
    vdp.begin_line(230); // recarrega fora da área ativa
    let mut fired = Vec::new();
    for line in 0..10 {
        vdp.begin_line(line);
        if vdp.interrupt_level() == 4 {
            fired.push(line);
            vdp.acknowledge(4);
        }
    }
    assert_eq!(fired, vec![2, 5, 8]);
}

#[test]
fn vint_at_line_224_has_priority_and_is_acknowledged() {
    let mut vdp = display_on();
    set_reg(&mut vdp, 1, 0x64); // IE0
    vdp.begin_line(223);
    vdp.render_line();
    assert_eq!(vdp.interrupt_level(), 0);
    vdp.begin_line(224);
    vdp.render_line();
    assert_eq!(vdp.interrupt_level(), 6);
    assert_ne!(vdp.read_status() & STATUS_VINT, 0);
    vdp.acknowledge(6);
    assert_eq!(vdp.interrupt_level(), 0);
}

#[test]
fn status_reports_vblank_and_resets_command_latch() {
    let mut vdp = display_on();
    vdp.begin_line(10);
    assert_eq!(vdp.read_status() & STATUS_VBLANK, 0);
    vdp.begin_line(230);
    assert_ne!(vdp.read_status() & STATUS_VBLANK, 0);
    vdp.write_control(0x4000);
    vdp.read_status();
    assert!(!vdp.command_pending);
}

#[test]
fn v_counter_skips_after_ea() {
    let mut vdp = display_on();
    vdp.begin_line(0xEA);
    assert_eq!(vdp.hv_counter(0.0) >> 8, 0xEA);
    vdp.begin_line(0xEB);
    assert_eq!(vdp.hv_counter(0.0) >> 8, 0xE5);
}

#[test]
fn state_round_trips() {
    let mut vdp = display_on();
    write_vram(&mut vdp, 0x100, &[0xBEEF]);
    write_cram(&mut vdp, 9, 0x0246);
    let mut w = Writer::default();
    vdp.save_state(&mut w);
    let mut restored = Vdp::new();
    let mut r = Reader::new(&w.bytes);
    restored.load_state(&mut r).expect("estado válido");
    assert_eq!(r.finish(), Ok(()));
    assert_eq!(restored.vram[..], vdp.vram[..]);
    assert_eq!(restored.cram, vdp.cram);
    assert_eq!(restored.regs, vdp.regs);
}
