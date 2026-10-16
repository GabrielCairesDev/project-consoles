/** Espelha `library::Game` do núcleo Rust (serde camelCase). */
export interface Game {
  /** Estável na biblioteca: caminho relativo à pasta. */
  id: string;
  consoleId: string;
  title: string;
  fileName: string;
  recognized: boolean;
  /** Há progresso salvo: abrir o jogo continua de onde parou. */
  hasSave: boolean;
}

/** Espelha `commands::LibraryScan`. */
export interface LibraryScan {
  folder: string;
  /** Jogos de todos os consoles. */
  games: Game[];
}

/** Espelha `covers::CoverKind`. */
export type CoverKind = "boxart" | "icon";

/** Capa pronta para `<img src>`. */
export interface CoverImage {
  src: string;
  kind: CoverKind;
}

/** Quantidade de jogos por `consoleId`. */
export function countGamesByConsole(games: readonly Game[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const game of games) {
    counts.set(game.consoleId, (counts.get(game.consoleId) ?? 0) + 1);
  }
  return counts;
}

/** Ex.: 1 → "1 jogo"; 12 → "12 jogos". */
export function formatGameCount(count: number): string {
  return count === 1 ? "1 jogo" : `${count} jogos`;
}
