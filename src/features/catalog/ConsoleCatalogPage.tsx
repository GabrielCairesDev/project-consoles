import { useMemo } from "react";
import { RotateCw, TriangleAlert } from "lucide-react";
import { Button } from "../../components/Button";
import { type Console, decadeLabel, groupByDecade } from "../../domain/console";
import { ConsoleCollection } from "./ConsoleCollection";
import { useConsoles } from "./useConsoles";
import styles from "./ConsoleCatalogPage.module.css";

const SKELETON_TILES = 12;

export function ConsoleCatalogPage({ onSelect }: { onSelect: (console: Console) => void }) {
  const { state, retry } = useConsoles();

  return (
    <main className={styles.page}>
      <h1 className={styles.visuallyHidden}>Consoles</h1>

      <div className={styles.content}>
        {state.status === "loading" && <LoadingState />}
        {state.status === "error" && <ErrorState onRetry={retry} />}
        {state.status === "success" &&
          (state.consoles.length === 0 ? (
            <p className={styles.message}>Nenhum console encontrado no catálogo.</p>
          ) : (
            <Catalog consoles={state.consoles} onSelect={onSelect} />
          ))}
      </div>
    </main>
  );
}

interface CatalogProps {
  consoles: Console[];
  onSelect: (console: Console) => void;
}

/** Consoles sempre em ordem de lançamento, agrupados por década. */
function Catalog({ consoles, onSelect }: CatalogProps) {
  const decades = useMemo(() => groupByDecade(consoles), [consoles]);

  return (
    <div className={styles.decades}>
      {decades.map(({ decade, consoles: items }) => {
        const headingId = `decada-${decade}`;
        return (
          <section key={decade} aria-labelledby={headingId}>
            <h2 id={headingId} className={styles.decadeTitle}>
              {decadeLabel(decade)}
            </h2>
            <ConsoleCollection consoles={items} onSelect={onSelect} />
          </section>
        );
      })}
    </div>
  );
}

function LoadingState() {
  return (
    <div role="status" aria-busy="true">
      <span className={styles.visuallyHidden}>Carregando consoles…</span>
      <div className={styles.skeletonGrid} aria-hidden="true">
        {Array.from({ length: SKELETON_TILES }, (_, i) => (
          <div key={i} className={styles.skeleton} />
        ))}
      </div>
    </div>
  );
}

function ErrorState({ onRetry }: { onRetry: () => void }) {
  return (
    <div className={styles.error} role="alert">
      <TriangleAlert className={styles.errorIcon} aria-hidden="true" />
      <p className={styles.errorTitle}>Não foi possível carregar os consoles.</p>
      <p className={styles.errorText}>Verifique se o aplicativo está em execução e tente novamente.</p>
      <Button icon={RotateCw} onClick={onRetry}>
        Tentar novamente
      </Button>
    </div>
  );
}
