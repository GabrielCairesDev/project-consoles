import { useCallback, useId, useRef, useState } from "react";
import { CircleHelp, Download, History, LoaderCircle, Play, RotateCcw, Upload } from "lucide-react";
import { Menu, type MenuItem } from "../../components/Menu";
import type { Game } from "../../domain/game";
import { useCover } from "./useCover";
import styles from "./GameTile.module.css";

/** Estado de execução do quadro, só em consoles jogáveis. */
export interface GameTilePlay {
  /** `launching` enquanto o jogo é iniciado; `running` com a janela aberta. */
  status: "idle" | "launching" | "running";
  /** Outro jogo está aberto: este não pode ser iniciado. */
  disabled: boolean;
  /** Há progresso salvo: abrir continua de onde parou. */
  hasSave: boolean;
  onPlay: () => void;
  /** Pede para recomeçar do início (o chamador confirma e apaga o progresso). */
  onRestart: () => void;
  /** Há save do próprio jogo: habilita "Exportar save". */
  hasGameSave: boolean;
  onExportSave: () => void;
  onImportSave: () => void;
}

interface GameTileProps {
  game: Game;
  /** Presente quando o console é jogável: o quadro vira um botão "Jogar". */
  play?: GameTilePlay;
}

/**
 * Quadro do jogo: só a capa; sem capa, o título. Enquanto a capa é buscada
 * e decodificada, mostra um placeholder animado no lugar.
 */
export function GameTile({ game, play }: GameTileProps) {
  const ref = useRef<HTMLElement | null>(null);
  // Ref de callback estável: o quadro é <div> ou <button> conforme o console.
  const setRef = useCallback((element: HTMLElement | null) => {
    ref.current = element;
  }, []);
  const statusId = useId();
  const [menuOpen, setMenuOpen] = useState(false);
  const cover = useCover(game.id, ref);
  const image = cover.status === "ready" ? cover.cover : null;
  // Estado do <img> por `src`: uma nova capa recomeça como "carregando".
  const [imageState, setImageState] = useState<{ src: string; status: "loaded" | "failed" }>();
  const imageStatus = image && imageState?.src === image.src ? imageState.status : "loading";

  const showImage = image !== null && imageStatus !== "failed";
  const loading = cover.status !== "ready" || (showImage && imageStatus === "loading");

  const content = (
    <>
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
      {play && play.status !== "idle" && <PlayStatus id={statusId} status={play.status} />}
      {play?.hasSave && play.status === "idle" && (
        <span className={styles.resumeBadge}>
          <History className={styles.badgeIcon} aria-hidden="true" />
          Continuar
        </span>
      )}
    </>
  );

  if (!play) {
    return (
      <div ref={setRef} className={styles.tile} aria-busy={loading || undefined}>
        {content}
      </div>
    );
  }

  const active = play.status !== "idle";
  const showMenu = !active && !play.disabled;
  const menuItems: MenuItem[] = [
    { label: play.hasSave ? "Continuar" : "Jogar", icon: Play, onSelect: play.onPlay },
    ...(play.hasSave
      ? [{ label: "Recomeçar do início", icon: RotateCcw, onSelect: play.onRestart, danger: true }]
      : []),
    {
      label: "Exportar save",
      icon: Upload,
      onSelect: play.onExportSave,
      disabled: !play.hasGameSave,
    },
    { label: "Importar save", icon: Download, onSelect: play.onImportSave },
  ];
  return (
    <div className={styles.slot}>
      <button
        ref={setRef}
        type="button"
        className={`${styles.tile} ${styles.playable}`}
        aria-label={`${play.hasSave ? "Continuar" : "Jogar"} ${game.title}`}
        aria-describedby={active ? statusId : undefined}
        aria-busy={loading || play.status === "launching" || undefined}
        data-active={active || undefined}
        disabled={active || play.disabled}
        onClick={play.onPlay}
        onContextMenu={(event) => {
          if (!showMenu) return;
          event.preventDefault();
          setMenuOpen(true);
        }}
      >
        {content}
      </button>
      {showMenu && (
        <Menu
          className={styles.menu}
          label={`Opções de ${game.title}`}
          open={menuOpen}
          onOpenChange={setMenuOpen}
          items={menuItems}
        />
      )}
    </div>
  );
}

/** Selo de execução: ícone e texto, não só cor. */
function PlayStatus({ id, status }: { id: string; status: "launching" | "running" }) {
  const launching = status === "launching";
  const Icon = launching ? LoaderCircle : Play;
  return (
    <span id={id} className={styles.playStatus}>
      <Icon
        className={launching ? `${styles.badgeIcon} ${styles.spinner}` : styles.badgeIcon}
        aria-hidden="true"
      />
      {launching ? "Iniciando…" : "Em execução"}
    </span>
  );
}
