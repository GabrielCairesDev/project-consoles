# Melhoria de imagem — catálogo e regras

Referência obrigatória para a regra §13 do `CLAUDE.md`. Vale para os consoles atuais e para todo console novo.

## 1. Princípio: quem decide é o usuário
- Cada console oferece **todas** as técnicas do catálogo que fazem sentido para ele (matriz da seção 5), não só uma.
- Cada técnica pode ser ligada, desligada e ajustada separadamente, por console.
- O padrão é o preset **Recomendado** do console. O preset **Original** (pixel a pixel, sem nenhuma etapa) está sempre disponível.
- Presets só preenchem as opções. Se o usuário mudar uma opção, o preset passa a **Personalizado** e nada do que ele escolheu é sobrescrito sem ele pedir.
- "Restaurar recomendado" volta o console ao padrão.
- Durante o jogo, um atalho alterna entre Original e a configuração atual, para comparar.

## 2. Pipeline em etapas (ordem fixa)
| # | Etapa | Onde roda | O que pode mudar |
|---|-------|-----------|------------------|
| 1 | Núcleo | dentro do núcleo do console | como o quadro é desenhado (resolução interna, precisão) |
| 2 | Cor | player | cores do quadro (paleta, correção de tela) |
| 3 | Escala | player | ampliação do quadro |
| 4 | Tela | player | simulação de CRT/LCD/sinal |
| 5 | Ajuste | player | proporção, corte de borda, encaixe na janela |

- As etapas 2–5 são só apresentação: não tocam na emulação, no tempo do jogo, nos saves nem no estado salvo.
- A etapa 1 muda o desenho do quadro, mas nunca o tempo do jogo, os saves ou a lógica. O estado salvo carrega igual com qualquer valor dela.
- Cada etapa desligada é identidade: o quadro passa sem nenhuma alteração.
- A UI fala com o player por configurações tipadas. Ela não conhece o algoritmo.

## 3. Catálogo de técnicas

### Etapa 1 — Núcleo (consoles 3D)
| Técnica | Para que serve | Observação |
|---------|----------------|------------|
| Resolução interna (1×, 2×, 3×, 4×…) | 3D nítido | a de maior impacto; o custo cresce com o quadrado do fator |
| Precisão de geometria (PGXP, PS1) | acaba com os polígonos tremendo | também corrige a perspectiva das texturas |
| Cor de 24 bits / sem dithering (PS1) | remove a trama de 15 bits | |
| Filtragem de textura (bilinear, 3 pontos, xBR) | texturas sem serrilhado | |
| Downsampling adaptativo | telas 2D voltam ao original, o 3D fica em alta | evita menus borrados ou desalinhados |

### Etapa 2 — Cor
| Técnica | Para que serve |
|---------|----------------|
| Paleta (portáteis monocromáticos) | verde original, cinza ou personalizada |
| Correção de cor de LCD | reproduz a tela real (mais escura e dessaturada) |
| Gama / brilho | ajuste fino |

### Etapa 3 — Escala
| Técnica | Para que serve | Tipo |
|---------|----------------|------|
| Vizinho mais próximo, escala inteira | pixel perfeito | todos |
| Sharp-bilinear | escala não inteira sem tremer nem borrar | todos |
| Scale2x/3x (EPX) | contornos arredondados, sem criar cores | 2D |
| MMPX (2×, 4× em duas passadas) | curvas, diagonais e cantos vivos, preservando detalhes de um pixel e as cores (licença MIT) | 2D |
| xBRZ (2–6×) | contornos suaves e limpos; **GPLv3**, não usar sem decisão de licença | 2D |
| ScaleFX | suave, combina com scanlines | 2D |
| Bilinear | suavização simples | 3D |

### Etapa 4 — Tela
| Técnica | Para que serve |
|---------|----------------|
| Scanlines | linhas do CRT (a partir de 2× na vertical) |
| Máscara CRT (abertura) | aparência de tubo; no app, junto com as scanlines ("TV de tubo") |
| Sinal composto (simplificado) | recria a transparência por dithering: mistura 1-2-1 na linha. Não é a simulação NTSC completa (a do Blargg é LGPL) |
| Grade de LCD | aparência de tela de portátil (a partir de 3×) |
| Frame blending | recria o rastro do LCD; sem ele, efeitos de piscar viram flicker |

### Etapa 5 — Ajuste
Proporção (original, 4:3, pixels quadrados), escala inteira ou preencher, e corte de borda/overscan.

## 4. Orçamento de desempenho
- Cada técnica é medida em release, num quadro de 2560×1440, e o custo fica registrado na matriz.
- Etapas 2–5 rodam em CPU (`softbuffer`) enquanto couberem no orçamento: **≤ 4 ms por quadro no total**, a 2560×1440.
- Uma técnica que estoure o orçamento não entra no Recomendado e abre a avaliação de migrar a apresentação para GPU (`wgpu`). Essa migração é decisão do usuário e não é feita por conta própria.
- Nenhuma combinação pode derrubar o jogo abaixo da velocidade real do console. Se a combinação escolhida não couber, a UI avisa antes, em vez de deixar o jogo travar.
- Sem alocação por quadro: buffers do player são reutilizados.

## 5. Matriz dos consoles atuais
**R** = ligado no Recomendado · **o** = disponível · **—** = não se aplica.

| Técnica | Master System | Mega Drive | Game Boy (DMG) | PlayStation |
|---------|---------------|------------|----------------|-------------|
| Resolução interna | — | — | — | R (2×) |
| PGXP | — | — | — | o |
| 24 bits / sem dithering | — | — | — | o |
| Filtragem de textura | — | — | — | o |
| Downsampling adaptativo | — | — | — | o |
| Paleta | — | — | R (verde original) | — |
| Correção de cor de LCD | — | — | — (tela monocromática; as paletas cobrem) | — |
| Scale2x/3x | R | R | R | — |
| MMPX | o | o | o | — |
| xBRZ / ScaleFX | — (licença) | — (licença) | — (licença) | — |
| Sharp-bilinear | o | o | o | o |
| Bilinear | — | — | — | R |
| Scanlines / TV de tubo | o | o | — | o |
| Sinal composto | o | o | — | o |
| Grade de LCD | — | — | o | — |
| Frame blending | — | — | o | — |
| Proporção: pixels quadrados | R | R | R | — |
| Proporção: TV 4:3 | o | o | — | R |
| Encaixe: escala inteira | R | R | R | o |
| Encaixe: preencher | o | o | o | R |

Ao implementar ou medir uma técnica, atualize esta matriz (custo e estado).

### 5.1 Estado e custo medido
Benchmark: `cargo test --release -p player bench_ -- --ignored --nocapture` (`video.rs`). Todas as etapas rodam em CPU e cabem no orçamento, então a migração para GPU não foi necessária. Janela de 2560×1440, média de 120 quadros, faixa de 3 execuções num Mac (Apple Silicon). Só as barras fora do quadro são pintadas; o quadro é escrito uma vez pelo filtro.

| Técnica | Estado | Caso medido | ms/quadro |
|---------|--------|-------------|-----------|
| Original (vizinho mais próximo) | implementada | SMS 256×192 | 0,75–0,91 |
| Scale2x/3x (EPX) | implementada | SMS 256×192 | 1,15–1,16 |
| MMPX (4×, duas passadas) | implementada | SMS 256×192 | 2,62–2,63 |
| Sharp-bilinear | implementada | SMS 256×192, 4:3 | 0,65–0,67 |
| Sharp-bilinear | implementada | PS1 320×240, 4:3 | 0,70–0,71 |
| Paleta + EPX | implementada | GB 160×144 | 0,79–0,81 |
| Paleta + sharp-bilinear | implementada | GB 160×144, 10:9 | 0,49–0,50 |
| Bilinear | implementada | PS1 640×480, 4:3 | 1,29–1,30 |
| Proporção e encaixe | implementadas (só posicionam o quadro, custo desprezível) | — | — |
| EPX + scanlines | implementada | SMS 256×192 | 1,38–1,72 |
| Composto + EPX + TV de tubo | implementada | SMS 256×192 | 2,14–2,17 |
| Composto + MMPX + TV de tubo (pior caso) | implementada | SMS 256×192 | 3,43–3,47 |
| Paleta + rastro + EPX + grade do LCD | implementada | GB 160×144 | 1,68–2,01 |
| Composto + bilinear + TV de tubo | implementada | PS1 640×480, 4:3 | 2,61–2,83 |
| Resolução interna 2× | em outro branch (`claude/gifted-gates-6ae501`) | — | — |
| Demais | não implementadas | — | — |

Histórico: antes de pintar só as barras, a janela inteira era limpa e reescrita, e a bilinear do PS1 em 640×480 custava 5,1–5,4 ms (acima do orçamento).

## 6. Configurações salvas
- Ficam por console em `ConsoleSettings.image` (`ImageChoices`), com uma escolha por técnica, gravada pelo id estável do player (`player::image`). Escolha ausente vale o Recomendado. O preset não é gravado: ele é deduzido comparando as escolhas com Original e Recomendado.
- Migração do formato antigo: `enhance: true` vira Recomendado; `enhance: false` vira Original.
- Um valor desconhecido ou inválido não derruba o app. Aquele campo volta ao Recomendado, o problema é registrado em log e o resto da configuração é preservado.
- O launcher passa a configuração ao processo do jogo de forma tipada. O player valida, e um valor recusado é informado, nunca ignorado em silêncio.

## 7. Testes obrigatórios
- Saída de cada técnica em casos conhecidos (diagonal, área lisa, borda do quadro, quadro de 1 pixel).
- Cada etapa desligada é identidade; o preset Original é idêntico ao quadro do núcleo, na apresentação de sempre do console (`ImageSettings::original(system)`).
- Técnicas de escala 2D não inventam cores, exceto onde a técnica mistura por definição (bilinear, xBRZ, NTSC).
- Etapa 1: o estado salvo em uma resolução carrega em outra, e o save do jogo não muda com ela.
- Configurações: ida e volta, migração do booleano antigo, valor inválido tratado e preferência respeitada ao abrir o jogo.
- Benchmark reproduzível de cada técnica do player, com o resultado anotado na matriz.

## 8. Checklist ao adicionar um console
1. Classificar o console (2D, 3D, portátil, saída de vídeo original) e preencher a linha dele na matriz.
2. Implementar as técnicas aplicáveis já existentes no player (reaproveitar, não duplicar).
3. Implementar as técnicas de núcleo do console, se houver.
4. Definir o preset Recomendado, justificado pelo hardware original.
5. Medir o custo e verificar o orçamento.
6. Expor na UI só as opções aplicáveis.
7. Escrever os testes da seção 7.
