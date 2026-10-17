/**
 * Ampliação do quadro; espelha `player::image::Scaler::id`.
 * `nearest`: original, pixel a pixel; `epx`: contornos arredondados para
 * pixel art (2D); `bilinear`: suavização (3D).
 */
export type Scaler = "nearest" | "epx" | "bilinear";

/** Escolhas de imagem, uma por técnica; espelha `settings::ImageChoices`. */
export interface ImageChoices {
  scaler?: Scaler;
}

/** O que o console oferece em imagem; espelha `commands::ImageInfo`. */
export interface ImageInfo {
  /** Ampliações do console, na ordem de exibição. */
  scalers: Scaler[];
  /** Preset Original: pixel a pixel. */
  original: ImageChoices;
  /** Preset Recomendado do console (o padrão). */
  recommended: ImageChoices;
}
