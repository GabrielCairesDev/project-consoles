import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CoverImage, Game } from "../../domain/game";
import { fetchGameCover } from "../../services/library";
import { GameTile } from "./GameTile";
import { clearCoverCache } from "./useCover";

vi.mock("../../services/library", () => ({ fetchGameCover: vi.fn() }));

const coverMock = vi.mocked(fetchGameCover);

const GAME: Game = {
  id: "a.gba",
  consoleId: "game-boy-advance",
  title: "Advance Wars (USA)",
  fileName: "a.gba",
  recognized: true,
  hasSave: false,
};

const BOXART: CoverImage = { src: "asset://capa.png", kind: "boxart" };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function tileOf(container: HTMLElement): HTMLElement {
  const tile = container.firstElementChild;
  if (!(tile instanceof HTMLElement)) throw new Error("quadro não renderizado");
  return tile;
}

describe("GameTile", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    clearCoverCache();
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  it("mostra o placeholder enquanto a capa é buscada", () => {
    coverMock.mockReturnValue(deferred<CoverImage | null>().promise);
    const { container } = render(<GameTile game={GAME} />);

    const tile = tileOf(container);
    expect(tile).toHaveAttribute("aria-busy", "true");
    expect(tile.querySelector("[aria-hidden='true']")).not.toBeNull();
    expect(screen.queryByText(GAME.title)).toBeNull();
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("mantém o placeholder até a imagem carregar e então a revela", async () => {
    coverMock.mockResolvedValue(BOXART);
    const { container } = render(<GameTile game={GAME} />);

    const img = await screen.findByRole("img", { name: GAME.title });
    expect(tileOf(container)).toHaveAttribute("aria-busy", "true");
    expect(img).not.toHaveAttribute("data-loaded");

    fireEvent.load(img);

    expect(tileOf(container)).not.toHaveAttribute("aria-busy");
    expect(img).toHaveAttribute("data-loaded");
  });

  it("se a imagem falhar ao carregar, mostra o título", async () => {
    coverMock.mockResolvedValue(BOXART);
    const { container } = render(<GameTile game={GAME} />);

    fireEvent.error(await screen.findByRole("img", { name: GAME.title }));

    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByText(GAME.title)).toBeInTheDocument();
    expect(tileOf(container)).not.toHaveAttribute("aria-busy");
  });

  it("sem capa disponível, troca o placeholder pelo título", async () => {
    const pending = deferred<CoverImage | null>();
    coverMock.mockReturnValue(pending.promise);
    const { container } = render(<GameTile game={GAME} />);

    await act(async () => {
      pending.resolve(null);
      await pending.promise;
    });

    expect(screen.getByText(GAME.title)).toBeInTheDocument();
    expect(tileOf(container)).not.toHaveAttribute("aria-busy");
  });
});
