import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { Console } from "./domain/console";
import { fetchConsoles } from "./services/consoles";
import { getLibraryDir } from "./services/library";

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
});
