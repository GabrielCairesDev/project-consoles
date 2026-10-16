import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { CoverImage, CoverKind, LibraryScan } from "../domain/game";

/** Pasta da biblioteca configurada, ou `null` se ainda não escolhida. */
export function getLibraryDir(): Promise<string | null> {
  return invoke<string | null>("get_library_dir");
}

/**
 * Abre o seletor de pastas nativo e salva a escolha. Retorna `null` se o
 * usuário cancelar.
 */
export async function chooseLibraryDir(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  if (typeof selected !== "string") return null;
  return invoke<string>("set_library_dir", { path: selected });
}

/** Varre a pasta da biblioteca e identifica os jogos de todos os consoles. */
export function scanLibrary(): Promise<LibraryScan> {
  return invoke<LibraryScan>("scan_library");
}

interface CoverResponse {
  path: string;
  kind: CoverKind;
}

/** Capa de um jogo; `null` quando não há capa nem ícone disponível. */
export async function fetchGameCover(gameId: string): Promise<CoverImage | null> {
  const cover = await invoke<CoverResponse | null>("game_cover", { gameId });
  return cover && { src: convertFileSrc(cover.path), kind: cover.kind };
}
