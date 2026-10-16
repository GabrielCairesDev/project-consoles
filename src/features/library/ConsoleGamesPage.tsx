import { useMemo, useState } from "react";
import { ArrowLeft, FolderOpen, Gamepad2, RotateCw, TriangleAlert } from "lucide-react";
import { Button } from "../../components/Button";
import { IconButton } from "../../components/IconButton";
import type { Console } from "../../domain/console";
import type { Game, LibraryScan } from "../../domain/game";
import { ConsoleLogo } from "../catalog/ConsoleLogo";
import { ControlsDialog } from "../controls/ControlsDialog";
import { GameTile } from "./GameTile";
import { useLibrary } from "./useLibrary";
import { useRunningGame } from "./useRunningGame";
import styles from "./ConsoleGamesPage.module.css";

const SKELETON_TILES = 10;

interface ConsoleGamesPageProps {
  console: Console;
  onBack: () => void;
}

/** Jogos de um console encontrados na pasta da biblioteca. */
export function ConsoleGamesPage({ console, onBack }: ConsoleGamesPageProps) {
  const [controlsOpen, setControlsOpen] = useState(false);

  return (
    <main className={styles.page}>
      <h1 className={styles.visuallyHidden}>{console.name}</h1>
      <div className={styles.toolbar}>
        <div className={styles.toolbarActions}>
          <IconButton icon={ArrowLeft} label="Voltar para consoles" onClick={onBack} />
          {console.supported && <LibraryActions />}
          {console.playable && (
            <IconButton
              icon={Gamepad2}
              label="Configurar controles"
              aria-haspopup="dialog"
              onClick={() => setControlsOpen(true)}
            />
          )}
        </div>
        <div className={styles.consoleLogo}>
          <ConsoleLogo console={console} />
        </div>
      </div>

      <div className={styles.content}>
        {console.supported ? (
          <Library console={console} />
        ) : (
          <p className={styles.message}>
            Reconhecimento de jogos ainda não disponível para este console.
          </p>
        )}
      </div>

      {controlsOpen && <ControlsDialog console={console} onClose={() => setControlsOpen(false)} />}
    </main>
  );
}

/** Trocar pasta (tooltip com o caminho atual) e atualizar a biblioteca. */
function LibraryActions() {
  const { state, choose, choosing, refresh } = useLibrary();
  const folder =
    state.status === "scanning" ? state.folder : state.status === "ready" ? state.scan.folder : null;
  if (folder === null) return null;

  const busy = state.status === "scanning";
  return (
    <>
      <IconButton
        icon={FolderOpen}
        label="Trocar pasta"
        tooltip={folder}
        onClick={choose}
        disabled={choosing || busy}
      />
      <IconButton icon={RotateCw} label="Atualizar" onClick={refresh} disabled={busy} />
    </>
  );
}

function Library({ console }: { console: Console }) {
  const { state, choose, choosing, chooseError, refresh } = useLibrary();

  if (state.status === "loading") return <LoadingGrid />;
  if (state.status === "error") {
    return <ErrorPanel message={state.message} onRetry={refresh} />;
  }
  if (state.status === "unset") {
    return (
      <div className={styles.panel}>
        <FolderOpen className={styles.panelIcon} aria-hidden="true" />
        <p className={styles.panelTitle}>Escolha a pasta da sua biblioteca</p>
        <p className={styles.panelText}>
          O app procura nela, e nas subpastas, os jogos de todos os consoles.
        </p>
        <Button icon={FolderOpen} onClick={choose} disabled={choosing}>
          Escolher pasta da biblioteca
        </Button>
        {chooseError && (
          <p className={styles.inlineError} role="alert">
            {chooseError}
          </p>
        )}
      </div>
    );
  }

  return (
    <>
      {chooseError && (
        <p className={styles.inlineError} role="alert">
          {chooseError}
        </p>
      )}
      {state.status === "scanning" ? <LoadingGrid /> : <Games console={console} scan={state.scan} />}
    </>
  );
}

function Games({ console, scan }: { console: Console; scan: LibraryScan }) {
  const games = useMemo(
    () => scan.games.filter((game) => game.consoleId === console.id),
    [scan, console.id],
  );

  if (games.length === 0) {
    return (
      <div className={styles.panel}>
        <FolderOpen className={styles.panelIcon} aria-hidden="true" />
        <p className={styles.panelTitle}>Nenhum jogo de {console.name} encontrado</p>
        <p className={styles.panelText}>
          Coloque as ROMs na pasta da biblioteca (ou em subpastas dela) e clique em Atualizar.
        </p>
      </div>
    );
  }
  if (console.playable) return <PlayableGames games={games} />;
  return (
    <ul className={styles.grid} aria-label="Jogos">
      {games.map((game) => (
        <li key={game.id}>
          <GameTile game={game} />
        </li>
      ))}
    </ul>
  );
}

/** Grade em que cada quadro abre o jogo; um jogo aberto por vez. */
function PlayableGames({ games }: { games: readonly Game[] }) {
  const running = useRunningGame();
  const busy = running.gameId !== null;

  return (
    <>
      {running.error && (
        <p className={styles.inlineError} role="alert">
          {running.error}
        </p>
      )}
      <ul className={styles.grid} aria-label="Jogos">
        {games.map((game) => {
          const current = running.gameId === game.id;
          return (
            <li key={game.id}>
              <GameTile
                game={game}
                play={{
                  status: current ? (running.launching ? "launching" : "running") : "idle",
                  disabled: busy && !current,
                  onPlay: () => running.launch(game.id),
                }}
              />
            </li>
          );
        })}
      </ul>
    </>
  );
}

function LoadingGrid() {
  return (
    <div role="status" aria-busy="true">
      <span className={styles.visuallyHidden}>Carregando jogos…</span>
      <div className={styles.grid} aria-hidden="true">
        {Array.from({ length: SKELETON_TILES }, (_, i) => (
          <div key={i} className={styles.skeleton} />
        ))}
      </div>
    </div>
  );
}

function ErrorPanel({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div className={styles.panel} role="alert">
      <TriangleAlert className={styles.errorIcon} aria-hidden="true" />
      <p className={styles.panelTitle}>{message}</p>
      <Button icon={RotateCw} onClick={onRetry}>
        Tentar novamente
      </Button>
    </div>
  );
}
