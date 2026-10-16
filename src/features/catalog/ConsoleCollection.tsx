import type { Console } from "../../domain/console";
import { ConsoleLogo } from "./ConsoleLogo";
import styles from "./ConsoleCollection.module.css";

interface ConsoleCollectionProps {
  consoles: readonly Console[];
  onSelect: (console: Console) => void;
}

/** Grade de consoles: cada quadro mostra só o logo e abre os jogos do console. */
export function ConsoleCollection({ consoles, onSelect }: ConsoleCollectionProps) {
  return (
    <ul className={styles.grid}>
      {consoles.map((item) => (
        <li key={item.id}>
          {/* O nome acessível vem do `alt` do logo. */}
          <button type="button" className={styles.tile} onClick={() => onSelect(item)}>
            <span className={styles.tileLogo}>
              <ConsoleLogo console={item} />
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
