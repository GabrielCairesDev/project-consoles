import { useMemo, useState } from "react";
import { RotateCw, TriangleAlert } from "lucide-react";
import { SegmentedControl, type SegmentedOption } from "../../components/SegmentedControl";
import {
  type Console,
  type SortOrder,
  decadeLabel,
  groupByDecade,
  sortConsoles,
} from "../../domain/console";
import { ConsoleCollection } from "./ConsoleCollection";
import { useConsoles } from "./useConsoles";
import styles from "./ConsoleCatalogPage.module.css";

const SKELETON_TILES = 12;

const SORT_OPTIONS: readonly SegmentedOption<SortOrder>[] = [
  { value: "year", label: "Ano" },
  { value: "popularity", label: "Popularidade" },
];

export function ConsoleCatalogPage() {
  const { state, retry } = useConsoles();
  const [sort, setSort] = useState<SortOrder>("year");

  return (
    <main className={styles.page}>
      <h1 className={styles.visuallyHidden}>Consoles</h1>

      <div className={styles.toolbar}>
        <SegmentedControl label="Ordenar" options={SORT_OPTIONS} value={sort} onChange={setSort} />
      </div>

      <div className={styles.content}>
        {state.status === "loading" && <LoadingState />}
        {state.status === "error" && <ErrorState onRetry={retry} />}
        {state.status === "success" &&
          (state.consoles.length === 0 ? (
            <p className={styles.message}>Nenhum console encontrado no catálogo.</p>
          ) : (
            <Catalog consoles={state.consoles} sort={sort} />
          ))}
      </div>
    </main>
  );
}

interface CatalogProps {
  consoles: Console[];
  sort: SortOrder;
}

function Catalog({ consoles, sort }: CatalogProps) {
  const sorted = useMemo(() => sortConsoles(consoles, sort), [consoles, sort]);
  const decades = useMemo(() => (sort === "year" ? groupByDecade(sorted) : null), [sorted, sort]);

  // Por popularidade, a posição no ranking importa mais que a década.
  if (!decades) {
    return (
      <section aria-label="Consoles por popularidade">
        <ConsoleCollection consoles={sorted} />
      </section>
    );
  }

  return (
    <div className={styles.decades}>
      {decades.map(({ decade, consoles: items }) => {
        const headingId = `decada-${decade}`;
        return (
          <section key={decade} aria-labelledby={headingId}>
            <h2 id={headingId} className={styles.decadeTitle}>
              {decadeLabel(decade)}
            </h2>
            <ConsoleCollection consoles={items} />
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
      <button type="button" className={styles.button} onClick={onRetry}>
        <RotateCw className={styles.buttonIcon} aria-hidden="true" />
        Tentar novamente
      </button>
    </div>
  );
}
