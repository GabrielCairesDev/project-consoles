/**
 * Ampliação do quadro; espelha `player::image::Scaler::id`.
 * `nearest`: original, pixel a pixel; `epx`: contornos arredondados para
 * pixel art (2D); `mmpx`: curvas e cantos da pixel art (2D); `bilinear`: suavização (3D); `sharp-bilinear`: pixels
 * iguais em qualquer escala.
 */
export type Scaler = "nearest" | "epx" | "mmpx" | "bilinear" | "sharp-bilinear";

/** Cores da tela; espelha `player::image::Palette::id`. */
export type Palette = "core" | "dmg" | "pocket";

/** Proporção na tela; espelha `player::image::AspectRatio::id`. */
export type AspectRatio = "square" | "tv";

/** Encaixe na janela; espelha `player::image::Fit::id`. */
export type Fit = "integer" | "fill";

/** Efeito de tela; espelha `player::image::ScreenEffect::id`. */
export type ScreenEffect = "none" | "scanlines" | "crt" | "lcd-grid";

/** Sinal de vídeo; espelha `player::image::Signal::id`. */
export type Signal = "rgb" | "composite";

/** Rastro do LCD; espelha `player::image::FrameBlend::id`. */
export type FrameBlend = "off" | "on";

/** Escolhas de imagem, uma por técnica; espelha `settings::ImageChoices`. */
export interface ImageChoices {
  palette?: Palette;
  scaler?: Scaler;
  aspect?: AspectRatio;
  fit?: Fit;
  screen?: ScreenEffect;
  signal?: Signal;
  blend?: FrameBlend;
}

/** O que o console oferece em imagem; espelha `commands::ImageInfo`. */
export interface ImageInfo {
  /** Paletas do console, na ordem de exibição. */
  palettes: Palette[];
  /** Ampliações do console, na ordem de exibição. */
  scalers: Scaler[];
  /** Proporções do console. */
  aspects: AspectRatio[];
  /** Encaixes na janela. */
  fits: Fit[];
  /** Efeitos de tela do console. */
  screens: ScreenEffect[];
  /** Sinais de vídeo do console. */
  signals: Signal[];
  /** Rastro do LCD (só portáteis). */
  blends: FrameBlend[];
  /** Preset Original: pixel a pixel, na apresentação de sempre do console. */
  original: ImageChoices;
  /** Preset Recomendado do console (o padrão). */
  recommended: ImageChoices;
}

export type ImagePreset = "original" | "recommended" | "custom";

/** Técnicas de imagem, na ordem do pipeline; cresce com o catálogo. */
const TECHNIQUES = ["palette", "signal", "blend", "scaler", "screen", "aspect", "fit"] as const satisfies readonly (keyof ImageChoices)[];

export function sameChoices(a: ImageChoices, b: ImageChoices): boolean {
  return TECHNIQUES.every((technique) => a[technique] === b[technique]);
}

/** Preset das escolhas atuais; o preset nunca é gravado, só deduzido. */
export function presetOf(choices: ImageChoices, info: ImageInfo): ImagePreset {
  if (sameChoices(choices, info.recommended)) return "recommended";
  if (sameChoices(choices, info.original)) return "original";
  return "custom";
}
