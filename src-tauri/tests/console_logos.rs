//! Integridade entre o catálogo (núcleo) e os logos (interface).
//!
//! A interface localiza o logo de cada console por `src/assets/consoles/<id>.svg`.
//! Este teste impede que um console seja adicionado sem logo, ou que sobre um
//! logo sem console correspondente.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use project_consoles_lib::catalog::consoles;

fn logos_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/assets/consoles")
}

fn logo_ids() -> BTreeSet<String> {
    fs::read_dir(logos_dir())
        .expect("pasta de logos deve existir")
        .map(|entry| entry.expect("entrada de diretório legível").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "svg"))
        .map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .expect("nome de arquivo UTF-8")
                .to_owned()
        })
        .collect()
}

#[test]
fn every_console_has_exactly_one_logo() {
    let catalog_ids: BTreeSet<String> = consoles().iter().map(|c| c.id.to_owned()).collect();
    let logos = logo_ids();

    let missing: Vec<_> = catalog_ids.difference(&logos).collect();
    let orphan: Vec<_> = logos.difference(&catalog_ids).collect();

    assert!(missing.is_empty(), "consoles sem logo: {missing:?}");
    assert!(orphan.is_empty(), "logos sem console: {orphan:?}");
}

#[test]
fn logos_are_scalable_and_self_contained() {
    for id in logo_ids() {
        let path = logos_dir().join(format!("{id}.svg"));
        let svg = fs::read_to_string(&path).expect("logo legível como UTF-8");
        let root = svg
            .find("<svg")
            .and_then(|start| svg[start..].find('>').map(|end| &svg[start..start + end]))
            .unwrap_or_else(|| panic!("{id}: elemento <svg> ausente"));

        assert!(root.contains("viewBox"), "{id}: sem viewBox, não escala");
        let lower = svg.to_ascii_lowercase();
        for forbidden in ["<script", "javascript:", "<foreignobject", "href=\"http"] {
            assert!(!lower.contains(forbidden), "{id}: contém {forbidden:?}");
        }
    }
}
