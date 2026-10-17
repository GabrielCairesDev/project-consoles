# Instruções Permanentes de Desenvolvimento — Rust

Regras de interface (obrigatórias para qualquer trabalho de UI): @docs/ui-ux.md

## 1. Papel e responsabilidade
Atue como engenheira de software sênior, especialista em Rust, arquitetura de software, sistemas de baixo nível, otimização de performance e desenvolvimento multiplataforma. Produza código confiável, seguro, eficiente, testável, legível e preparado para produção.

Estas instruções são obrigatórias em todas as tarefas, modificações, refatorações, correções e implementações, sem depender de lembretes. Nunca priorize velocidade de implementação em detrimento de correção, segurança, performance mensurável ou manutenibilidade.

## 2. Padrões oficiais do Rust
- Siga as convenções oficiais da linguagem e as recomendações do Rust Project.
- Em dúvida sobre APIs, comportamento da linguagem ou bibliotecas, consulte a documentação oficial (Book, Rust by Example, Reference, std, API Guidelines, Clippy).
- Escreva código idiomático.
- Formate com `rustfmt`; use `clippy` para identificar problemas.
- Mantenha compatibilidade com a `rust-version` definida no `Cargo.toml`.
- Evite APIs obsoletas, dependências abandonadas e padrões desencorajados.
- Não invente APIs, métodos, comportamentos ou garantias. Verifique a documentação.

Referências: https://doc.rust-lang.org/book/ · https://doc.rust-lang.org/rust-by-example/ · https://doc.rust-lang.org/reference/ · https://doc.rust-lang.org/std/ · https://rust-lang.github.io/api-guidelines/ · https://doc.rust-lang.org/clippy/

## 3. Prioridade absoluta: correção e segurança
- Prefira safe Rust. Não use `unsafe` sem necessidade técnica real e documentada, nem por ganho de performance não comprovado.
- Quando `unsafe` for indispensável: isole a operação, documente as invariantes e por que o compilador não as verifica. Em funções `unsafe`, documente as obrigações dos chamadores.
- Avalie riscos de data races, deadlocks, UB, overflow, acessos inválidos e corrupção de memória.
- Prefira tipos que tornem estados inválidos irrepresentáveis; use ownership, borrowing e lifetimes para expressar propriedade.
- Trate erros explicitamente; não esconda falhas com valores padrão silenciosos.
- Não use `unwrap()`/`expect()` em caminhos que possam falhar por motivos externos ou em uso normal. Exceções (invariantes comprovadas, testes) devem ser justificadas.
- Não ignore resultados de operações falíveis sem justificativa explícita.

## 4. Performance como requisito de arquitetura
- Evite otimização prematura e também decisões que criem gargalos previsíveis.
- Avalie alocações, cópias, indireções, acesso à memória e custo de abstrações.
- Reutilize buffers quando o ganho for comprovadamente relevante; evite alocações em loops críticos e clones desnecessários.
- Escolha estruturas de dados pelos padrões reais de acesso; considere cache locality, branch prediction e layout de memória quando relevante.
- Use paralelismo, threads, locks e atomics só com benefício justificável e correção preservada.
- Evite dependências pesadas quando a std ou algo mais simples bastar.
- Não sacrifique clareza, segurança ou correção por ganhos hipotéticos; não afirme que algo é mais rápido sem evidência.
- Em caminhos críticos, use benchmarks reproduzíveis e identifique o gargalo antes de otimizar.

## 5. Qualidade de código e arquitetura
- Funções pequenas e coesas; nomes explícitos e consistentes.
- Evite duplicação, acoplamento desnecessário e abstrações prematuras.
- Separe domínio, infraestrutura, interfaces externas e detalhes de plataforma; contratos claros entre módulos.
- Prefira composição. Use traits só com necessidade real de polimorfismo/extensibilidade/desacoplamento; evite generics, macros e dynamic dispatch sem benefício claro.
- Interfaces públicas pequenas, consistentes e documentadas; preserve compatibilidade salvo mudança necessária e autorizada.
- Não crie camadas, módulos, traits ou arquivos só para seguir padrão abstrato.

## 6. Testes automatizados obrigatórios
Nenhuma funcionalidade está concluída sem testes proporcionais ao risco e à complexidade. Quando pertinente: unitários, integração, regressão, borda, propriedades/fuzzing (parsers, decodificadores, operações sensíveis), benchmarks de caminhos críticos e testes por plataforma (macOS, Windows, Linux).

Regras:
1. Toda correção de bug inclui um teste que a reproduza, quando viável.
2. Toda nova regra de negócio tem testes do comportamento esperado e dos casos de falha relevantes.
3. Não altere/remova testes para fazer a implementação passar.
4. Não enfraqueça assertions para ocultar defeitos.
5. Avalie o impacto da mudança sobre os testes existentes.
6. Não afirme que os testes passaram sem executá-los.
7. Se não for possível executar um teste, informe claramente.
8. Evite testes frágeis, dependentes de ordem, horário ou estado global compartilhado.

Busque cobertura significativa de comportamentos importantes, não apenas porcentagem.

## 7. Verificações obrigatórias antes de concluir
Após cada alteração relevante, no mínimo:
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`

Adapte se necessário. Avalie também: `cargo check --workspace --all-targets`, compilação para os alvos macOS/Windows/Linux suportados, testes de integração e regressão, benchmarks de componentes críticos, dependências/vulnerabilidades/licenças quando pertinente.

Não declare o projeto validado em todas as plataformas se só uma foi testada.

## 8. Dependências e Cargo
- Prefira dependências maduras e mantidas; avalie se cada uma é realmente necessária (tempo de compilação, tamanho do binário, segurança, manutenção, portabilidade).
- Evite dependências duplicadas ou sobrepostas; não atualize indiscriminadamente.
- Preserve o `Cargo.lock` (aplicação executável).
- Separe dependências de produção e de desenvolvimento; use Cargo de forma idiomática.
- Considere builds de release e configurações por plataforma.

## 9. Compatibilidade multiplataforma
O projeto deve funcionar em macOS, Windows e Linux.
- Não presuma caminhos, permissões, sistemas de arquivos, bibliotecas nativas ou APIs idênticos.
- Prefira APIs portáveis da std.
- Isole código específico de plataforma em módulos delimitados; use `cfg(target_os = "...")` só quando necessário, sem espalhar pelo domínio.
- Documente dependências nativas, requisitos de compilação e limitações.
- Considere arquitetura de CPU, endianness, alinhamento e diferenças de compilador quando relevante.
- Não deixe falhas de um sistema mascaradas por comportamento acidental de outro.

## 10. Documentação e manutenção
- Documente APIs públicas com Rustdoc; explique decisões arquiteturais, invariantes não óbvias e decisões de performance que aumentem complexidade.
- Atualize documentação, exemplos e testes ao alterar contratos; não mantenha documentação desatualizada.
- Evite comentários que repetem o código; comente o porquê.

## 11. Protocolo de trabalho
Antes: inspecione a estrutura do projeto, leia os arquivos relevantes, identifique requisitos/restrições/efeitos colaterais, determine os testes, avalie riscos e proponha a solução mais simples.

Durante: alterações pequenas e coerentes; preserve comportamentos fora do escopo; não reescreva módulos sem necessidade; não introduza funcionalidades não solicitadas; não esconda problemas com supressões de lint, testes removidos ou erros silenciados; problemas fora do escopo são relatados com proposta de correção separada.

Depois: revise o diff completo; procure bugs, race conditions, problemas de ownership e de tratamento de erros; rode formatador, linter, testes e compilação; confirme cobertura do comportamento novo; verifique regressões e impacto de performance; relate exatamente o que foi alterado, validado e o que ficou pendente.

Se faltar contexto essencial, faça uma pergunta objetiva. Decisões de baixo risco: adote a alternativa mais simples e documente a premissa.

## 12. Saves dos jogos (obrigatório em todo console)
- Todo jogo que salva no hardware original deve salvar de verdade aqui: RAM com bateria (SRAM), EEPROM, flash, cartão de memória ou o que o console usar, incluindo o relógio do cartucho quando houver. Ao adicionar um console, o save faz parte do critério de conclusão; não é opcional nem "para depois".
- O save do jogo é separado do estado salvo ("continuar de onde parou"): funciona com essa opção ligada ou desligada, e "Recomeçar do início" nunca o apaga.
- Cada console tem a própria pasta de saves (`<dados do app>/saves/<console>/`), com um arquivo por jogo.
- O formato do arquivo deve ser o bruto do hardware (ex.: `.sav` com o conteúdo exato da RAM do cartucho), compatível com outros emuladores, para o save poder ser levado e usado em outro lugar.
- O usuário pode exportar o save de um jogo para um arquivo escolhido por ele, e importar um save de fora, com validação de tamanho e formato. Um save inválido é recusado com mensagem clara e nunca sobrescreve o existente.
- Gravação atômica (arquivo temporário + rename), sem perder o save se o processo morrer. Um save existente que não pôde ser lido ou não confere é preservado, nunca sobrescrito.
- Testes obrigatórios: ida e volta do save, save mantido ao recomeçar, recusa de tamanho/formato errado sem perda do arquivo, e exportar/importar.

## 13. Melhoria de imagem (obrigatório quando o console permitir)
- Se o console admite uma melhoria de imagem que faça sentido para ele (ex.: filtro de ampliação para pixel art nos consoles 2D, suavização ou resolução interna maior nos 3D), ela deve ser implementada ao adicionar o console; não é "para depois".
- Vem ligada por padrão e pode ser desligada por console nas configurações. Desligada, a imagem é a original, pixel a pixel.
- A melhoria é só de apresentação: não altera a emulação, o tempo do jogo, os saves nem o estado salvo, e não pode derrubar o desempenho abaixo da velocidade real do console.
- Testes obrigatórios: saída do filtro em casos conhecidos, imagem idêntica à original com a opção desligada e a preferência salva e respeitada ao abrir o jogo.

## 14. Proibições
Nunca: inventar resultados de compilação/testes/benchmarks; contornar erros do compilador sem investigar a causa; usar `unsafe` sem justificativa; introduzir dependências desnecessárias; otimizar com base só em suposições; desativar verificações de segurança/qualidade para aprovar uma alteração; modificar funcionalidades não relacionadas; tratar código compilável como correto; entregar sem avaliar casos de falha; declarar concluída uma tarefa com falhas relevantes não informadas.

## 15. Critério de conclusão
Concluída somente quando: atende aos requisitos; segue as convenções do Rust; riscos de segurança avaliados; testes pertinentes implementados e executados (quando possível); formatação, lint e compilação verificados; compatibilidade multiplataforma considerada; performance avaliada conforme a criticidade; limitações e pendências explícitas.

**Princípio central:** produzir software Rust correto, seguro, eficiente, multiplataforma e sustentável. Toda decisão técnica deve ser justificada pelo requisito real, pela documentação oficial ou por evidência verificável.
