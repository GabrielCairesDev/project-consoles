# Design system — project-consoles

Fonte de verdade dos valores: [`src/styles/tokens.css`](../src/styles/tokens.css). Este documento registra as decisões; os valores ficam só no CSS.

## Origem
Gerado com a skill UI UX Pro Max (`--design-system "gaming emulator launcher dark immersive" --density 5 --motion 3`) e ajustado às regras de [ui-ux.md](ui-ux.md):

- **Adotado:** paleta (roxo `#7C3AED` como primária, rosa `#F43F5E` como destaque, fundo azul-noite) e tipografia (Russo One para títulos, Chakra Petch para texto).
- **Estilo:** "Dark Mode (OLED)" da skill (custo de performance baixo, risco de acessibilidade baixo), em vez do "3D & Hyperrealism" sugerido, que pede WebGL, parallax e animações pesadas. Isso contraria as regras de performance e de evitar efeitos decorativos.
- **Descartado:** o padrão de landing page ("Feature-Rich Showcase") e as animações GSAP de scroll; o produto é um app desktop, não uma página de marketing.

## Decisões
- **Tema:** apenas escuro por enquanto (`color-scheme: dark`). Tema claro só quando houver requisito.
- **Fontes:** empacotadas localmente via `@fontsource`. O app funciona offline e a CSP bloqueia origens externas.
- **Ícones:** Lucide (`lucide-react`), traço 1.75. Nunca emojis. Ícones decorativos ficam com `aria-hidden`.
- **Movimento:** durações curtas (120/200/320 ms), zeradas com `prefers-reduced-motion`.
- **Foco:** contorno de 2px com `--color-focus-ring` em todo `:focus-visible`.
- **Contraste:** texto principal `#E2E8F0` e secundário `#94A3B8` sobre `#0B0B14` passam de 4.5:1; texto sobre a primária usa branco.

## Estrutura do front-end
- `src/styles/`: tokens e estilos globais.
- `src/features/<feature>/`: componentes de uma área do produto, com CSS Modules e testes ao lado.
- `src-tauri/`: núcleo em Rust. A UI conversa com ele apenas por comandos Tauri definidos explicitamente.
