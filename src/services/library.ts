import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { CoverImage, CoverKind, Game, LibraryScan } from "../domain/game";

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
  /** O jogo tem save próprio (`.sav`). */
  hasGameSave: boolean;
}

/** Apaga o progresso salvo do jogo, para recomeçar do início. */
export function deleteSave(gameId: string): Promise<void> {
  return invoke<void>("delete_save", { gameId });
}

/** `.sav` é o formato da maioria dos emuladores; `.srm` é o do RetroArch. Mesmo conteúdo. */
const SAVE_FILTERS = [{ name: "Save do jogo", extensions: ["sav", "srm"] }];

/**
 * Pergunta onde guardar o save do jogo e o copia para lá. Resolve com `false`
 * se o usuário cancelar.
 */
export async function exportGameSave(game: Game): Promise<boolean> {
  // Caracteres proibidos em nomes de arquivo no Windows (e `/` em todos).
  const name = game.title.replace(/[\\/:*?"<>|]/g, "_");
  const destination = await save({ defaultPath: `${name}.sav`, filters: SAVE_FILTERS });
  if (destination === null) return false;
  await invoke<void>("export_game_save", { gameId: game.id, destination });
  return true;
}

/**
 * Pergunta qual save instalar no jogo; o núcleo confere com o cartucho e
 * recusa sem tocar no save atual. Resolve com `false` se o usuário cancelar.
 */
export async function importGameSave(game: Game): Promise<boolean> {
  const source = await open({ multiple: false, directory: false, filters: SAVE_FILTERS });
  if (typeof source !== "string") return false;
  await invoke<void>("import_game_save", { gameId: game.id, source });
  return true;
}

/** Avisa quando a janela de um jogo fecha. Resolve com a função que cancela a escuta. */
export function onGameExited(callback: (event: GameExited) => void): Promise<UnlistenFn> {
  return listen<GameExited>("game-exited", (event) => callback(event.payload));
}
