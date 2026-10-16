import { useId } from "react";
import type { ControlAction } from "../../domain/controls";
import { type ControllerProps, KeyButton } from "./KeyButton";
import styles from "./Controller.module.css";

/** Centro de cada botão no desenho, em coordenadas do `viewBox` (440×240). */
const VIEW = { width: 440, height: 240 };
const PAD_TARGETS: { action: ControlAction; x: number; y: number }[] = [
  { action: "up", x: 110, y: 66 },
  { action: "down", x: 110, y: 144 },
  { action: "left", x: 71, y: 105 },
  { action: "right", x: 149, y: 105 },
  { action: "b", x: 302, y: 118 },
  { action: "a", x: 370, y: 86 },
  { action: "select", x: 186, y: 194 },
  { action: "start", x: 254, y: 194 },
];

/**
 * Metade de baixo do Game Boy clássico em SVG (cruz, A/B na diagonal e
 * Select/Start inclinados), com a tecla de cada botão por cima.
 */
export function GameBoyController(props: ControllerProps) {
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
          {/* Canto inferior direito arredondado, como no aparelho. */}
          <path
            className={styles.body}
            d="M28 12 H412 A16 16 0 0 1 428 28 V160 A68 68 0 0 1 360 228 H28 A16 16 0 0 1 12 212 V28 A16 16 0 0 1 28 12 Z"
          />
          <circle className={styles.well} cx="110" cy="105" r="58" />
          <rect
            className={styles.cross}
            x="93"
            y="54"
            width="34"
            height="102"
            rx="6"
          />
          <rect
            className={styles.cross}
            x="59"
            y="88"
            width="102"
            height="34"
            rx="6"
          />
          <circle className={styles.well} cx="110" cy="105" r="7" />
          <rect
            className={styles.well}
            x="268"
            y="64"
            width="136"
            height="76"
            rx="38"
            transform="rotate(-25 336 102)"
          />
          <circle className={styles.button} cx="302" cy="118" r="24" />
          <circle className={styles.button} cx="370" cy="86" r="24" />
          {[186, 254].map((x) => (
            <rect
              key={x}
              className={styles.button}
              x={x - 22}
              y="186"
              width="44"
              height="14"
              rx="7"
              transform={`rotate(-25 ${x} 193)`}
            />
          ))}
          <text className={styles.print} x="302" y="160">
            B
          </text>
          <text className={styles.print} x="370" y="128">
            A
          </text>
          <text className={styles.print} x="186" y="222">
            SELECT
          </text>
          <text className={styles.print} x="254" y="222">
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
