import {
  ACTION_LABELS,
  type Bindings,
  type ControlAction,
  keyLabel,
  keyName,
} from "../../domain/controls";
import styles from "./Controller.module.css";

export interface KeyButtonProps {
  action: ControlAction;
  bindings: Bindings;
  capturing: ControlAction | null;
  disabled: boolean;
  /** Id da dica de uso, para leitores de tela. */
  hintId: string;
  onSelect: (action: ControlAction) => void;
}

/** Tecla atribuída a uma ação: clicar começa a troca. */
export function KeyButton({
  action,
  bindings,
  capturing,
  disabled,
  hintId,
  onSelect,
}: KeyButtonProps) {
  const waiting = capturing === action;
  const label = ACTION_LABELS[action];
  const key = bindings[action] ?? "";
  return (
    <button
      type="button"
      className={styles.key}
      data-capturing={waiting || undefined}
      aria-label={
        waiting ? `${label}: aguardando tecla` : `${label}: ${keyName(key)}`
      }
      aria-describedby={hintId}
      aria-pressed={waiting}
      disabled={disabled}
      onClick={() => onSelect(action)}
    >
      {waiting ? "…" : keyLabel(key)}
    </button>
  );
}

export interface ControllerProps {
  bindings: Bindings;
  capturing: ControlAction | null;
  disabled: boolean;
  onSelect: (action: ControlAction) => void;
}
