import { useCallback, useRef, useState } from "react";
import type { Game } from "../../domain/game";
import { exportGameSave, importGameSave } from "../../services/library";

/** Resultado da última exportação ou importação, para o aviso na tela. */
export type SaveTransferNotice = { kind: "success" | "error"; message: string } | null;

export interface GameSaveTransfer {
  notice: SaveTransferNotice;
  exportSave: (game: Game) => void;
  importSave: (game: Game) => void;
}

function errorMessage(error: unknown, fallback: string): string {
  return typeof error === "string" ? error : fallback;
}

/**
 * Exporta e importa o save do próprio jogo (`.sav`). Uma operação por vez:
 * enquanto o diálogo ou a cópia estão em andamento, novos pedidos são
 * ignorados. `onImported` anota o jogo que passou a ter save.
 */
export function useGameSaveTransfer(onImported: (gameId: string) => void): GameSaveTransfer {
  const [notice, setNotice] = useState<SaveTransferNotice>(null);
  const busy = useRef(false);

  const run = useCallback(
    (action: () => Promise<boolean>, success: string, fallback: string, onDone?: () => void) => {
      if (busy.current) return;
      busy.current = true;
      setNotice(null);
      action()
        .then(
          (done) => {
            if (!done) return; // cancelado no diálogo
            onDone?.();
            setNotice({ kind: "success", message: success });
          },
          (error: unknown) =>
            setNotice({ kind: "error", message: errorMessage(error, fallback) }),
        )
        .finally(() => {
          busy.current = false;
        });
    },
    [],
  );

  const exportSave = useCallback(
    (game: Game) =>
      run(
        () => exportGameSave(game),
        `Save de ${game.title} exportado.`,
        "Não foi possível exportar o save.",
      ),
    [run],
  );

  const importSave = useCallback(
    (game: Game) =>
      run(
        () => importGameSave(game),
        `Save importado em ${game.title}.`,
        "Não foi possível importar o save.",
        () => onImported(game.id),
      ),
    [run, onImported],
  );

  return { notice, exportSave, importSave };
}
