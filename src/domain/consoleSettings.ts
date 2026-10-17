import type { ImageChoices } from "./image";

/** Opções de jogo de um console; espelha `settings::ConsoleSettings`. */
export interface ConsoleSettings {
  /** Salvar o progresso ao fechar e continuar dele ao abrir. */
  resume: boolean;
  /** Escolhas de imagem; o launcher sempre devolve cada uma explícita. */
  image: ImageChoices;
}
