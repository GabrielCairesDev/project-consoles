import { useCallback, useEffect, useRef, useState } from "react";
import type { Game } from "../../domain/game";
import { deleteSave, launchGame, onGameExited } from "../../services/library";

export interface RunningGame {
  /** Jogo sendo iniciado ou em execução; `null` quando nenhum está aberto. */
  gameId: string | null;
  /** O jogo foi pedido ao núcleo e ainda não começou. */
  launching: boolean;
  /** Mensagem da última falha ao iniciar um jogo. */
  error: string | null;
  /** Inicia (ou continua) o jogo; ignorado enquanto outro estiver aberto. */
  launch: (gameId: string) => void;
  /** Apaga o progresso salvo e inicia o jogo do começo. */
  restart: (gameId: string) => void;
  /** Se o jogo tem progresso salvo, considerando o que mudou nesta sessão. */
  hasSave: (game: Game) => boolean;
  /** Se o jogo tem save próprio (`.sav`), considerando o que mudou nesta sessão. */
  hasGameSave: (game: Game) => boolean;
  /** Anota que o jogo passou a ter save próprio (ex.: depois de importar). */
  markGameSave: (gameId: string) => void;
}

interface State {
  gameId: string | null;
  launching: boolean;
  error: string | null;
}

const IDLE: State = { gameId: null, launching: false, error: null };

type SaveFlags = Partial<Pick<Game, "hasSave" | "hasGameSave">>;

/** Saves alterados desde a varredura `source` (fechar, recomeçar ou importar). */
interface SaveOverrides {
  source: readonly Game[];
  values: ReadonlyMap<string, SaveFlags>;
}

function errorMessage(error: unknown, fallback: string): string {
  return typeof error === "string" ? error : fallback;
}

/**
 * Jogo em execução numa janela nativa. Um por vez: o estado volta ao normal
 * quando o núcleo avisa, via `game-exited`, que a janela fechou, e o aviso
 * também diz se ficou progresso salvo. Uma nova varredura (`games`) já traz
 * esse estado do disco e descarta o que foi anotado aqui.
 */
export function useRunningGame(games: readonly Game[]): RunningGame {
  const [state, setState] = useState<State>(IDLE);
  const [saves, setSaves] = useState<SaveOverrides>({
    source: games,
    values: new Map(),
  });
  const currentGames = useRef(games);
  currentGames.current = games;

  const setSave = useCallback((gameId: string, flags: SaveFlags) => {
    setSaves((current) => {
      const source = currentGames.current;
      const values = new Map(current.source === source ? current.values : []);
      values.set(gameId, { ...values.get(gameId), ...flags });
      return { source, values };
    });
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    onGameExited(({ gameId, hasSave, hasGameSave }) => {
      setSave(gameId, { hasSave, hasGameSave });
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
  }, [setSave]);

  const start = useCallback(
    (gameId: string, prepare?: () => Promise<void>) => {
      if (state.gameId !== null) return;
      setState({ gameId, launching: true, error: null });
      const fail = (message: string) =>
        setState((current) => (current.gameId === gameId ? { ...IDLE, error: message } : current));
      (prepare ? prepare().then(() => launchGame(gameId)) : launchGame(gameId)).then(
        // Se o jogo já fechou (game-exited antes da resposta), o estado já é outro.
        () =>
          setState((current) =>
            current.gameId === gameId ? { ...current, launching: false } : current,
          ),
        (error: unknown) => fail(errorMessage(error, "Não foi possível iniciar o jogo.")),
      );
    },
    [state.gameId],
  );

  const launch = useCallback((gameId: string) => start(gameId), [start]);

  const restart = useCallback(
    (gameId: string) =>
      start(gameId, async () => {
        await deleteSave(gameId);
        setSave(gameId, { hasSave: false });
      }),
    [start, setSave],
  );

  const overrides = saves.source === games ? saves.values : null;
  const hasSave = useCallback(
    (game: Game) => overrides?.get(game.id)?.hasSave ?? game.hasSave,
    [overrides],
  );
  const hasGameSave = useCallback(
    (game: Game) => overrides?.get(game.id)?.hasGameSave ?? game.hasGameSave,
    [overrides],
  );
  const markGameSave = useCallback(
    (gameId: string) => setSave(gameId, { hasGameSave: true }),
    [setSave],
  );

  return { ...state, launch, restart, hasSave, hasGameSave, markGameSave };
}
