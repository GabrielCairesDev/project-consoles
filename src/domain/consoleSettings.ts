/** Opções de jogo de um console; espelha `settings::ConsoleSettings`. */
export interface ConsoleSettings {
  /** Salvar o progresso ao fechar e continuar dele ao abrir. */
  resume: boolean;
  /** Melhoria de imagem ligada (o padrão); desligada, a imagem é a original. */
  enhance: boolean;
}
