import { useCallback, useEffect, useState } from "react";
import type { Console } from "../../domain/console";
import { fetchConsoles } from "../../services/consoles";

export type ConsolesState =
  | { status: "loading" }
  | { status: "error" }
  | { status: "success"; consoles: Console[] };

/** Carrega o catálogo e expõe `retry` para nova tentativa após erro. */
export function useConsoles(): { state: ConsolesState; retry: () => void } {
  const [state, setState] = useState<ConsolesState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    // Descarta respostas de tentativas antigas ou após desmontar.
    let current = true;
    setState({ status: "loading" });
    fetchConsoles().then(
      (consoles) => {
        if (current) setState({ status: "success", consoles });
      },
      (error: unknown) => {
        console.error("Falha ao carregar o catálogo de consoles", error);
        if (current) setState({ status: "error" });
      },
    );
    return () => {
      current = false;
    };
  }, [attempt]);

  const retry = useCallback(() => setAttempt((n) => n + 1), []);

  return { state, retry };
}
