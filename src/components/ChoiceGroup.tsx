import { type KeyboardEvent, useId, useRef } from "react";
import { TriangleAlert } from "lucide-react";
import styles from "./ChoiceGroup.module.css";

export interface Choice<T extends string> {
  value: T;
  label: string;
  /** Explica o efeito da escolha; vira a descrição acessível dela. */
  description?: string;
  /** Aviso mostrado antes de escolher (ex.: opção pesada); também entra
   * na descrição acessível. */
  warning?: string;
}

interface ChoiceGroupProps<T extends string> {
  label: string;
  /** Explica o grupo como um todo, abaixo do rótulo. */
  description?: string;
  choices: Choice<T>[];
  /** `null`: nenhuma escolha marcada (ex.: estado que não está na lista). */
  value: T | null;
  disabled?: boolean;
  /** `segmented`: opções curtas lado a lado; `list`: uma por linha, com descrição. */
  variant?: "segmented" | "list";
  onChange: (value: T) => void;
}

/**
 * Escolha única entre opções (`role="radiogroup"`). Setas mudam a escolha
 * como num grupo de rádio nativo, e só a opção marcada entra no Tab.
 */
export function ChoiceGroup<T extends string>({
  label,
  description,
  choices,
  value,
  disabled,
  variant = "list",
  onChange,
}: ChoiceGroupProps<T>) {
  const labelId = useId();
  const descriptionId = useId();
  const optionIdPrefix = useId();
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const selected = choices.findIndex((choice) => choice.value === value);
  const tabStop = selected >= 0 ? selected : 0;

  function onKeyDown(event: KeyboardEvent, index: number) {
    const step =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? -1
          : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = (index + step + choices.length) % choices.length;
    buttons.current[next]?.focus();
    onChange(choices[next].value);
  }

  return (
    <div className={styles.group} data-disabled={disabled || undefined}>
      <div className={styles.text}>
        <span id={labelId} className={styles.label}>
          {label}
        </span>
        {description && (
          <span id={descriptionId} className={styles.description}>
            {description}
          </span>
        )}
      </div>
      <div
        role="radiogroup"
        className={styles[variant]}
        aria-labelledby={labelId}
        aria-describedby={description ? descriptionId : undefined}
        aria-disabled={disabled || undefined}
      >
        {choices.map((choice, index) => {
          const checked = index === selected;
          const choiceLabelId = `${optionIdPrefix}-${index}-label`;
          const choiceDescriptionId = `${optionIdPrefix}-${index}-description`;
          const choiceWarningId = `${optionIdPrefix}-${index}-warning`;
          const describedBy = [
            choice.description && choiceDescriptionId,
            choice.warning && choiceWarningId,
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <button
              key={choice.value}
              ref={(button) => {
                buttons.current[index] = button;
              }}
              type="button"
              role="radio"
              className={styles.choice}
              aria-checked={checked}
              aria-labelledby={choiceLabelId}
              aria-describedby={describedBy || undefined}
              tabIndex={index === tabStop ? 0 : -1}
              disabled={disabled}
              onClick={() => !checked && onChange(choice.value)}
              onKeyDown={(event) => onKeyDown(event, index)}
            >
              {variant === "list" && <span className={styles.radio} aria-hidden="true" />}
              <span className={styles.choiceText}>
                <span id={choiceLabelId} className={styles.choiceLabel}>
                  {choice.label}
                </span>
                {choice.description && (
                  <span id={choiceDescriptionId} className={styles.choiceDescription}>
                    {choice.description}
                  </span>
                )}
                {choice.warning && (
                  <span id={choiceWarningId} className={styles.choiceWarning}>
                    <TriangleAlert className={styles.warningIcon} aria-hidden="true" />
                    {choice.warning}
                  </span>
                )}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
