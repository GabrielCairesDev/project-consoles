# UI/UX Engineering — Regras Permanentes do Projeto

## 1. Objetivo
Desenvolver um launcher multiplataforma de jogos clássicos, que roda os jogos com núcleos próprios em Rust (um por console), com aparência profissional, identidade visual própria, excelente usabilidade e desempenho consistente em macOS, Windows e Linux. A interface deve transmitir a qualidade de um produto comercial moderno, com referências em launchers de jogos, bibliotecas digitais e aplicativos desktop premium.

Não produza interfaces genéricas, com aparência de protótipo, painéis excessivamente simples ou componentes sem identidade visual.

## 2. Princípios de design
- Interface visualmente refinada, coerente e funcional.
- Desenvolva um design system antes de implementar múltiplas telas.
- Hierarquia visual clara, espaçamento consistente e tipografia legível.
- Paleta intencional, com cores semânticas para estados e ações.
- Ícones consistentes, sem misturar estilos incompatíveis. Nunca emojis no lugar de ícones.
- Evite excesso de bordas, sombras, gradientes, efeitos de vidro e elementos decorativos.
- Evite aparência de painel administrativo genérico.
- Imagens de jogos sem distorções ou cortes inadequados.
- A interface deve funcionar em janelas pequenas e grandes.

## 3. Identidade visual do launcher
A interface deve contemplar, conforme o escopo real do produto (não crie funcionalidades fictícias só para preencher a tela):
- Biblioteca de jogos com capas e metadados.
- Filtros por console, plataforma, gênero e estado de instalação, quando aplicável.
- Pesquisa rápida com feedback imediato.
- Navegação lateral ou outra estrutura consistente.
- Página de detalhes do jogo com informações e ações relevantes.
- Configurações gerais, gráficas, de áudio e de controles.
- Estados de carregamento, sem resultados, erro e operação em andamento.
- Indicadores claros do estado de cada jogo.
- Feedback visual para ação, seleção, foco, hover e desativado.

### Configurações de imagem (por console)
Regras de conteúdo em `docs/melhoria-de-imagem.md`.
- No topo, presets (Original, Recomendado e outros do console). O preset atual aparece marcado, e vira "Personalizado" quando o usuário muda uma opção.
- Abaixo, as opções avançadas agrupadas por etapa (Imagem 3D, Cor, Escala, Tela, Ajuste), visíveis sem esconder nada do usuário.
- Mostre só as opções que se aplicam ao console. Não exiba controles desativados sem motivo.
- Cada opção tem nome em linguagem simples e uma frase dizendo o que muda na imagem. Opções pesadas mostram o aviso de desempenho antes de ligar.
- A ação "Restaurar recomendado" fica sempre visível e não apaga nada além das opções de imagem.
- Mudanças são salvas na hora e valem ao abrir o próximo jogo. Durante o jogo, o atalho de comparação Original/atual é informado na tela de configurações.

## 4. Design system obrigatório
Centralize decisões visuais em tokens e componentes reutilizáveis. Defina: cores (fundo, superfícies, texto, bordas, estados), escala tipográfica e pesos, escala de espaçamento, raios, elevação/sombras, dimensões e comportamento de ícones, animações e durações, estados interativos, regras de layout e breakpoints.

Evite valores visuais arbitrários repetidos no código. Toda mudança global de identidade deve ser possível sem editar cada tela.

## 5. Componentes e arquitetura visual
- Componentes reutilizáveis com responsabilidades claras.
- Separe apresentação, estado da interface, lógica de aplicação e execução da emulação.
- Não misture lógica de negócio com componentes visuais.
- A UI não acessa detalhes internos do núcleo de emulação; use interfaces bem definidas para falar com os serviços da aplicação.
- Evite componentes gigantes, duplicação e abstrações desnecessárias.
- Centralize estilos, ícones, temas e padrões de interação.
- Prefira componentes simples e composáveis.

## 6. Performance da interface
- Nunca bloqueie a thread de interface com operações demoradas.
- Nada de disco, varredura de biblioteca ou processamento pesado de capas no caminho de renderização.
- Execução do emulador separada da renderização e das operações de interface.
- Use operações assíncronas ou workers quando apropriado.
- Evite reconstruções, cálculos e alocações desnecessárias.
- Capas sob demanda, com cache; não decodifique a mesma imagem repetidamente.
- Cancele ou descarte resultados obsoletos de pesquisas e operações assíncronas.
- Mantenha a responsividade na inicialização, busca e carregamento da biblioteca.
- Meça antes de introduzir otimizações complexas.

A interface deve continuar responsiva enquanto o núcleo do emulador executa operações intensivas.

## 7. Acessibilidade e interação
- Navegação por teclado e foco visual claramente identificável.
- Nomes acessíveis para controles e ícones.
- Contraste adequado; não dependa só de cor para transmitir informação.
- Respeite redução de movimento; evite animações excessivas ou que atrasem ações.
- Controles funcionam com mouse e teclado; considere gamepad onde fizer sentido.

## 8. Multiplataforma
- Comportamento consistente em macOS, Windows e Linux.
- Não presuma atalhos, menus, fontes, caminhos e decorações de janela idênticos; isole particularidades de plataforma.
- Respeite convenções de cada sistema quando melhorar a experiência.
- Teste escala de tela, densidades de pixel e redimensionamento.
- Não simule elementos nativos sem justificativa de design.
- Uma correção visual em uma plataforma não pode prejudicar as demais.

## 9. Testes e revisão visual
- Teste estados inicial, carregamento, sucesso, erro e ausência de dados.
- Teste navegação, filtros, pesquisa e ações principais.
- Teste diferentes dimensões de janela e verifique regressões visuais.
- Use testes automatizados de interação quando o framework suportar; screenshots de referência quando houver infraestrutura.
- Verifique acessibilidade e navegação por teclado.
- Valide nas três plataformas, sem afirmar testes que não foram executados.

## 10. Protocolo antes de alterar a interface
1. Inspecione a implementação atual e o design system existente.
2. Identifique componentes e fluxos afetados.
3. Verifique os padrões recomendados pelo framework.
4. Planeje a solução visual antes de mexer no código.
5. Reutilize componentes existentes.
6. Implemente preservando arquitetura, desempenho e comportamento.
7. Execute os testes pertinentes.
8. Revise a interface em diferentes estados e dimensões.
9. Informe o que foi alterado, validado e o que ficou pendente.

Não reescreva a interface inteira para corrigir um componente isolado.

## 11. Critérios de conclusão
Concluída somente quando: atende aos requisitos funcionais; respeita o design system; tem hierarquia e consistência; funciona nos estados relevantes; não bloqueia a interface; preserva a separação entre UI e núcleo de emulação; passa nas verificações disponíveis; não tem regressões conhecidas sem justificativa.

**Princípio central:** a interface deve parecer projetada como um produto completo, não montada a partir de componentes aleatórios. A qualidade visual nunca compromete a correção, a responsividade ou a performance do emulador.

## Ferramentas
- Skill de UI/UX: `.claude/skills/ui-ux-pro-max` (UI UX Pro Max, licença MIT). Use-a para decisões de design system, estilo, cor, tipografia e UX. Rode o script de busca a partir da raiz do projeto: `python3 .claude/skills/ui-ux-pro-max/scripts/search.py "<consulta>" --design-system`.
- Framework de interface: Tauri (núcleo em Rust, front-end web).
