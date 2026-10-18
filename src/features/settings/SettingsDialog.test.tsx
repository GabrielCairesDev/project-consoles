import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Console } from "../../domain/console";
import type { ImageChoices } from "../../domain/image";
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
  image: { aspects: ["square"], fits: ["integer"], screens: ["none"], signals: ["rgb"], blends: ["off"], resolutions: ["1x"], colorDepths: ["15-bit"], geometries: ["native"], textures: ["nearest"], palettes: ["core"], scalers: ["nearest", "epx"], original: { scaler: "nearest" }, recommended: { scaler: "epx" } },
};

const PS1: Console = {
  ...SMS,
  id: "playstation",
  name: "PlayStation",
  manufacturer: "Sony",
  releaseYear: 1994,
  needsBios: true,
  resumable: false,
  image: { aspects: ["square"], fits: ["integer"], screens: ["none"], signals: ["rgb"], blends: ["off"], resolutions: ["1x"], colorDepths: ["15-bit"], geometries: ["native"], textures: ["nearest"], palettes: ["core"], scalers: ["nearest", "bilinear"], original: { scaler: "nearest" }, recommended: { scaler: "bilinear" } },
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
    expect(presets).toHaveAccessibleDescription(/F1 alterna/);
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

  it("PlayStation escolhe resolução, cor e geometria do 3D, com aviso nas pesadas", async () => {
    const recommended: ImageChoices = {
      resolution: "2x",
      colorDepth: "24-bit",
      geometry: "native",
      scaler: "bilinear",
    };
    const playStation: Console = {
      ...SMS,
      id: "playstation",
      name: "PlayStation",
      image: {
        aspects: ["tv"],
        fits: ["integer", "fill"],
        screens: ["none", "scanlines", "crt"],
        signals: ["rgb", "composite"],
        blends: ["off"],
        resolutions: ["1x", "2x", "3x", "4x"],
        colorDepths: ["15-bit", "24-bit"],
        geometries: ["native", "precise"],
        textures: ["nearest", "bilinear"],
        palettes: ["core"],
        scalers: ["nearest", "bilinear", "sharp-bilinear"],
        original: { resolution: "1x", colorDepth: "15-bit", geometry: "native", scaler: "nearest" },
        recommended,
      },
    };
    getMock.mockResolvedValue({ resume: true, image: recommended });
    render(<Harness console={playStation} />);

    const resolution = await screen.findByRole("radiogroup", { name: "Resolução do 3D" });
    expect(within(resolution).getByRole("radio", { name: "2×" })).toBeChecked();
    expect(within(resolution).getByRole("radio", { name: "4×" })).toHaveAccessibleDescription(/Pesada/);
    expect(within(resolution).getByRole("radio", { name: "2×" })).not.toHaveAccessibleDescription(/Pesada/);
    fireEvent.click(within(resolution).getByRole("radio", { name: "4×" }));
    expect(saveMock).toHaveBeenLastCalledWith("playstation", {
      resume: true,
      image: { ...recommended, resolution: "4x" },
    });
    expect(await screen.findByRole("radio", { name: "Personalizado" })).toBeChecked();

    const geometry = screen.getByRole("radiogroup", { name: "Geometria" });
    fireEvent.click(within(geometry).getByRole("radio", { name: "Precisa (PGXP)" }));
    expect(saveMock).toHaveBeenLastCalledWith("playstation", {
      resume: true,
      image: { ...recommended, resolution: "4x", geometry: "precise" },
    });
    expect(
      within(screen.getByRole("radiogroup", { name: "Cor do 3D" })).getByRole("radio", { name: "24 bits" }),
    ).toBeChecked();
    // Espera o save da geometria terminar antes da próxima escolha.
    await waitFor(() =>
      expect(within(geometry).getByRole("radio", { name: "Precisa (PGXP)" })).not.toBeDisabled(),
    );
    const texture = screen.getByRole("radiogroup", { name: "Textura do 3D" });
    fireEvent.click(within(texture).getByRole("radio", { name: "Suavizada" }));
    expect(saveMock).toHaveBeenLastCalledWith("playstation", {
      resume: true,
      image: { ...recommended, resolution: "4x", geometry: "precise", texture: "bilinear" },
    });

    cleanup();
    render(<Harness />);
    await screen.findByRole("radiogroup", { name: "Ampliação" });
    expect(screen.queryByRole("radiogroup", { name: "Resolução do 3D" })).not.toBeInTheDocument();
  });

  it("Game Boy escolhe a paleta, e os outros consoles não mostram a opção", async () => {
    const gameBoy: Console = {
      ...SMS,
      id: "game-boy",
      name: "Game Boy",
      kind: "handheld",
      image: {
        aspects: ["square"],
        fits: ["integer", "fill"],
        screens: ["none", "lcd-grid"],
        signals: ["rgb"],
        blends: ["off", "on"],
        resolutions: ["1x"],
        colorDepths: ["15-bit"],
        geometries: ["native"],
        textures: ["nearest"],
        palettes: ["core", "dmg", "pocket"],
        scalers: ["nearest", "epx", "mmpx", "sharp-bilinear"],
        original: { palette: "core", scaler: "nearest" },
        recommended: { palette: "dmg", scaler: "epx" },
      },
    };
    getMock.mockResolvedValue({ resume: true, image: { palette: "dmg", scaler: "epx" } });
    render(<Harness console={gameBoy} />);

    const palettes = await screen.findByRole("radiogroup", { name: "Paleta" });
    expect(within(palettes).getByRole("radio", { name: "Verde original" })).toBeChecked();
    fireEvent.click(within(palettes).getByRole("radio", { name: "Game Boy Pocket" }));

    expect(saveMock).toHaveBeenCalledWith("game-boy", {
      resume: true,
      image: { palette: "pocket", scaler: "epx" },
    });
    expect(await screen.findByRole("radio", { name: "Personalizado" })).toBeChecked();
    expect(
      within(screen.getByRole("radiogroup", { name: "Ampliação" })).getByRole("radio", {
        name: "Pixels uniformes",
      }),
    ).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Traço preservado" })).toHaveAccessibleDescription(
      /MMPX/,
    );
    const trail = screen.getByRole("radiogroup", { name: "Rastro do LCD" });
    fireEvent.click(within(trail).getByRole("radio", { name: "Ligado" }));
    expect(saveMock).toHaveBeenLastCalledWith("game-boy", {
      resume: true,
      image: { palette: "pocket", scaler: "epx", blend: "on" },
    });
    expect(
      within(screen.getByRole("radiogroup", { name: "Efeito de tela" })).getByRole("radio", {
        name: "Grade do LCD",
      }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Sinal de vídeo" })).not.toBeInTheDocument();

    cleanup();
    render(<Harness />);
    await screen.findByRole("radiogroup", { name: "Ampliação" });
    expect(screen.queryByRole("radiogroup", { name: "Paleta" })).not.toBeInTheDocument();
  });

  it("proporção e encaixe aparecem só com mais de uma opção e salvam na hora", async () => {
    const sms: Console = {
      ...SMS,
      image: {
        ...SMS.image!,
        aspects: ["square", "tv"],
        fits: ["integer", "fill"],
        original: { scaler: "nearest", aspect: "square", fit: "integer" },
        recommended: { scaler: "epx", aspect: "square", fit: "integer" },
      },
    };
    getMock.mockResolvedValue({
      resume: true,
      image: { scaler: "epx", aspect: "square", fit: "integer" },
    });
    render(<Harness console={sms} />);

    const aspect = await screen.findByRole("radiogroup", { name: "Proporção" });
    expect(within(aspect).getByRole("radio", { name: "Pixels quadrados" })).toBeChecked();
    fireEvent.click(within(aspect).getByRole("radio", { name: "Tela de TV (4:3)" }));
    expect(saveMock).toHaveBeenCalledWith("master-system", {
      resume: true,
      image: { scaler: "epx", aspect: "tv", fit: "integer" },
    });
    const fit = screen.getByRole("radiogroup", { name: "Encaixe na janela" });
    await waitFor(() =>
      expect(within(aspect).getByRole("radio", { name: "Tela de TV (4:3)" })).toBeChecked(),
    );
    fireEvent.click(within(fit).getByRole("radio", { name: "Preencher" }));
    expect(saveMock).toHaveBeenLastCalledWith("master-system", {
      resume: true,
      image: { scaler: "epx", aspect: "tv", fit: "fill" },
    });

    cleanup();
    vi.mocked(getBios).mockResolvedValue({ installed: true });
    render(<Harness console={PS1} />);
    await screen.findByRole("radiogroup", { name: "Ampliação" });
    expect(screen.queryByRole("radiogroup", { name: "Proporção" })).not.toBeInTheDocument();
  });

  it("console sem opções de imagem não mostra a seção", async () => {
    render(<Harness console={{ ...SMS, image: null }} />);
    await screen.findByRole("switch", { name: "Continuar de onde parou" });

    expect(screen.queryByRole("radiogroup", { name: "Predefinição" })).not.toBeInTheDocument();
  });
});
