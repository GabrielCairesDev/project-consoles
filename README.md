# Project Consoles

Launcher de jogos clássicos para macOS, Windows e Linux. Cada console roda num
núcleo próprio escrito em Rust; a interface é Tauri com React.

## Estado atual

| Console | Joga | Saves do jogo | Continuar de onde parou | Observação |
| --- | --- | --- | --- | --- |
| Master System | sim | sim | sim | |
| Mega Drive | sim | sim | sim | |
| Game Boy | sim | sim | sim | |
| PlayStation | sim | sim (cartão de memória) | ainda não | precisa do BIOS do usuário |

O catálogo mostra 25 consoles, de 1985 (Master System) a 2017 (Switch),
numa grade ordenada pelo ano de lançamento. A biblioteca já reconhece os jogos de SNES, Nintendo 64, Game
Gear, Game Boy Color, Game Boy Advance, Nintendo DS e Nintendo 3DS, mas esses
consoles ainda não abrem jogos.

## Funcionalidades

- **Biblioteca:** uma pasta, com subpastas, guarda os jogos de todos os
  consoles.
  - Cartuchos são reconhecidos pelo CRC nos DATs No-Intro.
  - Discos de PlayStation (`.cue` + `.bin`) são reconhecidos pelo número de
    série no DAT Redump.
- **Capas:** vêm do acervo libretro-thumbnails, com cache em disco. Um jogo de
  Nintendo DS sem capa usa o ícone embutido na ROM.
- **Saves:** cada save fica em `<dados do app>/saves/<console>/`, no formato
  bruto do hardware. Dá para exportar e importar.
- **Melhoria de imagem por console:**
  - presets Original e Recomendado;
  - paleta, escala, CRT/LCD, sinal composto, gama, proporção e corte de
    borda;
  - no PS1, também resolução interna até 4×, cor de 24 bits, PGXP e
    filtragem de textura.

  No jogo, F1 compara a imagem Original com a configuração atual.
- **Controles:** teclas configuráveis por console.

## Requisitos

- Rust 1.95 ou mais novo.
- Node.js com npm.
- Os [pré-requisitos do Tauri 2](https://v2.tauri.app/start/prerequisites/)
  de cada sistema.
- Para PlayStation: um BIOS original (por exemplo SCPH1001), escolhido nas
  configurações do console. O app não distribui BIOS.

## Como rodar

```bash
npm install
```

```bash
npm run tauri dev -- --release
```

## Verificações

```bash
cargo fmt --all -- --check
```

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

```bash
cargo test --workspace --all-features
```

```bash
npm test
```

```bash
npm run typecheck
```

Os testes do PS1 que precisam de BIOS ou de um jogo estão marcados como
`#[ignore]`. Para rodá-los, defina `PSX_TESTS_DIR` e `PSX_DISC`; o cabeçalho de
`crates/psx/tests/test_programs.rs` explica o que cada variável aponta.

## Estrutura

| Caminho | Conteúdo |
| --- | --- |
| `crates/sms` | núcleo do Master System |
| `crates/genesis` | núcleo do Mega Drive |
| `crates/gameboy` | núcleo do Game Boy |
| `crates/psx` | núcleo do PlayStation |
| `crates/chips` | chips compartilhados (Z80, PSG) |
| `crates/player` | janela do jogo: vídeo, áudio, controles e melhoria de imagem |
| `src-tauri` | aplicativo: biblioteca, capas, saves, BIOS e configurações |
| `src` | interface React |
| `docs` | regras de UI/UX, design system e melhoria de imagem |

## Licenças de dados

Os DATs No-Intro e Redump vêm do libretro-database e estão sob CC-BY-SA 4.0
(veja `src-tauri/data/dats/SOURCES.md`). As fontes dos logos dos consoles
estão em `src/assets/consoles/SOURCES.md`.
