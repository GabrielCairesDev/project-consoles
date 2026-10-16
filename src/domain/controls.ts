/** Ações de todos os consoles, com os mesmos ids do núcleo. */
export const CONTROL_ACTIONS = [
  "up",
  "down",
  "left",
  "right",
  "button1",
  "button2",
  "pause",
  "a",
  "b",
  "c",
  "x",
  "y",
  "z",
  "start",
  "mode",
] as const;

export type ControlAction = (typeof CONTROL_ACTIONS)[number];

/**
 * Ação → tecla, com nomes de `KeyboardEvent.code` (ex.: "ArrowUp", "KeyZ").
 * Só as ações do console aparecem.
 */
export type Bindings = Partial<Record<ControlAction, string>>;

export interface ControlsInfo {
  /** Ações do controle deste console, na ordem do núcleo. */
  actions: ControlAction[];
  bindings: Bindings;
  defaults: Bindings;
  /** Teclas que o núcleo aceita; Esc fica reservado para fechar o jogo. */
  supportedKeys: string[];
}

export const ACTION_LABELS: Record<ControlAction, string> = {
  up: "Cima",
  down: "Baixo",
  left: "Esquerda",
  right: "Direita",
  button1: "Botão 1",
  button2: "Botão 2",
  pause: "Pause",
  a: "A",
  b: "B",
  c: "C",
  x: "X",
  y: "Y",
  z: "Z",
  start: "Start",
  mode: "Mode",
};

const SPECIAL_KEYS: Record<string, { label: string; name: string }> = {
  ArrowUp: { label: "↑", name: "Seta para cima" },
  ArrowDown: { label: "↓", name: "Seta para baixo" },
  ArrowLeft: { label: "←", name: "Seta para a esquerda" },
  ArrowRight: { label: "→", name: "Seta para a direita" },
  Enter: { label: "Enter", name: "Enter" },
  NumpadEnter: { label: "Num Enter", name: "Enter do teclado numérico" },
  Space: { label: "Espaço", name: "Espaço" },
  Tab: { label: "Tab", name: "Tab" },
  Backspace: { label: "⌫", name: "Backspace" },
  ShiftLeft: { label: "Shift E", name: "Shift esquerdo" },
  ShiftRight: { label: "Shift D", name: "Shift direito" },
  ControlLeft: { label: "Ctrl E", name: "Ctrl esquerdo" },
  ControlRight: { label: "Ctrl D", name: "Ctrl direito" },
  AltLeft: { label: "Alt E", name: "Alt esquerdo" },
  AltRight: { label: "Alt D", name: "Alt direito" },
  Comma: { label: ",", name: "Vírgula" },
  Period: { label: ".", name: "Ponto" },
  Slash: { label: "/", name: "Barra" },
  Semicolon: { label: ";", name: "Ponto e vírgula" },
  Quote: { label: "'", name: "Aspas" },
  BracketLeft: { label: "[", name: "Colchete esquerdo" },
  BracketRight: { label: "]", name: "Colchete direito" },
  Backslash: { label: "\\", name: "Barra invertida" },
  Minus: { label: "-", name: "Hífen" },
  Equal: { label: "=", name: "Igual" },
};

/** Texto curto da tecla, para o desenho do controle. */
export function keyLabel(code: string): string {
  const special = SPECIAL_KEYS[code];
  if (special) return special.label;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^Numpad\d$/.test(code)) return `Num ${code.slice(6)}`;
  return code;
}

/** Nome da tecla por extenso, para leitores de tela. */
export function keyName(code: string): string {
  return SPECIAL_KEYS[code]?.name ?? keyLabel(code);
}

export interface Rebind {
  bindings: Bindings;
  /** Ação que tinha a tecla e recebeu a antiga da ação alterada. */
  swappedWith: ControlAction | null;
}

/**
 * Atribui `key` a `action`. Se outra ação já usava a tecla, as duas trocam,
 * para nunca haver tecla repetida.
 */
export function rebind(
  bindings: Bindings,
  action: ControlAction,
  key: string,
): Rebind {
  const previous = bindings[action];
  const holder =
    CONTROL_ACTIONS.find((a) => a !== action && bindings[a] === key) ?? null;
  const next: Bindings = { ...bindings, [action]: key };
  if (holder && previous !== undefined) next[holder] = previous;
  return { bindings: next, swappedWith: holder };
}

export function sameBindings(a: Bindings, b: Bindings): boolean {
  return CONTROL_ACTIONS.every((action) => a[action] === b[action]);
}
