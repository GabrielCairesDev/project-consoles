import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { type Mock, beforeEach, describe, expect, it, vi } from "vitest";
import type { Console } from "../../domain/console";
import type { CoverImage, LibraryScan } from "../../domain/game";
import {
  chooseLibraryDir,
  fetchGameCover,
  type GameExited,
  getLibraryDir,
  launchGame,
  onGameExited,
  scanLibrary,
} from "../../services/library";
import { ConsoleGamesPage } from "./ConsoleGamesPage";
import { LibraryProvider } from "./LibraryProvider";
import { clearCoverCache } from "./useCover";

vi.mock("../../services/library", () => ({
  getLibraryDir: vi.fn(),
  chooseLibraryDir: vi.fn(),
  scanLibrary: vi.fn(),
  fetchGameCover: vi.fn(),
  launchGame: vi.fn(),
  onGameExited: vi.fn(),
}));

const getDirMock = vi.mocked(getLibraryDir);
const chooseMock = vi.mocked(chooseLibraryDir);
const scanMock = vi.mocked(scanLibrary);
const coverMock = vi.mocked(fetchGameCover);
const launchMock = vi.mocked(launchGame);
const exitedMock = vi.mocked(onGameExited);

const GBA: Console = {
  id: "game-boy-advance",
  name: "Game Boy Advance",
  manufacturer: "Nintendo",
  releaseYear: 2001,
  kind: "handheld",
  unitsSold: 81_510_000,
  supported: true,
  playable: false,
};

const SCAN: LibraryScan = {
  folder: "/Jogos",
  games: [
    {
      id: "a.gba",
      consoleId: "game-boy-advance",
      title: "Advance Wars (USA)",
      fileName: "a.gba",
      recognized: true,
    },
    {
      id: "gba/b.zip",
      consoleId: "game-boy-advance",
      title: "Golden Sun (USA)",
      fileName: "b.zip",
      recognized: true,
    },
    {
      id: "c.gba",
      consoleId: "game-boy-advance",
      title: "caseiro",
      fileName: "c.gba",
      recognized: false,
    },
    {
      id: "smw.sfc",
      consoleId: "snes",
      title: "Super Mario World (USA)",
      fileName: "smw.sfc",
      recognized: true,
    },
  ],
};

const BOXART: CoverImage = { src: "asset://capa.png", kind: "boxart" };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function renderPage(console: Console = GBA, onBack = vi.fn()) {
  render(
    <LibraryProvider>
      <ConsoleGamesPage console={console} onBack={onBack} />
    </LibraryProvider>,
  );
  return { onBack };
}

describe("ConsoleGamesPage", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    clearCoverCache();
    vi.spyOn(console, "error").mockImplementation(() => {});
    getDirMock.mockResolvedValue("/Jogos");
    scanMock.mockResolvedValue(SCAN);
    coverMock.mockResolvedValue(null);
  });

  it("console sem suporte mostra aviso e só o botão de voltar", async () => {
    renderPage({ ...GBA, id: "playstation", name: "PlayStation", supported: false });

    expect(screen.queryByRole("button", { name: "Trocar pasta" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Atualizar" })).toBeNull();

    expect(
      screen.getByText("Reconhecimento de jogos ainda não disponível para este console."),
    ).toBeInTheDocument();
  });

  it("botão voltar retorna ao catálogo", () => {
    const { onBack } = renderPage();

    fireEvent.click(screen.getByRole("button", { name: "Voltar para consoles" }));

    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("lista só os jogos do console, vindos da pasta única", async () => {
    renderPage();

    const grade = await screen.findByRole("list", { name: "Jogos" });
    expect(within(grade).getAllByRole("listitem")).toHaveLength(3);
    expect(within(grade).queryByText("Super Mario World (USA)")).toBeNull();
    // O caminho não aparece na tela: só no tooltip do botão de pasta.
    expect(screen.getByRole("button", { name: "Trocar pasta" })).toHaveAccessibleDescription(
      "/Jogos",
    );
    expect(scanMock).toHaveBeenCalledTimes(1);
  });

  it("sem pasta definida, pede para escolher e depois lista os jogos", async () => {
    getDirMock.mockResolvedValue(null);
    chooseMock.mockResolvedValue("/Jogos");
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "Escolher pasta da biblioteca" }));

    expect(await screen.findByRole("list", { name: "Jogos" })).toBeInTheDocument();
    expect(chooseMock).toHaveBeenCalledTimes(1);
    expect(scanMock).toHaveBeenCalledTimes(1);
  });

  it("cancelar a escolha mantém o pedido de pasta", async () => {
    getDirMock.mockResolvedValue(null);
    chooseMock.mockResolvedValue(null);
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "Escolher pasta da biblioteca" }));

    expect(
      await screen.findByRole("button", { name: "Escolher pasta da biblioteca" }),
    ).toBeEnabled();
    expect(scanMock).not.toHaveBeenCalled();
  });

  it("mostra o erro de uma pasta inválida", async () => {
    getDirMock.mockResolvedValue(null);
    chooseMock.mockRejectedValue("A pasta escolhida não existe ou não é uma pasta.");
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "Escolher pasta da biblioteca" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A pasta escolhida não existe ou não é uma pasta.",
    );
  });

  it("trocar a pasta varre a nova pasta", async () => {
    chooseMock.mockResolvedValue("/Outra");
    renderPage();
    await screen.findByRole("list", { name: "Jogos" });

    fireEvent.click(screen.getByRole("button", { name: "Trocar pasta" }));

    await waitFor(() => expect(scanMock).toHaveBeenCalledTimes(2));
    expect(chooseMock).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("list", { name: "Jogos" })).toBeInTheDocument();
  });

  it("mostra carregamento acessível durante a varredura", async () => {
    scanMock.mockReturnValue(deferred<LibraryScan>().promise);
    renderPage();

    // Na fase de varredura a pasta já é conhecida: a barra aparece, desativada.
    expect(await screen.findByRole("button", { name: "Atualizar" })).toBeDisabled();
    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("aria-busy", "true");
    expect(status).toHaveTextContent("Carregando jogos…");
  });

  it("sem jogos do console, explica onde colocar as ROMs", async () => {
    scanMock.mockResolvedValue({ folder: "/Jogos", games: [SCAN.games[3]] });
    renderPage();

    expect(
      await screen.findByText("Nenhum jogo de Game Boy Advance encontrado"),
    ).toBeInTheDocument();
  });

  it("erro na varredura permite tentar de novo", async () => {
    scanMock.mockRejectedValueOnce("Não foi possível ler /Jogos");
    renderPage();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Não foi possível ler /Jogos");

    fireEvent.click(within(alert).getByRole("button", { name: "Tentar novamente" }));

    expect(await screen.findByRole("list", { name: "Jogos" })).toBeInTheDocument();
    expect(scanMock).toHaveBeenCalledTimes(2);
  });

  it("falha ao ler a pasta configurada permite tentar de novo", async () => {
    getDirMock.mockRejectedValueOnce("Preferências ilegíveis");
    renderPage();

    const alert = await screen.findByRole("alert");
    fireEvent.click(within(alert).getByRole("button", { name: "Tentar novamente" }));

    expect(await screen.findByRole("list", { name: "Jogos" })).toBeInTheDocument();
    expect(getDirMock).toHaveBeenCalledTimes(2);
  });

  it("quadro mostra só a capa; sem capa, o título", async () => {
    coverMock.mockImplementation(async (gameId) => (gameId === "a.gba" ? BOXART : null));
    renderPage();

    const capa = await screen.findByRole("img", { name: "Advance Wars (USA)" });
    expect(capa).toHaveAttribute("src", "asset://capa.png");
    expect(capa.closest("li")?.textContent).toBe("");

    const semCapa = await screen.findByText("Golden Sun (USA)");
    expect(semCapa.closest("li")?.querySelector("img")).toBeNull();
  });

  it("ícone da ROM é marcado para ampliação sem borrar", async () => {
    coverMock.mockResolvedValue({ src: "asset://icone.png", kind: "icon" });
    renderPage();

    const grade = await screen.findByRole("list", { name: "Jogos" });
    const icone = await within(grade).findByRole("img", { name: "Advance Wars (USA)" });
    expect(icone).toHaveAttribute("data-kind", "icon");
  });

  it("sinaliza jogos não reconhecidos com texto, não só cor", async () => {
    renderPage();

    const item = (await screen.findByText("caseiro")).closest("li");
    expect(item).toHaveTextContent("Não reconhecido");
    expect(screen.getAllByText("Não reconhecido")).toHaveLength(1);
  });

  it("pede cada capa uma única vez, mesmo ao atualizar", async () => {
    renderPage();
    await screen.findByRole("list", { name: "Jogos" });
    await screen.findByText("Advance Wars (USA)");

    fireEvent.click(screen.getByRole("button", { name: "Atualizar" }));
    await screen.findByRole("list", { name: "Jogos" });

    expect(scanMock).toHaveBeenCalledTimes(2);
    expect(coverMock.mock.calls.filter(([id]) => id === "a.gba")).toHaveLength(1);
  });

  describe("console jogável", () => {
    const SMS: Console = {
      ...GBA,
      id: "master-system",
      name: "Master System",
      releaseYear: 1985,
      kind: "home",
      playable: true,
    };
    const SMS_SCAN: LibraryScan = {
      folder: "/Jogos",
      games: [
        {
          id: "sonic.sms",
          consoleId: "master-system",
          title: "Sonic the Hedgehog (USA)",
          fileName: "sonic.sms",
          recognized: true,
        },
        {
          id: "alex.sms",
          consoleId: "master-system",
          title: "Alex Kidd in Miracle World (USA)",
          fileName: "alex.sms",
          recognized: true,
        },
      ],
    };

    let emitExited: (event: GameExited) => void;
    let unlisten: Mock<() => void>;

    beforeEach(() => {
      scanMock.mockResolvedValue(SMS_SCAN);
      unlisten = vi.fn<() => void>();
      exitedMock.mockImplementation(async (callback) => {
        emitExited = callback;
        return unlisten;
      });
      launchMock.mockResolvedValue(undefined);
    });

    async function renderPlayable() {
      renderPage(SMS);
      await screen.findByRole("list", { name: "Jogos" });
      await waitFor(() => expect(exitedMock).toHaveBeenCalledTimes(1));
    }

    it("quadro é um botão que inicia o jogo", async () => {
      await renderPlayable();

      fireEvent.click(screen.getByRole("button", { name: "Jogar Sonic the Hedgehog (USA)" }));

      expect(launchMock).toHaveBeenCalledWith("sonic.sms");
      expect(launchMock).toHaveBeenCalledTimes(1);
    });

    it("jogo aberto mostra 'Em execução' e desativa os demais", async () => {
      await renderPlayable();

      const sonic = screen.getByRole("button", { name: "Jogar Sonic the Hedgehog (USA)" });
      fireEvent.click(sonic);

      expect(await within(sonic).findByText("Em execução")).toBeInTheDocument();
      expect(sonic).toHaveAccessibleDescription("Em execução");
      expect(sonic).toBeDisabled();
      expect(
        screen.getByRole("button", { name: "Jogar Alex Kidd in Miracle World (USA)" }),
      ).toBeDisabled();
    });

    it("mostra 'Iniciando…' enquanto o núcleo não responde", async () => {
      launchMock.mockReturnValue(deferred<void>().promise);
      await renderPlayable();

      const sonic = screen.getByRole("button", { name: "Jogar Sonic the Hedgehog (USA)" });
      fireEvent.click(sonic);

      expect(sonic).toHaveTextContent("Iniciando…");
      expect(sonic).toHaveAttribute("aria-busy", "true");
    });

    it("game-exited devolve a página ao normal", async () => {
      await renderPlayable();
      const sonic = screen.getByRole("button", { name: "Jogar Sonic the Hedgehog (USA)" });
      fireEvent.click(sonic);
      await within(sonic).findByText("Em execução");

      act(() => emitExited({ gameId: "sonic.sms", code: 0 }));

      expect(screen.queryByText("Em execução")).toBeNull();
      for (const tile of within(screen.getByRole("list", { name: "Jogos" })).getAllByRole(
        "button",
      )) {
        expect(tile).toBeEnabled();
      }
    });

    it("game-exited de outro jogo não altera o jogo aberto", async () => {
      await renderPlayable();
      const sonic = screen.getByRole("button", { name: "Jogar Sonic the Hedgehog (USA)" });
      fireEvent.click(sonic);
      await within(sonic).findByText("Em execução");

      act(() => emitExited({ gameId: "alex.sms", code: null }));

      expect(within(sonic).getByText("Em execução")).toBeInTheDocument();
    });

    it("jogo que fecha antes da resposta do núcleo não fica preso em execução", async () => {
      const launch = deferred<void>();
      launchMock.mockReturnValue(launch.promise);
      await renderPlayable();
      fireEvent.click(screen.getByRole("button", { name: "Jogar Sonic the Hedgehog (USA)" }));

      act(() => emitExited({ gameId: "sonic.sms", code: 1 }));
      await act(async () => launch.resolve());

      expect(screen.queryByText("Em execução")).toBeNull();
      expect(screen.queryByText("Iniciando…")).toBeNull();
    });

    it("mostra o erro ao iniciar e libera os quadros", async () => {
      launchMock.mockRejectedValue("ROM inválida para o Master System.");
      await renderPlayable();

      fireEvent.click(screen.getByRole("button", { name: "Jogar Sonic the Hedgehog (USA)" }));

      expect(await screen.findByRole("alert")).toHaveTextContent(
        "ROM inválida para o Master System.",
      );
      expect(screen.getByRole("button", { name: "Jogar Sonic the Hedgehog (USA)" })).toBeEnabled();
    });

    it("cancela a escuta de game-exited ao sair da página", async () => {
      renderPage(SMS);
      await screen.findByRole("list", { name: "Jogos" });
      await waitFor(() => expect(exitedMock).toHaveBeenCalled());
      // Deixa a promessa do `listen` resolver antes de desmontar.
      await act(async () => {});
      expect(unlisten).not.toHaveBeenCalled();

      cleanup();

      expect(unlisten).toHaveBeenCalledTimes(1);
    });

    it("console não jogável não tem quadros clicáveis", async () => {
      renderPage({ ...SMS, playable: false });

      const grade = await screen.findByRole("list", { name: "Jogos" });
      expect(within(grade).queryAllByRole("button")).toHaveLength(0);
      expect(within(grade).queryByRole("button", { name: /^Jogar / })).toBeNull();
      expect(exitedMock).not.toHaveBeenCalled();
    });
  });
});
