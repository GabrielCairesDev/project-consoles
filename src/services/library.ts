import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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

/**
 * Abre o jogo em uma janela nativa. Resolve quando o jogo começou; rejeita com
 * a mensagem do núcleo quando não foi possível iniciá-lo.
 */
export function launchGame(gameId: string): Promise<void> {
  return invoke<void>("launch_game", { gameId });
}

/** Payload do evento `game-exited`; `code` é `null` quando o processo foi encerrado por sinal. */
export interface GameExited {
  gameId: string;
  code: number | null;
  /** O jogo deixou progresso salvo para continuar. */
  hasSave: boolean;
}

/** Apaga o progresso salvo do jogo, para recomeçar do início. */
export function deleteSave(gameId: string): Promise<void> {
  return invoke<void>("delete_save", { gameId });
}

/** Avisa quando a janela de um jogo fecha. Resolve com a função que cancela a escuta. */
export function onGameExited(callback: (event: GameExited) => void): Promise<UnlistenFn> {
  return listen<GameExited>("game-exited", (event) => callback(event.payload));
}
