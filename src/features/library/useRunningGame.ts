import { useCallback, useEffect, useState } from "react";
import { launchGame, onGameExited } from "../../services/library";

export interface RunningGame {
  /** Jogo sendo iniciado ou em execução; `null` quando nenhum está aberto. */
  gameId: string | null;
  /** O jogo foi pedido ao núcleo e ainda não começou. */
  launching: boolean;
  /** Mensagem da última falha ao iniciar um jogo. */
  error: string | null;
  /** Inicia o jogo; ignorado enquanto outro estiver aberto. */
  launch: (gameId: string) => void;
}

interface State {
  gameId: string | null;
  launching: boolean;
  error: string | null;
}

const IDLE: State = { gameId: null, launching: false, error: null };

/**
 * Jogo em execução numa janela nativa. Um por vez: o estado volta ao normal
 * quando o núcleo avisa, via `game-exited`, que a janela fechou.
 */
export function useRunningGame(): RunningGame {
  const [state, setState] = useState<State>(IDLE);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    onGameExited(({ gameId }) => {
      setState((current) => (current.gameId === gameId ? IDLE : current));
    }).then(
      (stop) => {
        if (active) unlisten = stop;
        else stop();
      },
      (error: unknown) => console.error("Não foi possível escutar game-exited", error),
    );
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const launch = useCallback(
    (gameId: string) => {
      if (state.gameId !== null) return;
      setState({ gameId, launching: true, error: null });
      launchGame(gameId).then(
        // Se o jogo já fechou (game-exited antes da resposta), o estado já é outro.
        () => setState((current) => (current.gameId === gameId ? { ...current, launching: false } : current)),
        (error: unknown) =>
          setState((current) =>
            current.gameId === gameId
              ? {
                  ...IDLE,
                  error: typeof error === "string" ? error : "Não foi possível iniciar o jogo.",
                }
              : current,
          ),
      );
    },
    [state.gameId],
  );

  return { ...state, launch };
}
