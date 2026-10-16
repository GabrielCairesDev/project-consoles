import { type ComponentType, type KeyboardEvent, useEffect, useId, useRef } from "react";
import { EllipsisVertical } from "lucide-react";
import styles from "./Menu.module.css";

export interface MenuItem {
  label: string;
  icon?: ComponentType<{ className?: string; "aria-hidden"?: boolean }>;
  onSelect: () => void;
  /** Ação destrutiva: destacada com a cor de perigo. */
  danger?: boolean;
}

interface MenuProps {
  /** Nome acessível do botão que abre o menu. */
  label: string;
  items: readonly MenuItem[];
  /** Controlado por fora para também abrir pelo clique direito. */
  open: boolean;
  onOpenChange: (open: boolean) => void;
  className?: string;
}

/**
 * Botão "⋯" com menu suspenso. Setas navegam, Esc ou clique fora fecham e o
 * foco volta ao botão; escolher um item fecha o menu antes de executá-lo.
 */
export function Menu({ label, items, open, onOpenChange, className }: MenuProps) {
  const menuId = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);

  const itemElements = () =>
    Array.from(root.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? []);

  useEffect(() => {
    if (!open) return;
    itemElements()[0]?.focus();
    function onPointerDown(event: PointerEvent) {
      if (!root.current?.contains(event.target as Node)) onOpenChange(false);
    }
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open, onOpenChange]);

  function close(restoreFocus: boolean) {
    onOpenChange(false);
    if (restoreFocus) trigger.current?.focus();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!open) return;
    const elements = itemElements();
    const index = elements.findIndex((el) => el === document.activeElement);
    const move = (to: number) => {
      event.preventDefault();
      elements[(to + elements.length) % elements.length]?.focus();
    };
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        close(true);
        break;
      case "ArrowDown":
        move(index + 1);
        break;
      case "ArrowUp":
        move(index - 1);
        break;
      case "Home":
        move(0);
        break;
      case "End":
        move(elements.length - 1);
        break;
      case "Tab":
        close(false);
        break;
    }
  }

  return (
    <div ref={root} className={[styles.root, className].filter(Boolean).join(" ")} onKeyDown={handleKeyDown}>
      <button
        ref={trigger}
        type="button"
        className={styles.trigger}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        data-open={open || undefined}
        onClick={() => onOpenChange(!open)}
      >
        <EllipsisVertical className={styles.triggerIcon} aria-hidden />
      </button>
      {open && (
        <div id={menuId} role="menu" aria-label={label} className={styles.menu}>
          {items.map(({ label: itemLabel, icon: Icon, onSelect, danger }) => (
            <button
              key={itemLabel}
              type="button"
              role="menuitem"
              tabIndex={-1}
              className={styles.item}
              data-danger={danger || undefined}
              onClick={() => {
                close(true);
                onSelect();
              }}
            >
              {Icon && <Icon className={styles.itemIcon} aria-hidden />}
              {itemLabel}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
