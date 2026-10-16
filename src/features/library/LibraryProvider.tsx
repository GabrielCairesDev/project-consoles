import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { chooseLibraryDir, getLibraryDir, scanLibrary } from "../../services/library";
import { clearCoverCache } from "./useCover";
import { LibraryContext, type LibraryState } from "./useLibrary";

function messageOf(error: unknown, fallback: string): string {
  return typeof error === "string" ? error : fallback;
}

/**
 * Biblioteca única do app: lê a pasta configurada e a varre uma vez; todas as
 * telas de console filtram o mesmo resultado.
 */
export function LibraryProvider({ children }: { children: ReactNode }) {
  const [folder, setFolder] = useState<string | null | undefined>(undefined);
  const [state, setState] = useState<LibraryState>({ status: "loading" });
  const [dirRound, setDirRound] = useState(0);
  const [scanRound, setScanRound] = useState(0);
  const [choosing, setChoosing] = useState(false);
  const [chooseError, setChooseError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    getLibraryDir().then(
      (dir) => current && setFolder(dir),
      (error: unknown) => {
        console.error("Falha ao ler a pasta da biblioteca", error);
        if (current) {
          setState({
            status: "error",
            message: messageOf(error, "Não foi possível ler as preferências do app."),
          });
        }
      },
    );
    return () => {
      current = false;
    };
  }, [dirRound]);

  useEffect(() => {
    if (folder === undefined) return;
    if (folder === null) {
      setState({ status: "unset" });
      return;
    }
    // Descarta respostas de varreduras antigas ou após desmontar.
    let current = true;
    setState({ status: "scanning", folder });
    scanLibrary().then(
      (scan) => current && setState({ status: "ready", scan }),
      (error: unknown) => {
        console.error("Falha ao varrer a biblioteca", error);
        if (current) {
          setState({ status: "error", message: messageOf(error, "Não foi possível ler a pasta.") });
        }
      },
    );
    return () => {
      current = false;
    };
  }, [folder, scanRound]);

  const choose = useCallback(async () => {
    setChoosing(true);
    setChooseError(null);
    try {
      const dir = await chooseLibraryDir();
      if (dir) {
        clearCoverCache();
        setFolder(dir);
        setScanRound((n) => n + 1);
      }
    } catch (error) {
      setChooseError(messageOf(error, "Não foi possível usar essa pasta."));
    } finally {
      setChoosing(false);
    }
  }, []);

  // Se a própria leitura da pasta falhou, tentar de novo é relê-la; senão,
  // repetir a varredura.
  const refresh = useCallback(() => {
    if (folder === undefined) {
      setState({ status: "loading" });
      setDirRound((n) => n + 1);
    } else {
      setScanRound((n) => n + 1);
    }
  }, [folder]);

  const value = useMemo(
    () => ({ state, choose, choosing, chooseError, refresh }),
    [state, choose, choosing, chooseError, refresh],
  );
  return <LibraryContext.Provider value={value}>{children}</LibraryContext.Provider>;
}
