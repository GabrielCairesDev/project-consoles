import { describe, expect, it } from "vitest";
import { type ImageInfo, presetOf, sameChoices } from "./image";

const SMS: ImageInfo = {
  aspects: ["square"],
  fits: ["integer"],
  screens: ["none"],
  signals: ["rgb"],
  blends: ["off"],
  palettes: ["core"],
  scalers: ["nearest", "epx"],
  original: { scaler: "nearest" },
  recommended: { scaler: "epx" },
};

describe("presets de imagem", () => {
  it("reconhece Original e Recomendado", () => {
    expect(presetOf({ scaler: "epx" }, SMS)).toBe("recommended");
    expect(presetOf({ scaler: "nearest" }, SMS)).toBe("original");
  });

  it("qualquer outra combinação é Personalizado", () => {
    expect(presetOf({ scaler: "bilinear" }, SMS)).toBe("custom");
    expect(presetOf({}, SMS)).toBe("custom");
  });

  it("compara todas as técnicas", () => {
    expect(sameChoices({ palette: "dmg", scaler: "epx" }, { palette: "core", scaler: "epx" })).toBe(
      false,
    );
  });

  it("compara técnica por técnica", () => {
    expect(sameChoices({ scaler: "epx" }, { scaler: "epx" })).toBe(true);
    expect(sameChoices({ scaler: "epx" }, { scaler: "nearest" })).toBe(false);
  });
});
