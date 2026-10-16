/**
 * Logos vetoriais dos consoles, por `id` do catálogo, já na versão para fundo
 * escuro (cores originais; preto convertido para branco).
 * Fontes, licenças e tratamento: src/assets/consoles/SOURCES.md.
 *
 * Os SVGs são embutidos no bundle e entregues como data URI. Assim nenhum
 * logo depende de uma requisição de arquivo à parte: antes, só os maiores que
 * o limite de inline do Vite (4 KB) eram servidos como arquivo e deixavam de
 * aparecer no app.
 */
const logoSources = import.meta.glob<string>("../../assets/consoles/*.svg", {
  eager: true,
  query: "?raw",
  import: "default",
});

const byId = new Map<string, string>(
  Object.entries(logoSources).map(([path, svg]) => [
    path.slice(path.lastIndexOf("/") + 1, -".svg".length),
    `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`,
  ]),
);

export function consoleLogoUrl(id: string): string | undefined {
  return byId.get(id);
}
