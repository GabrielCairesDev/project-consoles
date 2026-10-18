import { describe, expect, it } from "vitest";
import { type Console, sortByRelease } from "./console";

function make(id: string, releaseYear: number, unitsSold = 1_000_000): Console {
  return { id, name: id, manufacturer: "X", releaseYear, kind: "home", unitsSold, supported: true, playable: false, needsBios: false, resumable: true, image: null };
}

const ids = (consoles: readonly Console[]) => consoles.map((c) => c.id);

describe("sortByRelease", () => {
  it("retorna lista vazia para entrada vazia", () => {
    expect(sortByRelease([])).toEqual([]);
  });

  it("ordena por ano mesmo com entrada fora de ordem, sem separar por década", () => {
    const sorted = sortByRelease([make("c", 2001), make("a", 1983), make("b", 1989), make("d", 1990)]);

    expect(ids(sorted)).toEqual(["a", "b", "d", "c"]);
  });

  it("preserva a ordem recebida entre consoles do mesmo ano", () => {
    expect(ids(sortByRelease([make("y", 1985), make("b", 1983), make("a", 1983)]))).toEqual([
      "b",
      "a",
      "y",
    ]);
  });

  it("não altera o array recebido", () => {
    const input = [make("y", 1985), make("x", 1983)];
    sortByRelease(input);

    expect(ids(input)).toEqual(["y", "x"]);
  });
});
