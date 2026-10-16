import { invoke } from "@tauri-apps/api/core";
import type { ConsoleSettings } from "../domain/consoleSettings";

export function getConsoleSettings(consoleId: string): Promise<ConsoleSettings> {
  return invoke<ConsoleSettings>("get_console_settings", { consoleId });
}

/** Salva as opções; rejeita com a mensagem do núcleo se não for possível. */
export function saveConsoleSettings(
  consoleId: string,
  settings: ConsoleSettings,
): Promise<ConsoleSettings> {
  return invoke<ConsoleSettings>("set_console_settings", { consoleId, settings });
}
