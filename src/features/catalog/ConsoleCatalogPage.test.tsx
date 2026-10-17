import { StrictMode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Console } from "../../domain/console";
import { fetchConsoles } from "../../services/consoles";
import { ConsoleCatalogPage } from "./ConsoleCatalogPage";

vi.mock("../../services/consoles", () => ({ fetchConsoles: vi.fn() }));

const fetchMock = vi.mocked(fetchConsoles);
const noop = () => {};

const CONSOLES: Console[] = [
  {
    id: "master-system",
    name: "Master System",
    manufacturer: "Sega",
    releaseYear: 1985,
    kind: "home",
    unitsSold: 13_000_000,
    supported: true,
    playable: false,
    needsBios: false,
    resumable: true,
    enhancement: null,
  },
  {
    id: "game-boy",
    name: "Game Boy",
    manufacturer: "Nintendo",
    releaseYear: 1989,
    kind: "handheld",
    unitsSold: 118_690_000,
    supported: true,
    playable: false,
    needsBios: false,
    resumable: true,
    enhancement: null,
  },
  {
    id: "playstation-2",
    name: "PlayStation 2",
    manufacturer: "Sony",
    releaseYear: 2000,
    kind: "home",
    unitsSold: 160_000_000,
    supported: false,
    playable: false,
    needsBios: false,
    resumable: true,
    enhancement: null,
  },
];

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** Nomes acessíveis dos logos da grade, na ordem exibida. */
function gridOrder(): string[] {
  return screen.getAllByRole("img").map((img) => img.getAttribute("alt") ?? "");
}

describe("ConsoleCatalogPage", () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  it("não exibe cabeçalho visível, só um título para leitores de tela", async () => {
    fetchMock.mockResolvedValue(CONSOLES);
    render(<ConsoleCatalogPage onSelect={noop} />);
    await screen.findByRole("region", { name: "Anos 1980" });

    expect(screen.queryByRole("banner")).toBeNull();
    expect(screen.queryByText(/bem-vindo/i)).toBeNull();
    expect(screen.getByRole("heading", { level: 1, name: "Consoles" })).toBeInTheDocument();
  });

  it("mostra estado de carregamento acessível enquanto aguarda o núcleo", () => {
    fetchMock.mockReturnValue(deferred<Console[]>().promise);
    render(<ConsoleCatalogPage onSelect={noop} />);

    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("aria-busy", "true");
    expect(status).toHaveTextContent("Carregando consoles…");
  });

  it("na grade, cada quadro mostra só o logo, nomeado pelo console", async () => {
    fetchMock.mockResolvedValue(CONSOLES);
    render(<ConsoleCatalogPage onSelect={noop} />);

    const eighties = await screen.findByRole("region", { name: "Anos 1980" });
    const tiles = within(eighties).getAllByRole("listitem");
    expect(tiles).toHaveLength(2);
    for (const tile of tiles) {
      expect(tile.textContent).toBe("");
      const logo = within(tile).getByRole("img");
      // Todo logo é embutido como data URI, sem requisição de arquivo à parte.
      expect(logo.getAttribute("src")).toMatch(/^data:image\/svg\+xml/);
    }
    expect(within(eighties).getByRole("img", { name: "Game Boy" })).toBeInTheDocument();
  });

  it("ordena sempre por ano, sem seletor de ordenação", async () => {
    fetchMock.mockResolvedValue([CONSOLES[2], CONSOLES[0], CONSOLES[1]]);
    render(<ConsoleCatalogPage onSelect={noop} />);
    await screen.findByRole("region", { name: "Anos 1980" });

    expect(gridOrder()).toEqual(["Master System", "Game Boy", "PlayStation 2"]);
    expect(screen.queryByRole("radio")).toBeNull();
  });

  it("agrupa por década sem exibir a contagem de consoles", async () => {
    fetchMock.mockResolvedValue(CONSOLES);
    render(<ConsoleCatalogPage onSelect={noop} />);
    await screen.findByRole("region", { name: "Anos 1980" });

    expect(screen.getByRole("heading", { level: 2, name: "Anos 1980" })).toHaveTextContent(
      /^Anos 1980$/,
    );
    expect(screen.getByRole("region", { name: "Anos 2000" })).toBeInTheDocument();
    expect(screen.queryByText(/\d+ consoles?$/)).toBeNull();
  });




  it("cada quadro é um botão nomeado pelo console que abre seus jogos", async () => {
    fetchMock.mockResolvedValue(CONSOLES);
    const onSelect = vi.fn();
    render(<ConsoleCatalogPage onSelect={onSelect} />);
    await screen.findByRole("region", { name: "Anos 1980" });

    fireEvent.click(screen.getByRole("button", { name: "Game Boy" }));

    expect(onSelect).toHaveBeenCalledWith(CONSOLES[1]);
  });

  it("mostra o nome quando o console não tem logo", async () => {
    fetchMock.mockResolvedValue([{ ...CONSOLES[0], id: "sem-logo", name: "Console Teste" }]);
    render(<ConsoleCatalogPage onSelect={noop} />);
    await screen.findByRole("region", { name: "Anos 1980" });

    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByText("Console Teste")).toBeInTheDocument();
  });

  it("mostra estado vazio quando o catálogo não tem consoles", async () => {
    fetchMock.mockResolvedValue([]);
    render(<ConsoleCatalogPage onSelect={noop} />);

    expect(await screen.findByText("Nenhum console encontrado no catálogo.")).toBeInTheDocument();
  });

  it("mostra erro e recupera ao tentar novamente", async () => {
    fetchMock.mockRejectedValueOnce(new Error("ipc indisponível")).mockResolvedValueOnce(CONSOLES);
    render(<ConsoleCatalogPage onSelect={noop} />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Não foi possível carregar os consoles.");

    fireEvent.click(within(alert).getByRole("button", { name: "Tentar novamente" }));

    expect(await screen.findByRole("region", { name: "Anos 1980" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("descarta a resposta de uma carga anterior que chega atrasada", async () => {
    // StrictMode executa o efeito duas vezes: a primeira carga fica obsoleta.
    const stale = deferred<Console[]>();
    fetchMock.mockReturnValueOnce(stale.promise).mockResolvedValueOnce(CONSOLES);
    render(
      <StrictMode>
        <ConsoleCatalogPage onSelect={noop} />
      </StrictMode>,
    );
    await screen.findByRole("region", { name: "Anos 1980" });

    await act(async () => {
      stale.resolve([]);
      await stale.promise;
    });

    expect(screen.getByRole("region", { name: "Anos 1980" })).toBeInTheDocument();
    expect(screen.queryByText("Nenhum console encontrado no catálogo.")).toBeNull();
  });
});
