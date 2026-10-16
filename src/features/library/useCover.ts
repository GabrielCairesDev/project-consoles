import { type RefObject, useEffect, useState } from "react";
import type { CoverImage } from "../../domain/game";
import { fetchGameCover } from "../../services/library";

export type CoverState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; cover: CoverImage | null };

/**
 * Capas já pedidas nesta sessão, por jogo. Guarda a promessa: o mesmo jogo
 * nunca dispara duas requisições, mesmo remontando o quadro.
 */
const cache = new Map<string, Promise<CoverImage | null>>();

/** Limpa o cache em memória (usado ao atualizar a pasta e nos testes). */
export function clearCoverCache() {
  cache.clear();
}

function requestCover(gameId: string): Promise<CoverImage | null> {
  let pending = cache.get(gameId);
  if (!pending) {
    pending = fetchGameCover(gameId).catch((error: unknown) => {
      console.error("Falha ao obter capa", error);
      // Erro não fica em cache: a próxima exibição tenta de novo.
      cache.delete(gameId);
      return null;
    });
    cache.set(gameId, pending);
  }
  return pending;
}

/** Busca a capa só quando o quadro entra na área visível. */
export function useCover(gameId: string, target: RefObject<Element | null>): CoverState {
  const [state, setState] = useState<CoverState>({ status: "idle" });

  useEffect(() => {
    const element = target.current;
    if (!element) return;
    let active = true;

    const load = () => {
      setState({ status: "loading" });
      requestCover(gameId).then((cover) => {
        if (active) setState({ status: "ready", cover });
      });
    };

    // Sem IntersectionObserver (ambientes antigos ou de teste), carrega já.
    if (typeof IntersectionObserver === "undefined") {
      load();
      return () => {
        active = false;
      };
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          observer.disconnect();
          load();
        }
      },
      { rootMargin: "200px" },
    );
    observer.observe(element);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [gameId, target]);

  return state;
}
