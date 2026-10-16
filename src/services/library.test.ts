import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Game } from "../domain/game";
import { exportGameSave, importGameSave } from "./library";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

const GAME: Game = {
  id: "GB/Pokemon Red.gb",
  consoleId: "game-boy",
  title: "Pokemon - Red Version (USA, Europe)",
  fileName: "Pokemon Red.gb",
  recognized: true,
  hasSave: false,
  hasGameSave: true,
};

describe("save do jogo", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });

  it("exporta para o arquivo escolhido, sugerindo .sav com o título", async () => {
    vi.mocked(save).mockResolvedValue("/Backup/red.sav");

    await expect(exportGameSave(GAME)).resolves.toBe(true);

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: `${GAME.title}.sav` }),
    );
    expect(invoke).toHaveBeenCalledWith("export_game_save", {
      gameId: GAME.id,
      destination: "/Backup/red.sav",
    });
  });

  it("o nome sugerido não leva caracteres proibidos em arquivos", async () => {
    vi.mocked(save).mockResolvedValue(null);

    await exportGameSave({ ...GAME, title: 'Sonic: A/B "X"?' });

    expect(save).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "Sonic_ A_B _X__.sav" }));
  });

  it("importa o arquivo escolhido", async () => {
    vi.mocked(open).mockResolvedValue("/Downloads/red.srm");

    await expect(importGameSave(GAME)).resolves.toBe(true);

    expect(open).toHaveBeenCalledWith(expect.objectContaining({ multiple: false, directory: false }));
    expect(invoke).toHaveBeenCalledWith("import_game_save", {
      gameId: GAME.id,
      source: "/Downloads/red.srm",
    });
  });

  it("cancelar o diálogo não chama o núcleo", async () => {
    vi.mocked(save).mockResolvedValue(null);
    vi.mocked(open).mockResolvedValue(null);

    await expect(exportGameSave(GAME)).resolves.toBe(false);
    await expect(importGameSave(GAME)).resolves.toBe(false);

    expect(invoke).not.toHaveBeenCalled();
  });

  it("repassa a recusa do núcleo", async () => {
    vi.mocked(open).mockResolvedValue("/x.sav");
    vi.mocked(invoke).mockRejectedValue("Save não importado: tamanho errado.");

    await expect(importGameSave(GAME)).rejects.toBe("Save não importado: tamanho errado.");
  });

  it("no PlayStation o save é o memory card (.mcr)", async () => {
    const crash: Game = {
      ...GAME,
      id: "PS1/Crash Bandicoot (USA).cue",
      consoleId: "playstation",
      title: "Crash Bandicoot (USA)",
    };
    vi.mocked(save).mockResolvedValue(null);
    vi.mocked(open).mockResolvedValue(null);

    await exportGameSave(crash);
    await importGameSave(crash);

    const filters = [{ name: "Memory card", extensions: ["mcr", "mcd", "srm"] }];
    expect(save).toHaveBeenCalledWith({ defaultPath: "Crash Bandicoot (USA).mcr", filters });
    expect(open).toHaveBeenCalledWith(expect.objectContaining({ filters }));
  });
});
