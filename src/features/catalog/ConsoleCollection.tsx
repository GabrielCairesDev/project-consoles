import type { Console } from "../../domain/console";
import { formatGameCount } from "../../domain/game";
import { useGameCounts } from "../library/LibraryContext";
import { ConsoleLogo } from "./ConsoleLogo";
import styles from "./ConsoleCollection.module.css";

interface ConsoleCollectionProps {
  consoles: readonly Console[];
  onSelect: (console: Console) => void;
}

/**
 * Grade de consoles: cada quadro mostra o logo e, quando houver, quantos
 * jogos dele estão na biblioteca. Clicar abre os jogos do console.
 */
export function ConsoleCollection({ consoles, onSelect }: ConsoleCollectionProps) {
  const counts = useGameCounts();

  return (
    <ul className={styles.grid}>
      {consoles.map((item) => {
        const count = counts.get(item.id) ?? 0;
        return (
          <li key={item.id}>
            {/* Sem contagem, o nome acessível vem do `alt` do logo. */}
            <button
              type="button"
              className={styles.tile}
              onClick={() => onSelect(item)}
              aria-label={count > 0 ? `${item.name}, ${formatGameCount(count)}` : undefined}
            >
              <span className={styles.tileLogo}>
                <ConsoleLogo console={item} />
              </span>
              {count > 0 && (
                // Visualmente só o número; o rótulo do botão diz "N jogos".
                <span className={styles.count} aria-hidden="true">
                  {count}
                </span>
              )}
            </button>
          </li>
        );
      })}
    </ul>
  );
}
