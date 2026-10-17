/** Espelha `catalog::ConsoleKind` do núcleo Rust (serde camelCase). */
export type ConsoleKind = "home" | "handheld" | "hybrid";

/**
 * Melhoria de imagem do console; espelha `player::Enhancement::id`.
 * `pixelArt`: ampliação que arredonda contornos (2D); `smooth`: suavização
 * bilinear (3D).
 */
export type ImageEnhancement = "pixelArt" | "smooth";

/** Espelha `catalog::Console` do núcleo Rust (serde camelCase). */
export interface Console {
  id: string;
  name: string;
  manufacturer: string;
  releaseYear: number;
  kind: ConsoleKind;
  unitsSold: number;
  /** O app reconhece ROMs deste console. */
  supported: boolean;
  /** O app executa jogos deste console em uma janela nativa. */
  playable: boolean;
  /** Os jogos só rodam com o BIOS do usuário, escolhido nas configurações. */
  needsBios: boolean;
  /** O núcleo sabe "continuar de onde parou". */
  resumable: boolean;
  /** Melhoria de imagem disponível, se o console roda. */
  enhancement: ImageEnhancement | null;
}

export interface DecadeGroup {
  /** Primeiro ano da década (ex.: 1980). */
  decade: number;
  consoles: Console[];
}

/**
 * Agrupa consoles por década de lançamento, em ordem crescente.
 * Dentro de cada década, ordena por ano, preservando a ordem recebida do
 * núcleo entre consoles do mesmo ano.
 */
export function groupByDecade(consoles: readonly Console[]): DecadeGroup[] {
  const byDecade = new Map<number, Console[]>();
  for (const item of consoles) {
    const decade = Math.floor(item.releaseYear / 10) * 10;
    const group = byDecade.get(decade);
    if (group) {
      group.push(item);
    } else {
      byDecade.set(decade, [item]);
    }
  }
  return [...byDecade.entries()]
    .sort(([a], [b]) => a - b)
    .map(([decade, items]) => ({
      decade,
      consoles: items.sort((a, b) => a.releaseYear - b.releaseYear),
    }));
}

export function decadeLabel(decade: number): string {
  return `Anos ${decade}`;
}
