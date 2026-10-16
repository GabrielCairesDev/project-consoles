/** Espelha `catalog::ConsoleKind` do núcleo Rust (serde camelCase). */
export type ConsoleKind = "home" | "handheld" | "hybrid";

/** Espelha `catalog::Console` do núcleo Rust (serde camelCase). */
export interface Console {
  id: string;
  name: string;
  manufacturer: string;
  releaseYear: number;
  kind: ConsoleKind;
  unitsSold: number;
}

export type SortOrder = "year" | "popularity";

/**
 * Retorna uma cópia ordenada:
 * - `year`: lançamento mais antigo primeiro;
 * - `popularity`: mais vendido primeiro, empates pelo lançamento mais antigo.
 * A ordenação é estável: empates completos preservam a ordem recebida.
 */
export function sortConsoles(consoles: readonly Console[], order: SortOrder): Console[] {
  const byYear = (a: Console, b: Console) => a.releaseYear - b.releaseYear;
  const compare =
    order === "year" ? byYear : (a: Console, b: Console) => b.unitsSold - a.unitsSold || byYear(a, b);
  return [...consoles].sort(compare);
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
