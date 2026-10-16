import { useRef } from "react";
import { CircleHelp } from "lucide-react";
import type { Game } from "../../domain/game";
import { useCover } from "./useCover";
import styles from "./GameTile.module.css";

/** Quadro do jogo: só a capa; sem capa, o título. */
export function GameTile({ game }: { game: Game }) {
  const ref = useRef<HTMLDivElement>(null);
  const cover = useCover(game.id, ref);
  const image = cover.status === "ready" ? cover.cover : null;

  return (
    <div ref={ref} className={styles.tile} aria-busy={cover.status === "loading" || undefined}>
      {image ? (
        <img
          className={styles.cover}
          data-kind={image.kind}
          src={image.src}
          alt={game.title}
          title={game.title}
          decoding="async"
          draggable={false}
        />
      ) : (
        <span className={styles.title} data-pending={cover.status !== "ready" || undefined}>
          {game.title}
        </span>
      )}
      {!game.recognized && (
        <span className={styles.badge}>
          <CircleHelp className={styles.badgeIcon} aria-hidden="true" />
          Não reconhecido
        </span>
      )}
    </div>
  );
}
