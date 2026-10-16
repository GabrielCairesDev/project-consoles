import { createContext, useContext, useMemo } from "react";
import { type LibraryScan, countGamesByConsole } from "../../domain/game";

export type LibraryState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "unset" }
  | { status: "scanning"; folder: string }
  | { status: "ready"; scan: LibraryScan };

export interface LibraryContextValue {
  state: LibraryState;
  /** Abre o seletor de pasta; ao confirmar, varre a nova pasta. */
  choose: () => Promise<void>;
  choosing: boolean;
  chooseError: string | null;
  /** Varre de novo a pasta atual. */
  refresh: () => void;
}

/**
 * Contexto da biblioteca. Fica separado do `<LibraryProvider>` porque o Fast
 * Refresh só recarrega a quente arquivos que exportam apenas componentes.
 */
export const LibraryContext = createContext<LibraryContextValue | null>(null);

export function useLibrary(): LibraryContextValue {
  const value = useContext(LibraryContext);
  if (!value) throw new Error("useLibrary precisa de <LibraryProvider>");
  return value;
}

const NO_COUNTS: ReadonlyMap<string, number> = new Map();

export interface GameCounts {
  /** Jogos encontrados por console; vazio até a varredura terminar. */
  counts: ReadonlyMap<string, number>;
  /** A biblioteca ainda está sendo lida ou varrida. */
  loading: boolean;
}

/** Contagem de jogos por console. Fora de um `<LibraryProvider>`, vazia. */
export function useGameCounts(): GameCounts {
  const state = useContext(LibraryContext)?.state;
  const games = state?.status === "ready" ? state.scan.games : null;
  const loading = state?.status === "loading" || state?.status === "scanning";
  const counts = useMemo(() => (games ? countGamesByConsole(games) : NO_COUNTS), [games]);
  return { counts, loading };
}
