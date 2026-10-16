import type { Console } from "../../domain/console";
import { ConsoleLogo } from "./ConsoleLogo";
import styles from "./ConsoleCollection.module.css";

/** Grade de consoles: cada quadro mostra só o logo, centralizado. */
export function ConsoleCollection({ consoles }: { consoles: readonly Console[] }) {
  return (
    <ul className={styles.grid}>
      {consoles.map((item) => (
        <li key={item.id} className={styles.tile}>
          <div className={styles.tileLogo}>
            <ConsoleLogo console={item} />
          </div>
        </li>
      ))}
    </ul>
  );
}
