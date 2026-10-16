import { describe, expect, it } from "vitest";
import { type Game, countGamesByConsole, formatGameCount } from "./game";

function game(id: string, consoleId: string): Game {
  return { id, consoleId, title: id, fileName: id, recognized: true };
}

describe("countGamesByConsole", () => {
  it("conta os jogos de cada console", () => {
    const counts = countGamesByConsole([
      game("a", "snes"),
      game("b", "snes"),
      game("c", "game-boy"),
    ]);

    expect(counts.get("snes")).toBe(2);
    expect(counts.get("game-boy")).toBe(1);
    expect(counts.get("nintendo-64")).toBeUndefined();
  });

  it("lista vazia não tem contagens", () => {
    expect(countGamesByConsole([]).size).toBe(0);
  });
});

describe("formatGameCount", () => {
  it("usa singular e plural", () => {
    expect(formatGameCount(1)).toBe("1 jogo");
    expect(formatGameCount(2)).toBe("2 jogos");
    expect(formatGameCount(120)).toBe("120 jogos");
  });
});
