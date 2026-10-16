/** Espelha `library::Game` do núcleo Rust (serde camelCase). */
export interface Game {
  /** Estável na biblioteca: caminho relativo à pasta. */
  id: string;
  consoleId: string;
  title: string;
  fileName: string;
  recognized: boolean;
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
