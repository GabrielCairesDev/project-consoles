import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { Console } from "./domain/console";
import { fetchConsoles } from "./services/consoles";
import { getLibraryDir, scanLibrary } from "./services/library";

vi.mock("./services/consoles", () => ({ fetchConsoles: vi.fn() }));
vi.mock("./services/library", () => ({
  getLibraryDir: vi.fn(),
  chooseLibraryDir: vi.fn(),
  scanLibrary: vi.fn(),
  fetchGameCover: vi.fn(),
}));

const CONSOLES: Console[] = [
  {
    id: "playstation",
    name: "PlayStation",
    manufacturer: "Sony",
    releaseYear: 1994,
    kind: "home",
    unitsSold: 102_490_000,
    supported: false,
  },
];

describe("App", () => {
  beforeEach(() => {
    vi.mocked(fetchConsoles).mockResolvedValue(CONSOLES);
    vi.mocked(getLibraryDir).mockResolvedValue(null);
  });

  it("navega do catálogo para o console e volta mantendo o catálogo", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "PlayStation" }));
    expect(screen.getByRole("heading", { level: 1, name: "PlayStation" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 1, name: "Consoles" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Consoles" }));
    expect(screen.getByRole("heading", { level: 1, name: "Consoles" })).toBeInTheDocument();
    expect(fetchConsoles).toHaveBeenCalledTimes(1);
  });

  it("mostra no card do console quantos jogos ele tem na biblioteca", async () => {
    vi.mocked(fetchConsoles).mockResolvedValue([
      { ...CONSOLES[0], id: "snes", name: "Super Famicom / SNES", supported: true },
      { ...CONSOLES[0], id: "game-boy", name: "Game Boy", supported: true },
      CONSOLES[0],
    ]);
    vi.mocked(getLibraryDir).mockResolvedValue("/Jogos");
    vi.mocked(scanLibrary).mockResolvedValue({
      folder: "/Jogos",
      games: [
        { id: "a", consoleId: "snes", title: "A", fileName: "a", recognized: true },
        { id: "b", consoleId: "snes", title: "B", fileName: "b", recognized: true },
        { id: "c", consoleId: "game-boy", title: "C", fileName: "c", recognized: true },
      ],
    });
    render(<App />);

    const snes = await screen.findByRole("button", { name: "Super Famicom / SNES, 2 jogos" });
    // Visualmente, só o número.
    expect(snes).toHaveTextContent(/^2$/);
    expect(screen.getByRole("button", { name: "Game Boy, 1 jogo" })).toBeInTheDocument();
    // Console sem jogos fica sem contagem.
    expect(screen.getByRole("button", { name: "PlayStation" })).toBeInTheDocument();
  });
});
