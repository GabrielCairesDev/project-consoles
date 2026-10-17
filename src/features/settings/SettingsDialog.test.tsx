import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Console } from "../../domain/console";
import { chooseBios, getBios } from "../../services/bios";
import { getConsoleSettings, saveConsoleSettings } from "../../services/consoleSettings";
import { SettingsDialog } from "./SettingsDialog";
import { useConsoleSettings } from "./useConsoleSettings";

vi.mock("../../services/consoleSettings", () => ({
  getConsoleSettings: vi.fn(),
  saveConsoleSettings: vi.fn(),
}));

vi.mock("../../services/bios", () => ({
  getBios: vi.fn(),
  chooseBios: vi.fn(),
}));

const getMock = vi.mocked(getConsoleSettings);
const saveMock = vi.mocked(saveConsoleSettings);

const SMS: Console = {
  id: "master-system",
  name: "Master System",
  manufacturer: "Sega",
  releaseYear: 1985,
  kind: "home",
  unitsSold: 13_000_000,
  supported: true,
  playable: true,
  needsBios: false,
  resumable: true,
  image: { scalers: ["nearest", "epx"], original: { scaler: "nearest" }, recommended: { scaler: "epx" } },
};

const PS1: Console = {
  ...SMS,
  id: "playstation",
  name: "PlayStation",
  manufacturer: "Sony",
  releaseYear: 1994,
  needsBios: true,
  resumable: false,
  image: { scalers: ["nearest", "bilinear"], original: { scaler: "nearest" }, recommended: { scaler: "bilinear" } },
};

function Harness({
  console = SMS,
  onClose = () => {},
}: {
  console?: Console;
  onClose?: () => void;
}) {
  const control = useConsoleSettings(console.id, true);
  return <SettingsDialog console={console} control={control} onClose={onClose} />;
}

describe("SettingsDialog", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    getMock.mockResolvedValue({ resume: true, image: { scaler: "epx" } });
    saveMock.mockImplementation(async (_id, settings) => settings);
  });

  afterEach(cleanup);

  it("mostra a opção de continuar com a explicação", async () => {
    render(<Harness />);

    const option = await screen.findByRole("switch", { name: "Continuar de onde parou" });
    expect(option).toBeChecked();
    expect(option).toHaveAccessibleDescription(/sempre começa do início/);
    expect(getMock).toHaveBeenCalledWith("master-system");
  });

  it("desligar salva na hora", async () => {
    render(<Harness />);

    fireEvent.click(await screen.findByRole("switch", { name: "Continuar de onde parou" }));

    expect(saveMock).toHaveBeenCalledWith("master-system", { resume: false, image: { scaler: "epx" } });
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Continuar de onde parou" })).not.toBeChecked(),
    );
  });

  it("clicar no texto da opção também alterna", async () => {
    render(<Harness />);
    await screen.findByRole("switch", { name: "Continuar de onde parou" });

    fireEvent.click(screen.getByText("Continuar de onde parou"));

    expect(saveMock).toHaveBeenCalledWith("master-system", { resume: false, image: { scaler: "epx" } });
  });

  it("falha ao salvar volta ao valor anterior e mostra o erro", async () => {
    saveMock.mockRejectedValue("Não foi possível salvar as configurações: disco cheio");
    render(<Harness />);

    fireEvent.click(await screen.findByRole("switch", { name: "Continuar de onde parou" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("disco cheio");
    expect(screen.getByRole("switch", { name: "Continuar de onde parou" })).toBeChecked();
  });

  it("erro ao carregar permite tentar de novo", async () => {
    getMock.mockRejectedValueOnce("sem acesso");
    render(<Harness />);

    fireEvent.click(await screen.findByRole("button", { name: "Tentar novamente" }));

    expect(await screen.findByRole("switch", { name: "Continuar de onde parou" })).toBeChecked();
    expect(getMock).toHaveBeenCalledTimes(2);
  });

  it("console sem BIOS não mostra a seção nem consulta o núcleo", async () => {
    render(<Harness />);
    await screen.findByRole("switch", { name: "Continuar de onde parou" });

    expect(screen.queryByText("BIOS")).not.toBeInTheDocument();
    expect(vi.mocked(getBios)).not.toHaveBeenCalled();
  });

  it("PlayStation sem BIOS pede o arquivo e mostra quando foi instalado", async () => {
    vi.mocked(getBios).mockResolvedValue({ installed: false });
    vi.mocked(chooseBios).mockResolvedValue({ installed: true });
    render(<Harness console={PS1} />);

    expect(await screen.findByText("Nenhum BIOS escolhido")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Escolher BIOS…" }));

    expect(await screen.findByText("BIOS instalado")).toBeInTheDocument();
    expect(vi.mocked(chooseBios)).toHaveBeenCalledWith("playstation");
    expect(screen.getByRole("button", { name: "Trocar BIOS…" })).toBeEnabled();
  });

  it("BIOS recusado mostra o motivo e mantém o anterior", async () => {
    vi.mocked(getBios).mockResolvedValue({ installed: true });
    vi.mocked(chooseBios).mockRejectedValue("BIOS não instalado: o arquivo tem 100 bytes.");
    render(<Harness console={PS1} />);

    fireEvent.click(await screen.findByRole("button", { name: "Trocar BIOS…" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("100 bytes");
    expect(screen.getByText("BIOS instalado")).toBeInTheDocument();
  });

  it("cancelar a escolha do BIOS não muda nada", async () => {
    vi.mocked(getBios).mockResolvedValue({ installed: false });
    vi.mocked(chooseBios).mockResolvedValue(null);
    render(<Harness console={PS1} />);

    fireEvent.click(await screen.findByRole("button", { name: "Escolher BIOS…" }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Escolher BIOS…" })).toBeEnabled(),
    );
    expect(screen.getByText("Nenhum BIOS escolhido")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("sem suporte no núcleo, continuar de onde parou fica desligado e explicado", async () => {
    vi.mocked(getBios).mockResolvedValue({ installed: true });
    render(<Harness console={PS1} />);

    const option = await screen.findByRole("switch", { name: "Continuar de onde parou" });
    expect(option).not.toBeChecked();
    expect(option).toBeDisabled();
    expect(option).toHaveAccessibleDescription(/Ainda não disponível no PlayStation/);
  });

  it("imagem começa no Recomendado, com a ampliação marcada", async () => {
    render(<Harness />);

    const presets = await screen.findByRole("radiogroup", { name: "Predefinição" });
    expect(presets).toHaveAccessibleDescription(/próximo jogo/);
    expect(screen.getByRole("radio", { name: "Recomendado" })).toBeChecked();
    expect(within(presets).getByRole("radio", { name: "Original" })).not.toBeChecked();
    const smooth = screen.getByRole("radio", { name: "Contornos suaves" });
    expect(smooth).toBeChecked();
    expect(smooth).toHaveAccessibleDescription(/pixel art/);
    expect(screen.queryByRole("radio", { name: "Personalizado" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restaurar recomendado" })).toBeDisabled();
  });

  it("preset Original salva na hora e marca a ampliação original", async () => {
    render(<Harness />);
    const presets = await screen.findByRole("radiogroup", { name: "Predefinição" });

    fireEvent.click(within(presets).getByRole("radio", { name: "Original" }));

    expect(saveMock).toHaveBeenCalledWith("master-system", {
      resume: true,
      image: { scaler: "nearest" },
    });
    const scaling = screen.getByRole("radiogroup", { name: "Ampliação" });
    await waitFor(() =>
      expect(within(scaling).getByRole("radio", { name: "Sem filtro" })).toBeChecked(),
    );
    expect(within(presets).getByRole("radio", { name: "Original" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "Continuar de onde parou" })).toBeChecked();
  });

  it("setas do teclado mudam a ampliação como num grupo de rádio", async () => {
    render(<Harness />);
    const scaling = await screen.findByRole("radiogroup", { name: "Ampliação" });
    const smooth = within(scaling).getByRole("radio", { name: "Contornos suaves" });
    const original = within(scaling).getByRole("radio", { name: "Sem filtro" });
    expect(smooth).toHaveAttribute("tabindex", "0");
    expect(original).toHaveAttribute("tabindex", "-1");

    fireEvent.keyDown(smooth, { key: "ArrowDown" });

    expect(saveMock).toHaveBeenCalledWith("master-system", {
      resume: true,
      image: { scaler: "nearest" },
    });
    expect(original).toHaveFocus();
  });

  it("escolhas fora dos presets aparecem como Personalizado e podem ser restauradas", async () => {
    const custom: Console = {
      ...SMS,
      image: { ...SMS.image!, scalers: ["nearest", "epx", "bilinear"] },
    };
    getMock.mockResolvedValue({ resume: true, image: { scaler: "bilinear" } });
    render(<Harness console={custom} />);

    expect(await screen.findByRole("radio", { name: "Personalizado" })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Restaurar recomendado" }));

    expect(saveMock).toHaveBeenCalledWith("master-system", {
      resume: true,
      image: { scaler: "epx" },
    });
    await waitFor(() =>
      expect(screen.queryByRole("radio", { name: "Personalizado" })).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("radio", { name: "Recomendado" })).toBeChecked();
  });

  it("PlayStation oferece só as ampliações dele", async () => {
    vi.mocked(getBios).mockResolvedValue({ installed: true });
    getMock.mockResolvedValue({ resume: true, image: { scaler: "nearest" } });
    render(<Harness console={PS1} />);

    const scaling = await screen.findByRole("radiogroup", { name: "Ampliação" });
    const options = within(scaling).getAllByRole("radio");
    expect(options.map((option) => option.textContent)).toEqual([
      expect.stringMatching(/^Sem filtro/),
      expect.stringMatching(/^Suavizada/),
    ]);
    expect(within(scaling).getByRole("radio", { name: "Suavizada" })).toHaveAccessibleDescription(
      /gráficos 3D/,
    );
    expect(screen.getByRole("button", { name: "Restaurar recomendado" })).toBeEnabled();
  });

  it("falha ao salvar a imagem volta à escolha anterior", async () => {
    saveMock.mockRejectedValue("Não foi possível salvar as configurações: disco cheio");
    render(<Harness />);
    const presets = await screen.findByRole("radiogroup", { name: "Predefinição" });

    fireEvent.click(within(presets).getByRole("radio", { name: "Original" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("disco cheio");
    expect(within(presets).getByRole("radio", { name: "Recomendado" })).toBeChecked();
  });

  it("console sem opções de imagem não mostra a seção", async () => {
    render(<Harness console={{ ...SMS, image: null }} />);
    await screen.findByRole("switch", { name: "Continuar de onde parou" });

    expect(screen.queryByRole("radiogroup", { name: "Predefinição" })).not.toBeInTheDocument();
  });
});
