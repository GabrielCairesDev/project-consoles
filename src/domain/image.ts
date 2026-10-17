/**
 * Ampliação do quadro; espelha `player::image::Scaler::id`.
 * `nearest`: original, pixel a pixel; `epx`: contornos arredondados para
 * pixel art (2D); `bilinear`: suavização (3D); `sharp-bilinear`: pixels
 * iguais em qualquer escala.
 */
export type Scaler = "nearest" | "epx" | "bilinear" | "sharp-bilinear";

/** Cores da tela; espelha `player::image::Palette::id`. */
export type Palette = "core" | "dmg" | "pocket";

/** Escolhas de imagem, uma por técnica; espelha `settings::ImageChoices`. */
export interface ImageChoices {
  palette?: Palette;
  scaler?: Scaler;
}

/** O que o console oferece em imagem; espelha `commands::ImageInfo`. */
export interface ImageInfo {
  /** Paletas do console, na ordem de exibição. */
  palettes: Palette[];
  /** Ampliações do console, na ordem de exibição. */
  scalers: Scaler[];
  /** Preset Original: pixel a pixel. */
  original: ImageChoices;
  /** Preset Recomendado do console (o padrão). */
  recommended: ImageChoices;
}

export type ImagePreset = "original" | "recommended" | "custom";

/** Técnicas de imagem, na ordem do pipeline; cresce com o catálogo. */
const TECHNIQUES = ["palette", "scaler"] as const satisfies readonly (keyof ImageChoices)[];

export function sameChoices(a: ImageChoices, b: ImageChoices): boolean {
  return TECHNIQUES.every((technique) => a[technique] === b[technique]);
}

/** Preset das escolhas atuais; o preset nunca é gravado, só deduzido. */
export function presetOf(choices: ImageChoices, info: ImageInfo): ImagePreset {
  if (sameChoices(choices, info.recommended)) return "recommended";
  if (sameChoices(choices, info.original)) return "original";
  return "custom";
}
