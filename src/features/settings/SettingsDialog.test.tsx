import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Console } from "../../domain/console";
import { getConsoleSettings, saveConsoleSettings } from "../../services/consoleSettings";
import { SettingsDialog } from "./SettingsDialog";
import { useConsoleSettings } from "./useConsoleSettings";

vi.mock("../../services/consoleSettings", () => ({
  getConsoleSettings: vi.fn(),
  saveConsoleSettings: vi.fn(),
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
};

function Harness({ onClose = () => {} }: { onClose?: () => void }) {
  const control = useConsoleSettings(SMS.id, true);
  return <SettingsDialog console={SMS} control={control} onClose={onClose} />;
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
});
