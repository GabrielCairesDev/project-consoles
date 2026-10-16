import { useCallback, useEffect, useRef, useState } from "react";
import { type BiosStatus, chooseBios, getBios } from "../../services/bios";

export type BiosState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; bios: BiosStatus };

export interface BiosControl {
  state: BiosState;
  /** Escolha em andamento (diálogo aberto ou cópia). */
  choosing: boolean;
  /** Falha da última escolha (o BIOS anterior continua). */
  chooseError: string | null;
  choose: () => void;
  retry: () => void;
}

function message(error: unknown, fallback: string): string {
  return typeof error === "string" ? error : fallback;
}

/**
 * BIOS do console. Desligado (`enabled = false`, console sem BIOS) não
 * acessa o núcleo.
 */
export function useBios(consoleId: string, enabled: boolean): BiosControl {
  const [state, setState] = useState<BiosState>({ status: "loading" });
  const [round, setRound] = useState(0);
  const [choosing, setChoosing] = useState(false);
  const [chooseError, setChooseError] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let current = true;
    setState({ status: "loading" });
    getBios(consoleId).then(
      (bios) => current && setState({ status: "ready", bios }),
      (error: unknown) =>
        current &&
        setState({ status: "error", message: message(error, "Não foi possível ver o BIOS.") }),
    );
    return () => {
      current = false;
    };
  }, [consoleId, enabled, round]);

  const choose = useCallback(() => {
    if (choosing) return;
    setChoosing(true);
    setChooseError(null);
    chooseBios(consoleId).then(
      (bios) => {
        if (!alive.current) return;
        if (bios) setState({ status: "ready", bios });
        setChoosing(false);
      },
      (error: unknown) => {
        if (!alive.current) return;
        setChoosing(false);
        setChooseError(message(error, "Não foi possível instalar o BIOS."));
      },
    );
  }, [consoleId, choosing]);

  return { state, choosing, chooseError, choose, retry: () => setRound((r) => r + 1) };
}
