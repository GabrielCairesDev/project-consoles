//! Teste contra o acervo real (requer internet). Ignorado por padrão:
//! `cargo test -- --ignored` para rodar.

use project_consoles_lib::covers::{BoxartSource, Fetched, LibretroThumbnails};

#[test]
#[ignore = "acessa a internet"]
fn downloads_a_known_boxart_and_reports_missing_ones() {
    let source = LibretroThumbnails::default();
    let system = "Nintendo - Super Nintendo Entertainment System";

    match source.fetch(system, "Super Mario World (USA)") {
        Fetched::Found(bytes) => assert!(bytes.len() > 1000, "PNG suspeito de tão pequeno"),
        other => panic!("esperava capa, veio {other:?}"),
    }
    assert!(matches!(
        source.fetch(system, "Jogo Que Nao Existe (Brazil)"),
        Fetched::NotFound
    ));
}
