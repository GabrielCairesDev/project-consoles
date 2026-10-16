import { type ButtonHTMLAttributes, type ComponentType, useId } from "react";
import styles from "./IconButton.module.css";

interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  icon: ComponentType<{ className?: string; "aria-hidden"?: boolean }>;
  /** Nome acessível; também é o tooltip quando `tooltip` não é informado. */
  label: string;
  /** Texto exibido ao passar o mouse ou focar pelo teclado. */
  tooltip?: string;
}

/**
 * Botão só com ícone. O tooltip aparece no hover e no foco de teclado; quando
 * difere do nome, é ligado ao botão como descrição acessível.
 */
export function IconButton({
  icon: Icon,
  label,
  tooltip,
  className,
  type = "button",
  ...rest
}: IconButtonProps) {
  const tooltipId = useId();
  const text = tooltip ?? label;

  return (
    <span className={styles.wrapper}>
      <button
        type={type}
        className={[styles.button, className].filter(Boolean).join(" ")}
        aria-label={label}
        aria-describedby={text !== label ? tooltipId : undefined}
        {...rest}
      >
        <Icon className={styles.icon} aria-hidden />
      </button>
      <span id={tooltipId} role="tooltip" className={styles.tooltip}>
        {text}
      </span>
    </span>
  );
}
