import { describe, expect, it } from "vitest";
import { consoleLogoUrl } from "./consoleLogos";

describe("consoleLogoUrl", () => {
  // Regressão: logos acima do limite de inline do Vite (4 KB), como os da
  // Sega e o do Dreamcast, eram servidos como arquivo e não apareciam no app.
  it.each(["dreamcast", "sega-saturn", "master-system", "game-boy-color", "snes", "xbox"])(
    "embute o logo grande %s como data URI de SVG",
    (id) => {
      const url = consoleLogoUrl(id);
      expect(url).toMatch(/^data:image\/svg\+xml;charset=utf-8,%3Csvg/);
      expect(decodeURIComponent(url ?? "")).toContain("viewBox=");
    },
  );

  it("embute também os logos pequenos", () => {
    expect(consoleLogoUrl("playstation")).toMatch(/^data:image\/svg\+xml/);
  });

  it("retorna undefined para console sem logo", () => {
    expect(consoleLogoUrl("inexistente")).toBeUndefined();
  });
});
