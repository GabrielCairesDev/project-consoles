import { useId } from "react";
import {
  ACTION_LABELS,
  type Bindings,
  type ControlAction,
  keyLabel,
  keyName,
} from "../../domain/controls";
import styles from "./SmsController.module.css";

/** Centro de cada botão no desenho, em coordenadas do `viewBox` (440×220). */
const VIEW = { width: 440, height: 220 };
const PAD_TARGETS: { action: Exclude<ControlAction, "pause">; x: number; y: number }[] = [
  { action: "up", x: 105, y: 82 },
  { action: "down", x: 105, y: 148 },
  { action: "left", x: 72, y: 115 },
  { action: "right", x: 138, y: 115 },
  { action: "button1", x: 288, y: 118 },
  { action: "button2", x: 364, y: 118 },
];

interface SmsControllerProps {
  bindings: Bindings;
  capturing: ControlAction | null;
  disabled: boolean;
  onSelect: (action: ControlAction) => void;
}

/**
 * Controle do Master System desenhado em SVG, com a tecla de cada botão por
 * cima. Clicar numa tecla começa a troca. O Pause fica à parte: no
 * aparelho original ele é um botão do console, não do controle.
 */
export function SmsController({ bindings, capturing, disabled, onSelect }: SmsControllerProps) {
  const hintId = useId();

  const keyButton = (action: ControlAction) => {
    const waiting = capturing === action;
    const label = ACTION_LABELS[action];
    return (
      <button
        type="button"
        className={styles.key}
        data-capturing={waiting || undefined}
        aria-label={waiting ? `${label}: aguardando tecla` : `${label}: ${keyName(bindings[action])}`}
        aria-describedby={hintId}
        aria-pressed={waiting}
        disabled={disabled}
        onClick={() => onSelect(action)}
      >
        {waiting ? "…" : keyLabel(bindings[action])}
      </button>
    );
  };

  return (
    <div className={styles.layout}>
      <p id={hintId} className={styles.hint}>
        Clique numa tecla e pressione a nova.
      </p>

      <div className={styles.pad}>
        <svg
          className={styles.art}
          viewBox={`0 0 ${VIEW.width} ${VIEW.height}`}
          aria-hidden="true"
          focusable="false"
        >
          <path className={styles.cable} d="M220 32 C 222 14, 238 6, 262 2" />
          <rect className={styles.body} x="12" y="30" width="416" height="170" rx="22" />
          <rect className={styles.bezel} x="26" y="44" width="388" height="142" rx="14" />
          <rect className={styles.well} x="50" y="60" width="110" height="110" rx="18" />
          <rect className={styles.cross} x="88" y="66" width="34" height="98" rx="6" />
          <rect className={styles.cross} x="56" y="98" width="98" height="34" rx="6" />
          <circle className={styles.well} cx="105" cy="115" r="7" />
          <rect className={styles.button} x="262" y="92" width="52" height="52" rx="10" />
          <rect className={styles.button} x="338" y="92" width="52" height="52" rx="10" />
          <text className={styles.print} x="288" y="168">
            1 START
          </text>
          <text className={styles.print} x="364" y="168">
            2
          </text>
        </svg>

        {PAD_TARGETS.map(({ action, x, y }) => (
          <div
            key={action}
            className={styles.target}
            style={{ left: `${(x / VIEW.width) * 100}%`, top: `${(y / VIEW.height) * 100}%` }}
          >
            {keyButton(action)}
          </div>
        ))}
      </div>

      <div className={styles.console}>
        <span className={styles.consoleLabel}>
          Pause <span className={styles.consoleNote}>(botão do console)</span>
        </span>
        {keyButton("pause")}
      </div>
    </div>
  );
}
