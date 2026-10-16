import { useId } from "react";
import styles from "./SegmentedControl.module.css";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
}

interface SegmentedControlProps<T extends string> {
  label: string;
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
}

/**
 * Escolha única entre poucas opções. Usa rádios nativos: teclado (setas),
 * foco e leitores de tela funcionam sem lógica extra.
 */
export function SegmentedControl<T extends string>({
  label,
  options,
  value,
  onChange,
}: SegmentedControlProps<T>) {
  const name = useId();

  return (
    <fieldset className={styles.control}>
      <legend className={styles.legend}>{label}</legend>
      <div className={styles.track}>
        {options.map(({ value: optionValue, label: optionLabel }) => (
          <label key={optionValue} className={styles.option}>
            <input
              className={styles.input}
              type="radio"
              name={name}
              value={optionValue}
              checked={optionValue === value}
              onChange={() => onChange(optionValue)}
            />
            <span className={styles.face}>{optionLabel}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}
