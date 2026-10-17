import { useId } from "react";
import { RotateCcw } from "lucide-react";
import { Button } from "../../components/Button";
import { type Choice, ChoiceGroup } from "../../components/ChoiceGroup";
import {
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
        description="Valem a partir do próximo jogo aberto. Original mostra a imagem pixel a pixel."
        variant="segmented"
        choices={presetChoices}
        value={preset}
        disabled={disabled}
        onChange={applyPreset}
      />
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
