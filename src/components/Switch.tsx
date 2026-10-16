import { useId } from "react";
import styles from "./Switch.module.css";

interface SwitchProps {
  label: string;
  /** Explica o efeito da opção; vira a descrição acessível do controle. */
  description?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}

/** Opção liga/desliga com rótulo e descrição; a linha inteira é clicável. */
export function Switch({ label, description, checked, disabled, onChange }: SwitchProps) {
  const labelId = useId();
  const descriptionId = useId();

  return (
    <div className={styles.row} data-disabled={disabled || undefined}>
      <div className={styles.text}>
        <span id={labelId} className={styles.label} onClick={() => !disabled && onChange(!checked)}>
          {label}
        </span>
        {description && (
          <span id={descriptionId} className={styles.description}>
            {description}
          </span>
        )}
      </div>
      <button
        type="button"
        role="switch"
        className={styles.switch}
        aria-checked={checked}
        aria-labelledby={labelId}
        aria-describedby={description ? descriptionId : undefined}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className={styles.thumb} aria-hidden="true" />
      </button>
    </div>
  );
}
