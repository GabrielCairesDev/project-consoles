import type { ButtonHTMLAttributes, ComponentType } from "react";
import styles from "./Button.module.css";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "ghost";
  icon?: ComponentType<{ className?: string; "aria-hidden"?: boolean }>;
}

/** Botão padrão do app, com ícone opcional à esquerda do texto. */
export function Button({
  variant = "primary",
  icon: Icon,
  className,
  children,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={[styles.button, styles[variant], className].filter(Boolean).join(" ")}
      {...rest}
    >
      {Icon && <Icon className={styles.icon} aria-hidden />}
      {children}
    </button>
  );
}
