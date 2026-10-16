import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

/** Espelha `commands::BiosStatus`. */
export interface BiosStatus {
  installed: boolean;
}

/** O BIOS que o console exige (hoje, só o PlayStation). */
export function getBios(consoleId: string): Promise<BiosStatus> {
  return invoke<BiosStatus>("get_bios", { consoleId });
}

/** O BIOS do PlayStation costuma vir como `SCPH1001.BIN` (ou `.rom`). */
const BIOS_FILTERS = [{ name: "BIOS", extensions: ["bin", "rom"] }];

/**
 * Pergunta qual arquivo usar como BIOS; o núcleo confere e guarda uma cópia.
 * Resolve com `null` se o usuário cancelar e rejeita com a mensagem do
 * núcleo se o arquivo for recusado (o BIOS anterior continua).
 */
export async function chooseBios(consoleId: string): Promise<BiosStatus | null> {
  const source = await open({ multiple: false, directory: false, filters: BIOS_FILTERS });
  if (typeof source !== "string") return null;
  return invoke<BiosStatus>("set_bios", { consoleId, source });
}
