import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
};

const PS1: Console = {
  ...SMS,
  id: "playstation",
  name: "PlayStation",
  manufacturer: "Sony",
  releaseYear: 1994,
  needsBios: true,
  resumable: false,
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
    getMock.mockResolvedValue({ resume: true });
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

    expect(saveMock).toHaveBeenCalledWith("master-system", { resume: false });
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Continuar de onde parou" })).not.toBeChecked(),
    );
  });

  it("clicar no texto da opção também alterna", async () => {
    render(<Harness />);
    await screen.findByRole("switch");

    fireEvent.click(screen.getByText("Continuar de onde parou"));

    expect(saveMock).toHaveBeenCalledWith("master-system", { resume: false });
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

    expect(await screen.findByRole("switch")).toBeChecked();
    expect(getMock).toHaveBeenCalledTimes(2);
  });

  it("console sem BIOS não mostra a seção nem consulta o núcleo", async () => {
    render(<Harness />);
    await screen.findByRole("switch");

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
});
