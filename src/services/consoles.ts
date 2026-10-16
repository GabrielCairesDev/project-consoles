import { invoke } from "@tauri-apps/api/core";
import type { Console } from "../domain/console";

/** Busca o catálogo de consoles no núcleo Rust (comando `list_consoles`). */
export function fetchConsoles(): Promise<Console[]> {
  return invoke<Console[]>("list_consoles");
}
