import { useId } from "react";
import type { ControlAction } from "../../domain/controls";
import { type ControllerProps, KeyButton } from "./KeyButton";
import styles from "./Controller.module.css";

/** Centro de cada botão no desenho, em coordenadas do `viewBox` (440×230). */
const VIEW = { width: 440, height: 230 };
const PAD_TARGETS: { action: ControlAction; x: number; y: number }[] = [
  { action: "l2", x: 84, y: 16 },
  { action: "l1", x: 140, y: 16 },
  { action: "r1", x: 300, y: 16 },
  { action: "r2", x: 356, y: 16 },
  { action: "up", x: 110, y: 78 },
  { action: "down", x: 110, y: 156 },
  { action: "left", x: 71, y: 117 },
  { action: "right", x: 149, y: 117 },
  { action: "select", x: 194, y: 150 },
  { action: "start", x: 246, y: 150 },
  { action: "triangle", x: 330, y: 78 },
  { action: "square", x: 291, y: 117 },
  { action: "circle", x: 369, y: 117 },
  { action: "cross", x: 330, y: 156 },
];

/** Botões frontais: posição e o símbolo impresso em cada um. */
const FACE = { triangle: [330, 78], square: [291, 117], circle: [369, 117], cross: [330, 156] };
const FACE_RADIUS = 19;

/**
 * Controle digital do PlayStation (SCPH-1080) em SVG, com a tecla de cada
 * botão por cima. Os gatilhos L1, L2, R1 e R2 ficam na borda de cima.
 */
export function PlayStationController(props: ControllerProps) {
  const hintId = useId();

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
          <path className={styles.cable} d="M220 40 C 222 24, 238 14, 262 8" />
          {[60, 116, 276, 332].map((x) => (
            <rect
              key={x}
              className={styles.button}
              x={x}
              y="30"
              width="48"
              height="16"
              rx="6"
            />
          ))}
          <path
            className={styles.body}
            d="M70 44 L 370 44 C 418 44, 438 90, 432 150 C 428 196, 410 222, 384 222 C 356 222, 340 200, 318 180 L 122 180 C 100 200, 84 222, 56 222 C 30 222, 12 196, 8 150 C 2 90, 22 44, 70 44 Z"
          />
          <circle className={styles.well} cx="110" cy="117" r="56" />
          <rect
            className={styles.cross}
            x="94"
            y="68"
            width="32"
            height="98"
            rx="6"
          />
          <rect
            className={styles.cross}
            x="61"
            y="101"
            width="98"
            height="32"
            rx="6"
          />
          <circle className={styles.well} cx="330" cy="117" r="66" />
          {[194, 246].map((x) => (
            <rect
              key={x}
              className={styles.button}
              x={x - 18}
              y="143"
              width="36"
              height="14"
              rx="7"
            />
          ))}
          {Object.values(FACE).map(([x, y]) => (
            <circle
              key={`${x}-${y}`}
              className={styles.button}
              cx={x}
              cy={y}
              r={FACE_RADIUS}
            />
          ))}
          <g className={styles.symbol}>
            <path d="M330 64 L 342 84 L 318 84 Z" />
            <rect x="282" y="108" width="18" height="18" />
            <circle cx="369" cy="117" r="10" />
            <path d="M321 147 L 339 165 M 339 147 L 321 165" />
          </g>
          <text className={styles.print} x="194" y="172">
            SELECT
          </text>
          <text className={styles.print} x="246" y="172">
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
            <KeyButton action={action} hintId={hintId} {...props} />
          </div>
        ))}
      </div>
    </div>
  );
}
