/** Espelha `catalog::ConsoleKind` do núcleo Rust (serde camelCase). */
export type ConsoleKind = "home" | "handheld" | "hybrid";

import type { ImageInfo } from "./image";

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
  /** Opções de imagem do console, se ele roda. */
  image: ImageInfo | null;
}

/**
 * Consoles em ordem de lançamento, do mais antigo ao mais novo. Entre
 * consoles do mesmo ano, preserva a ordem recebida do núcleo (por nome).
 * Não altera o array recebido.
 */
export function sortByRelease(consoles: readonly Console[]): Console[] {
  return [...consoles].sort((a, b) => a.releaseYear - b.releaseYear);
}
