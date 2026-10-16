import { describe, expect, it } from "vitest";
import {
  type Console,
  decadeLabel,
  groupByDecade,
} from "./console";

function make(id: string, releaseYear: number, unitsSold = 1_000_000): Console {
  return { id, name: id, manufacturer: "X", releaseYear, kind: "home", unitsSold, supported: true };
}

const ids = (consoles: readonly Console[]) => consoles.map((c) => c.id);

describe("groupByDecade", () => {
  it("retorna lista vazia para entrada vazia", () => {
    expect(groupByDecade([])).toEqual([]);
  });

  it("agrupa por década em ordem crescente mesmo com entrada fora de ordem", () => {
    const groups = groupByDecade([make("c", 2001), make("a", 1983), make("b", 1989)]);

    expect(groups.map((g) => g.decade)).toEqual([1980, 2000]);
    expect(ids(groups[0].consoles)).toEqual(["a", "b"]);
  });

  it("trata os limites da década corretamente", () => {
    const groups = groupByDecade([make("fim", 1989), make("inicio", 1990)]);

    expect(groups.map((g) => [g.decade, g.consoles[0].id])).toEqual([
      [1980, "fim"],
      [1990, "inicio"],
    ]);
  });

  it("ordena por ano dentro da década e preserva a ordem entre empates", () => {
    const groups = groupByDecade([make("y", 1985), make("b", 1983), make("a", 1983)]);

    expect(ids(groups[0].consoles)).toEqual(["b", "a", "y"]);
  });

  it("não altera o array recebido", () => {
    const input = [make("y", 1985), make("x", 1983)];
    groupByDecade(input);

    expect(ids(input)).toEqual(["y", "x"]);
  });
});

describe("decadeLabel", () => {
  it("formata a década", () => {
    expect(decadeLabel(1990)).toBe("Anos 1990");
  });
});
