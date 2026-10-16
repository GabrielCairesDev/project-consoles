import { useRef, useState } from "react";
import { CircleHelp } from "lucide-react";
import type { Game } from "../../domain/game";
import { useCover } from "./useCover";
import styles from "./GameTile.module.css";

/**
 * Quadro do jogo: só a capa; sem capa, o título. Enquanto a capa é buscada
 * e decodificada, mostra um placeholder animado no lugar.
 */
export function GameTile({ game }: { game: Game }) {
  const ref = useRef<HTMLDivElement>(null);
  const cover = useCover(game.id, ref);
  const image = cover.status === "ready" ? cover.cover : null;
  // Estado do <img> por `src`: uma nova capa recomeça como "carregando".
  const [imageState, setImageState] = useState<{ src: string; status: "loaded" | "failed" }>();
  const imageStatus = image && imageState?.src === image.src ? imageState.status : "loading";

  const showImage = image !== null && imageStatus !== "failed";
  const loading = cover.status !== "ready" || (showImage && imageStatus === "loading");

  return (
    <div ref={ref} className={styles.tile} aria-busy={loading || undefined}>
      {loading && <div className={styles.placeholder} aria-hidden="true" />}
      {showImage && (
        <img
          className={styles.cover}
          data-kind={image.kind}
          data-loaded={imageStatus === "loaded" || undefined}
          src={image.src}
          alt={game.title}
          title={game.title}
          decoding="async"
          draggable={false}
          onLoad={() => setImageState({ src: image.src, status: "loaded" })}
          onError={() => setImageState({ src: image.src, status: "failed" })}
        />
      )}
      {!loading && !showImage && <span className={styles.title}>{game.title}</span>}
      {!game.recognized && (
        <span className={styles.badge}>
          <CircleHelp className={styles.badgeIcon} aria-hidden="true" />
          Não reconhecido
        </span>
      )}
    </div>
  );
}
