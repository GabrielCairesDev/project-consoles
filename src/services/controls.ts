import { invoke } from "@tauri-apps/api/core";
import type { Bindings, ControlsInfo } from "../domain/controls";

/** Teclas configuradas para o console (o padrão, se nunca alteradas). */
export function getControls(consoleId: string): Promise<ControlsInfo> {
  return invoke<ControlsInfo>("get_controls", { consoleId });
}

/** Salva as teclas do console; rejeita com a mensagem do núcleo se inválidas. */
export function saveControls(consoleId: string, bindings: Bindings): Promise<ControlsInfo> {
  return invoke<ControlsInfo>("set_controls", { consoleId, bindings });
}
