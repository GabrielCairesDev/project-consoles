import { useId } from "react";
import { RotateCcw } from "lucide-react";
import { Button } from "../../components/Button";
import { type Choice, ChoiceGroup } from "../../components/ChoiceGroup";
import {
  type AspectRatio,
  type ColorDepth,
  type Fit,
  type Geometry,
  type Resolution,
  type FrameBlend,
  type ScreenEffect,
  type Signal,
  type ImageChoices,
  type ImageInfo,
  type ImagePreset,
  type Palette,
  presetOf,
  type Scaler,
} from "../../domain/image";
import styles from "./SettingsDialog.module.css";

interface ImageSectionProps {
  image: ImageInfo;
  choices: ImageChoices;
  disabled: boolean;
  onChange: (choices: ImageChoices) => void;
}

const HEAVY =
  "Pesada: em cenas cheias pode passar do tempo de um quadro e deixar o jogo lento.";

/** Resolução em que o núcleo desenha o 3D. */
const RESOLUTION_CHOICE: Record<Resolution, Omit<Choice<Resolution>, "value">> = {
  "1x": { label: "Original (1×)", description: "O 3D na resolução do console." },
  "2x": {
    label: "2×",
    description: "Polígonos e texturas duas vezes mais nítidos, com folga no tempo de cada quadro.",
  },
  "3x": { label: "3×", description: "Três vezes mais nítido.", warning: HEAVY },
  "4x": { label: "4×", description: "Quatro vezes mais nítido.", warning: HEAVY },
};

/** Cores do desenho 3D. */
const COLOR_DEPTH_CHOICE: Record<ColorDepth, Omit<Choice<ColorDepth>, "value">> = {
  "15-bit": {
    label: "15 bits, com pontilhado",
    description: "As cores do console, com a trama de pontos que disfarçava a falta de tons.",
  },
  "24-bit": {
    label: "24 bits",
    description: "Degradês lisos, sem a trama de pontos.",
  },
};

/** Precisão dos vértices. */
const GEOMETRY_CHOICE: Record<Geometry, Omit<Choice<Geometry>, "value">> = {
  native: {
    label: "Original",
    description: "Os polígonos tremem um pouco quando se movem, como no console.",
  },
  precise: {
    label: "Precisa (PGXP)",
    description:
      "Polígonos firmes, sem tremer; aparece a partir de 2×. Em raros jogos pode desalinhar peças.",
  },
};

/** Nome e efeito de cada ampliação, em linguagem simples. */
const SCALER_CHOICE: Record<Scaler, Omit<Choice<Scaler>, "value">> = {
  nearest: {
    label: "Sem filtro",
    description: "Os pixels exatamente como o console desenhava, sem filtro.",
  },
  epx: {
    label: "Contornos suaves",
    description: "Arredonda as bordas da pixel art sem borrar nem criar cores (Scale2x).",
  },
  mmpx: {
    label: "Traço preservado",
    description:
      "Reconstrói curvas, diagonais e cantos vivos mantendo detalhes de um pixel e as cores (MMPX).",
  },
  bilinear: {
    label: "Suavizada",
    description: "Suaviza o serrilhado e o pontilhado dos gráficos 3D.",
  },
  "sharp-bilinear": {
    label: "Pixels uniformes",
    description:
      "Todos os pixels do mesmo tamanho em qualquer janela, com só a divisa entre eles suavizada.",
  },
};

/** Nome e aparência de cada paleta. */
const PALETTE_CHOICE: Record<Palette, Omit<Choice<Palette>, "value">> = {
  core: {
    label: "Cinza",
    description: "Tons neutros de cinza, sem imitar nenhuma tela.",
  },
  dmg: {
    label: "Verde original",
    description: "O verde-oliva da tela do Game Boy original.",
  },
  pocket: {
    label: "Game Boy Pocket",
    description: "Os tons neutros e mais escuros da tela do Pocket.",
  },
};

/** Proporção da imagem, em linguagem simples. */
const ASPECT_CHOICE: Record<AspectRatio, Omit<Choice<AspectRatio>, "value">> = {
  square: {
    label: "Pixels quadrados",
    description: "Cada pixel do console vira um quadrado, sem esticar.",
  },
  tv: {
    label: "Tela de TV (4:3)",
    description:
      "A proporção da TV da época: os pixels ficam um pouco mais largos, como o jogo foi pensado para ser visto.",
  },
};

/** Como a imagem ocupa a janela. */
const FIT_CHOICE: Record<Fit, Omit<Choice<Fit>, "value">> = {
  integer: {
    label: "Escala inteira",
    description: "Todos os pixels do mesmo tamanho, com barras pretas em volta.",
  },
  fill: {
    label: "Preencher",
    description: "O maior tamanho que cabe na janela, mantendo a proporção.",
  },
};

/** Simulação da tela. */
const SCREEN_CHOICE: Record<ScreenEffect, Omit<Choice<ScreenEffect>, "value">> = {
  none: { label: "Nenhum", description: "A imagem sem simular a tela." },
  scanlines: {
    label: "Scanlines",
    description: "Linhas escuras entre as linhas do console, como numa TV de tubo.",
  },
  crt: {
    label: "TV de tubo",
    description: "Scanlines e a grade de fósforo vermelho, verde e azul de um tubo de imagem.",
  },
  "lcd-grid": {
    label: "Grade do LCD",
    description: "As divisões finas entre os pixels da tela do portátil.",
  },
};

/** Sinal de vídeo simulado. */
const SIGNAL_CHOICE: Record<Signal, Omit<Choice<Signal>, "value">> = {
  rgb: { label: "RGB", description: "Imagem limpa, como num cabo RGB." },
  composite: {
    label: "Composto",
    description:
      "Mistura levemente os pixels vizinhos, transformando o pontilhado dos jogos em transparência, como numa TV comum.",
  },
};

/** Rastro do LCD. */
const BLEND_CHOICE: Record<FrameBlend, Omit<Choice<FrameBlend>, "value">> = {
  off: { label: "Desligado", description: "Cada quadro aparece sozinho." },
  on: {
    label: "Ligado",
    description:
      "Mistura cada quadro com o anterior, como a tela lenta do portátil: efeitos que piscam viram transparência.",
  },
};

const PRESET_CHOICES: Choice<ImagePreset>[] = [
  { value: "original", label: "Original" },
  { value: "recommended", label: "Recomendado" },
];

const CUSTOM_CHOICE: Choice<ImagePreset> = { value: "custom", label: "Personalizado" };

/**
 * Opções de imagem do console: presets no topo e, abaixo, cada técnica do
 * console por etapa. Mudar uma técnica deixa o preset como Personalizado.
 */
export function ImageSection({ image, choices, disabled, onChange }: ImageSectionProps) {
  const titleId = useId();
  const preset = presetOf(choices, image);
  // "Personalizado" não se escolhe: aparece quando as escolhas não batem
  // com nenhum preset.
  const presetChoices = preset === "custom" ? [...PRESET_CHOICES, CUSTOM_CHOICE] : PRESET_CHOICES;

  function applyPreset(value: ImagePreset) {
    if (value === "original") onChange(image.original);
    if (value === "recommended") onChange(image.recommended);
  }

  return (
    <section className={styles.section} aria-labelledby={titleId}>
      <h3 id={titleId} className={styles.sectionTitle}>
        Imagem
      </h3>
      <ChoiceGroup
        label="Predefinição"
        description="Valem a partir do próximo jogo aberto. Original mostra a imagem pixel a pixel; durante o jogo, a tecla F1 alterna entre ela e a sua escolha."
        variant="segmented"
        choices={presetChoices}
        value={preset}
        disabled={disabled}
        onChange={applyPreset}
      />
      {image.resolutions.length > 1 && (
        <ChoiceGroup
          label="Resolução do 3D"
          choices={image.resolutions.map((value) => ({ value, ...RESOLUTION_CHOICE[value] }))}
          value={choices.resolution ?? null}
          disabled={disabled}
          onChange={(resolution) => onChange({ ...choices, resolution })}
        />
      )}
      {image.colorDepths.length > 1 && (
        <ChoiceGroup
          label="Cor do 3D"
          choices={image.colorDepths.map((value) => ({ value, ...COLOR_DEPTH_CHOICE[value] }))}
          value={choices.colorDepth ?? null}
          disabled={disabled}
          onChange={(colorDepth) => onChange({ ...choices, colorDepth })}
        />
      )}
      {image.geometries.length > 1 && (
        <ChoiceGroup
          label="Geometria"
          choices={image.geometries.map((value) => ({ value, ...GEOMETRY_CHOICE[value] }))}
          value={choices.geometry ?? null}
          disabled={disabled}
          onChange={(geometry) => onChange({ ...choices, geometry })}
        />
      )}
      {image.palettes.length > 1 && (
        <ChoiceGroup
          label="Paleta"
          choices={image.palettes.map((value) => ({ value, ...PALETTE_CHOICE[value] }))}
          value={choices.palette ?? null}
          disabled={disabled}
          onChange={(palette) => onChange({ ...choices, palette })}
        />
      )}
      {image.scalers.length > 1 && (
        <ChoiceGroup
          label="Ampliação"
          choices={image.scalers.map((value) => ({ value, ...SCALER_CHOICE[value] }))}
          value={choices.scaler ?? null}
          disabled={disabled}
          onChange={(scaler) => onChange({ ...choices, scaler })}
        />
      )}
      {image.signals.length > 1 && (
        <ChoiceGroup
          label="Sinal de vídeo"
          choices={image.signals.map((value) => ({ value, ...SIGNAL_CHOICE[value] }))}
          value={choices.signal ?? null}
          disabled={disabled}
          onChange={(signal) => onChange({ ...choices, signal })}
        />
      )}
      {image.blends.length > 1 && (
        <ChoiceGroup
          label="Rastro do LCD"
          choices={image.blends.map((value) => ({ value, ...BLEND_CHOICE[value] }))}
          value={choices.blend ?? null}
          disabled={disabled}
          onChange={(blend) => onChange({ ...choices, blend })}
        />
      )}
      {image.screens.length > 1 && (
        <ChoiceGroup
          label="Efeito de tela"
          choices={image.screens.map((value) => ({ value, ...SCREEN_CHOICE[value] }))}
          value={choices.screen ?? null}
          disabled={disabled}
          onChange={(screen) => onChange({ ...choices, screen })}
        />
      )}
      {image.aspects.length > 1 && (
        <ChoiceGroup
          label="Proporção"
          choices={image.aspects.map((value) => ({ value, ...ASPECT_CHOICE[value] }))}
          value={choices.aspect ?? null}
          disabled={disabled}
          onChange={(aspect) => onChange({ ...choices, aspect })}
        />
      )}
      {image.fits.length > 1 && (
        <ChoiceGroup
          label="Encaixe na janela"
          choices={image.fits.map((value) => ({ value, ...FIT_CHOICE[value] }))}
          value={choices.fit ?? null}
          disabled={disabled}
          onChange={(fit) => onChange({ ...choices, fit })}
        />
      )}
      <div className={styles.actions}>
        <Button
          variant="ghost"
          icon={RotateCcw}
          disabled={disabled || preset === "recommended"}
          onClick={() => onChange(image.recommended)}
        >
          Restaurar recomendado
        </Button>
      </div>
    </section>
  );
}
