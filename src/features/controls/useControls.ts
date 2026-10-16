import { useCallback, useEffect, useRef, useState } from "react";
import {
  ACTION_LABELS,
  type Bindings,
  type ControlAction,
  type ControlsInfo,
  keyName,
  rebind,
  sameBindings,
} from "../../domain/controls";
import { getControls, saveControls } from "../../services/controls";

type LoadState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; info: ControlsInfo };

export interface ControlsState {
  load: LoadState;
  /** Ação esperando a próxima tecla, ou `null`. */
  capturing: ControlAction | null;
  saving: boolean;
  /** Mensagem de status para o usuário (anunciada a leitores de tela). */
  notice: string | null;
  error: string | null;
  startCapture: (action: ControlAction) => void;
  cancelCapture: () => void;
  resetToDefaults: () => void;
  retry: () => void;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Teclas de um console: carrega, captura a próxima tecla para uma ação,
 * troca com a ação que já a usava e salva na hora. Se a gravação falhar,
 * volta ao último mapa salvo.
 */
export function useControls(consoleId: string): ControlsState {
  const [load, setLoad] = useState<LoadState>({ status: "loading" });
  const [round, setRound] = useState(0);
  const [capturing, setCapturing] = useState<ControlAction | null>(null);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    let current = true;
    setLoad({ status: "loading" });
    getControls(consoleId).then(
      (info) => current && setLoad({ status: "ready", info }),
      (e: unknown) =>
        current && setLoad({ status: "error", message: message(e) }),
    );
    return () => {
      current = false;
    };
  }, [consoleId, round]);

  const info = load.status === "ready" ? load.info : null;

  const save = useCallback(
    (next: Bindings, done: string) => {
      if (!info || sameBindings(next, info.bindings)) {
        setNotice(done);
        return;
      }
      const previous = info;
      setLoad({ status: "ready", info: { ...info, bindings: next } });
      setSaving(true);
      setError(null);
      saveControls(consoleId, next).then(
        (saved) => {
          if (!alive.current) return;
          setLoad({ status: "ready", info: saved });
          setSaving(false);
          setNotice(done);
        },
        (e: unknown) => {
          if (!alive.current) return;
          setLoad({ status: "ready", info: previous });
          setSaving(false);
          setNotice(null);
          setError(message(e));
        },
      );
    },
    [consoleId, info],
  );

  const assign = useCallback(
    (action: ControlAction, code: string) => {
      if (!info) return;
      const { bindings, swappedWith } = rebind(info.bindings, action, code);
      const assigned = `${ACTION_LABELS[action]} agora usa ${keyName(code)}.`;
      const swapped = swappedWith
        ? ` ${ACTION_LABELS[swappedWith]} ficou com ${keyName(bindings[swappedWith] ?? "")}.`
        : "";
      save(bindings, assigned + swapped);
    },
    [info, save],
  );

  // Captura global: a tecla não deve acionar mais nada (nem fechar o diálogo).
  useEffect(() => {
    if (!capturing || !info) return;
    function onKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      if (!capturing || !info || event.repeat) return;
      if (event.code === "Escape") {
        setCapturing(null);
        setNotice("Troca cancelada.");
        return;
      }
      if (!info.supportedKeys.includes(event.code)) {
        setNotice(
          `A tecla ${event.key || event.code} não pode ser usada. Escolha outra.`,
        );
        return;
      }
      setCapturing(null);
      assign(capturing, event.code);
    }
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () =>
      window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [capturing, info, assign]);

  return {
    load,
    capturing,
    saving,
    notice,
    error,
    startCapture: (action) => {
      if (saving) return;
      setError(null);
      setCapturing(action);
      setNotice(
        `Pressione a tecla para ${ACTION_LABELS[action]}. Esc cancela.`,
      );
    },
    cancelCapture: () => setCapturing(null),
    resetToDefaults: () => {
      if (!info) return;
      setCapturing(null);
      save(info.defaults, "Controles restaurados para o padrão.");
    },
    retry: () => setRound((r) => r + 1),
  };
}
