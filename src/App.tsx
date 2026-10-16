import { useLayoutEffect, useRef, useState } from "react";
import type { Console } from "./domain/console";
import { ConsoleCatalogPage } from "./features/catalog/ConsoleCatalogPage";
import { ConsoleGamesPage } from "./features/library/ConsoleGamesPage";
import { LibraryProvider } from "./features/library/LibraryProvider";

export default function App() {
  const [selected, setSelected] = useState<Console | null>(null);
  const catalogScroll = useRef(0);

  const open = (console: Console) => {
    catalogScroll.current = window.scrollY;
    setSelected(console);
  };

  // Cada tela começa do topo; ao voltar, o catálogo retoma onde estava.
  useLayoutEffect(() => {
    window.scrollTo(0, selected ? 0 : catalogScroll.current);
  }, [selected]);

  return (
    <LibraryProvider>
      {/* O catálogo só é ocultado, não desmontado: mantém dados e ordenação. */}
      <div hidden={selected !== null}>
        <ConsoleCatalogPage onSelect={open} />
      </div>
      {selected && <ConsoleGamesPage console={selected} onBack={() => setSelected(null)} />}
    </LibraryProvider>
  );
}
