import { useId } from "react";
import type { ControlAction } from "../../domain/controls";
import { type ControllerProps, KeyButton } from "./KeyButton";
import styles from "./Controller.module.css";

/** Centro de cada botão no desenho, em coordenadas do `viewBox` (440×230). */
const VIEW = { width: 440, height: 230 };
const PAD_TARGETS: { action: ControlAction; x: number; y: number }[] = [
  { action: "up", x: 110, y: 78 },
  { action: "down", x: 110, y: 156 },
  { action: "left", x: 71, y: 117 },
  { action: "right", x: 149, y: 117 },
  { action: "start", x: 220, y: 150 },
  { action: "x", x: 286, y: 92 },
  { action: "y", x: 338, y: 84 },
  { action: "z", x: 390, y: 76 },
  { action: "a", x: 286, y: 146 },
  { action: "b", x: 338, y: 138 },
  { action: "c", x: 390, y: 130 },
];

/** Botões frontais, para desenhar a forma (fileira de cima é menor). */
const FACE_BUTTONS = PAD_TARGETS.filter(({ action }) =>
  "abcxyz".includes(action),
).map(({ action, x, y }) => ({
  action,
  x,
  y,
  r: "xyz".includes(action) ? 15 : 20,
}));

/**
 * Controle de 6 botões do Mega Drive em SVG, com a tecla de cada botão por
 * cima. X, Y, Z e Mode só existem no controle de 6 botões; jogos feitos
 * para o de 3 os ignoram. O Mode fica à parte (botão do ombro).
 */
export function GenesisController(props: ControllerProps) {
  const hintId = useId();
  const keyButton = (action: ControlAction) => (
    <KeyButton action={action} hintId={hintId} {...props} />
  );

  return (
    <div className={styles.layout}>
      <p id={hintId} className={styles.hint}>
        Clique numa tecla e pressione a nova.
      </p>

      <div
        className={styles.pad}
        style={{ aspectRatio: `${VIEW.width} / ${VIEW.height}` }}
      >
        <svg
          className={styles.art}
          viewBox={`0 0 ${VIEW.width} ${VIEW.height}`}
          aria-hidden="true"
          focusable="false"
        >
          <path className={styles.cable} d="M220 34 C 222 16, 238 8, 262 4" />
          <path
            className={styles.body}
            d="M60 40 C 120 26, 320 26, 380 40 C 440 54, 436 196, 392 212 C 352 226, 300 204, 262 190 L 178 190 C 140 204, 88 226, 48 212 C 4 196, 0 54, 60 40 Z"
          />
          <circle className={styles.well} cx="110" cy="117" r="58" />
          <rect
            className={styles.cross}
            x="93"
            y="66"
            width="34"
            height="102"
            rx="6"
          />
          <rect
            className={styles.cross}
            x="59"
            y="100"
            width="102"
            height="34"
            rx="6"
          />
          <circle className={styles.well} cx="110" cy="117" r="7" />
          <rect
            className={styles.button}
            x="196"
            y="140"
            width="48"
            height="20"
            rx="10"
          />
          {FACE_BUTTONS.map(({ action, x, y, r }) => (
            <circle
              key={action}
              className={styles.button}
              cx={x}
              cy={y}
              r={r}
            />
          ))}
          <text className={styles.print} x="220" y="176">
            START
          </text>
        </svg>

        {PAD_TARGETS.map(({ action, x, y }) => (
          <div
            key={action}
            className={styles.target}
            style={{
              left: `${(x / VIEW.width) * 100}%`,
              top: `${(y / VIEW.height) * 100}%`,
            }}
          >
            {keyButton(action)}
          </div>
        ))}
      </div>

      <div className={styles.extras}>
        <div className={styles.console}>
          <span className={styles.consoleLabel}>
            Mode <span className={styles.consoleNote}>(ombro do controle)</span>
          </span>
          {keyButton("mode")}
        </div>
        <p className={styles.hint}>
          X, Y, Z e Mode existem só no controle de 6 botões; jogos do controle
          de 3 os ignoram.
        </p>
      </div>
    </div>
  );
}
