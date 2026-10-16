import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Console } from "../../domain/console";
import type { Bindings, ControlsInfo } from "../../domain/controls";
import { getControls, saveControls } from "../../services/controls";
import { ControlsDialog } from "./ControlsDialog";

vi.mock("../../services/controls", () => ({
  getControls: vi.fn(),
  saveControls: vi.fn(),
}));

const getMock = vi.mocked(getControls);
const saveMock = vi.mocked(saveControls);

const SMS: Console = {
  id: "master-system",
  name: "Master System",
  manufacturer: "Sega",
  releaseYear: 1985,
  kind: "home",
  unitsSold: 13_000_000,
  supported: true,
  playable: true,
};

const DEFAULTS: Bindings = {
  up: "ArrowUp",
  down: "ArrowDown",
  left: "ArrowLeft",
  right: "ArrowRight",
  button1: "KeyZ",
  button2: "KeyX",
  pause: "Enter",
};

function info(bindings: Bindings = DEFAULTS): ControlsInfo {
  return {
    actions: ["up", "down", "left", "right", "button1", "button2", "pause"],
    bindings,
    defaults: DEFAULTS,
    supportedKeys: [
      "ArrowUp",
      "ArrowDown",
      "ArrowLeft",
      "ArrowRight",
      "KeyZ",
      "KeyX",
      "KeyJ",
      "Enter",
    ],
  };
}

async function renderDialog(onClose = vi.fn()) {
  render(<ControlsDialog console={SMS} onClose={onClose} />);
  await screen.findByRole("button", { name: "Botão 1: Z" });
  return { onClose };
}

function pressKey(code: string, key = code) {
  act(() => {
    window.dispatchEvent(
      new KeyboardEvent("keydown", { code, key, bubbles: true }),
    );
  });
}

describe("ControlsDialog", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    getMock.mockResolvedValue(info());
    saveMock.mockImplementation(async (_id, bindings) => info(bindings));
  });

  afterEach(cleanup);

  it("mostra as teclas atuais de cada botão do controle", async () => {
    await renderDialog();

    expect(getMock).toHaveBeenCalledWith("master-system");
    for (const name of [
      "Cima: Seta para cima",
      "Baixo: Seta para baixo",
      "Esquerda: Seta para a esquerda",
      "Direita: Seta para a direita",
      "Botão 1: Z",
      "Botão 2: X",
      "Pause: Enter",
    ]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(
      screen.getByRole("button", { name: "Restaurar padrão" }),
    ).toBeDisabled();
  });

  it("clicar numa tecla e pressionar outra troca e salva", async () => {
    await renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Botão 1: Z" }));
    expect(
      screen.getByRole("button", { name: "Botão 1: aguardando tecla" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("status", { name: "" })).toHaveTextContent(
      "Pressione a tecla para Botão 1",
    );

    pressKey("KeyJ");

    expect(saveMock).toHaveBeenCalledWith("master-system", {
      ...DEFAULTS,
      button1: "KeyJ",
    });
    expect(
      await screen.findByRole("button", { name: "Botão 1: J" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Botão 1 agora usa J.")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Restaurar padrão" }),
    ).toBeEnabled();
  });

  it("tecla já usada troca com a outra ação", async () => {
    await renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Botão 1: Z" }));
    pressKey("KeyX");

    expect(saveMock).toHaveBeenCalledWith("master-system", {
      ...DEFAULTS,
      button1: "KeyX",
      button2: "KeyZ",
    });
    expect(
      await screen.findByRole("button", { name: "Botão 2: Z" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Botão 2 ficou com Z/)).toBeInTheDocument();
  });

  it("Esc cancela a troca sem fechar o diálogo", async () => {
    const { onClose } = await renderDialog();

    fireEvent.click(
      screen.getByRole("button", { name: "Cima: Seta para cima" }),
    );
    pressKey("Escape");

    expect(onClose).not.toHaveBeenCalled();
    expect(saveMock).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Cima: Seta para cima" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Troca cancelada.")).toBeInTheDocument();
  });

  it("tecla não suportada é recusada e continua aguardando", async () => {
    await renderDialog();

    fireEvent.click(
      screen.getByRole("button", { name: "Cima: Seta para cima" }),
    );
    pressKey("F1");

    expect(saveMock).not.toHaveBeenCalled();
    expect(
      screen.getByText("A tecla F1 não pode ser usada. Escolha outra."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Cima: aguardando tecla" }),
    ).toBeInTheDocument();
  });

  it("falha ao salvar volta ao mapa anterior e mostra o erro", async () => {
    saveMock.mockRejectedValue("disco cheio");
    await renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Botão 1: Z" }));
    pressKey("KeyJ");

    expect(await screen.findByRole("alert")).toHaveTextContent("disco cheio");
    expect(
      screen.getByRole("button", { name: "Botão 1: Z" }),
    ).toBeInTheDocument();
  });

  it("restaurar padrão salva as teclas originais", async () => {
    getMock.mockResolvedValue(info({ ...DEFAULTS, up: "KeyJ" }));
    render(<ControlsDialog console={SMS} onClose={vi.fn()} />);
    const reset = await screen.findByRole("button", {
      name: "Restaurar padrão",
    });
    await waitFor(() => expect(reset).toBeEnabled());

    fireEvent.click(reset);

    expect(saveMock).toHaveBeenCalledWith("master-system", DEFAULTS);
    expect(
      await screen.findByText("Controles restaurados para o padrão."),
    ).toBeInTheDocument();
    expect(reset).toBeDisabled();
  });

  it("erro ao carregar permite tentar de novo", async () => {
    getMock.mockRejectedValueOnce("sem acesso");
    render(<ControlsDialog console={SMS} onClose={vi.fn()} />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Tentar novamente" }),
    );

    expect(
      await screen.findByRole("button", { name: "Botão 1: Z" }),
    ).toBeInTheDocument();
    expect(getMock).toHaveBeenCalledTimes(2);
  });

  it("fecha pelo botão Fechar", async () => {
    const { onClose } = await renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Fechar" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("mega drive mostra os 6 botões, Start e Mode", async () => {
    const genesis: Bindings = {
      up: "ArrowUp",
      down: "ArrowDown",
      left: "ArrowLeft",
      right: "ArrowRight",
      a: "KeyZ",
      b: "KeyX",
      c: "KeyC",
      x: "KeyA",
      y: "KeyS",
      z: "KeyD",
      start: "Enter",
      mode: "ShiftRight",
    };
    getMock.mockResolvedValue({
      actions: [
        "up",
        "down",
        "left",
        "right",
        "a",
        "b",
        "c",
        "x",
        "y",
        "z",
        "start",
        "mode",
      ],
      bindings: genesis,
      defaults: genesis,
      supportedKeys: [
        "KeyZ",
        "KeyX",
        "KeyC",
        "KeyA",
        "KeyS",
        "KeyD",
        "KeyJ",
        "Enter",
        "ShiftRight",
      ],
    });
    render(
      <ControlsDialog
        console={{ ...SMS, id: "mega-drive", name: "Mega Drive" }}
        onClose={vi.fn()}
      />,
    );

    await screen.findByRole("button", { name: "A: Z" });
    for (const name of [
      "B: X",
      "C: C",
      "X: A",
      "Y: S",
      "Z: D",
      "Start: Enter",
      "Mode: Shift direito",
    ]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(
      screen.queryByRole("button", { name: /Pause/ }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "C: C" }));
    pressKey("KeyJ");
    expect(saveMock).toHaveBeenCalledWith("mega-drive", {
      ...genesis,
      c: "KeyJ",
    });
  });
});
