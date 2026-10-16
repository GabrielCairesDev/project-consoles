import type { Console } from "../../domain/console";
import { consoleLogoUrl } from "./consoleLogos";
import styles from "./ConsoleLogo.module.css";

/** Logo centralizado e contido na caixa do pai; sem logo, mostra o nome. */
export function ConsoleLogo({ console }: { console: Console }) {
  const src = consoleLogoUrl(console.id);

  if (!src) {
    return <span className={styles.fallback}>{console.name}</span>;
  }

  return (
    <img
      className={styles.logo}
      src={src}
      alt={console.name}
      title={console.name}
      loading="lazy"
      decoding="async"
      draggable={false}
    />
  );
}
