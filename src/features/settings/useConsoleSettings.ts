import { useCallback, useEffect, useRef, useState } from "react";
import type { ConsoleSettings } from "../../domain/consoleSettings";
import { getConsoleSettings, saveConsoleSettings } from "../../services/consoleSettings";

export type ConsoleSettingsState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; settings: ConsoleSettings };

export interface ConsoleSettingsControl {
  state: ConsoleSettingsState;
  saving: boolean;
  /** Falha da última gravação (o valor anterior foi restaurado). */
  saveError: string | null;
  update: (patch: Partial<ConsoleSettings>) => void;
  retry: () => void;
}

function message(error: unknown): string {
  return typeof error === "string" ? error : "Não foi possível acessar as configurações.";
}

/**
 * Opções de jogo do console. A alteração aparece na hora e é gravada em
 * seguida; se a gravação falhar, volta ao valor anterior. Desligado
 * (`enabled = false`) não acessa o núcleo.
 */
export function useConsoleSettings(consoleId: string, enabled: boolean): ConsoleSettingsControl {
  const [state, setState] = useState<ConsoleSettingsState>({ status: "loading" });
  const [round, setRound] = useState(0);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
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
    getConsoleSettings(consoleId).then(
      (settings) => current && setState({ status: "ready", settings }),
      (error: unknown) => current && setState({ status: "error", message: message(error) }),
    );
    return () => {
      current = false;
    };
  }, [consoleId, enabled, round]);

  const update = useCallback(
    (patch: Partial<ConsoleSettings>) => {
      if (state.status !== "ready" || saving) return;
      const previous = state.settings;
      const next = { ...previous, ...patch };
      setState({ status: "ready", settings: next });
      setSaving(true);
      setSaveError(null);
      saveConsoleSettings(consoleId, next).then(
        (saved) => {
          if (!alive.current) return;
          setState({ status: "ready", settings: saved });
          setSaving(false);
        },
        (error: unknown) => {
          if (!alive.current) return;
          setState({ status: "ready", settings: previous });
          setSaving(false);
          setSaveError(message(error));
        },
      );
    },
    [consoleId, state, saving],
  );

  return { state, saving, saveError, update, retry: () => setRound((r) => r + 1) };
}
