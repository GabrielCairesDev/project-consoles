import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Menu, type MenuItem } from "./Menu";

function Harness({ items }: { items: MenuItem[] }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Menu label="Opções" items={items} open={open} onOpenChange={setOpen} />
      <button type="button">fora</button>
    </>
  );
}

const items = (first = vi.fn(), second = vi.fn()): MenuItem[] => [
  { label: "Primeiro", onSelect: first },
  { label: "Segundo", onSelect: second, danger: true },
];

describe("Menu", () => {
  afterEach(cleanup);

  it("abre com foco no primeiro item e navega pelas setas", () => {
    render(<Harness items={items()} />);
    const trigger = screen.getByRole("button", { name: "Opções" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(trigger);

    expect(trigger).toHaveAttribute("aria-expanded", "true");
    const [first, second] = screen.getAllByRole("menuitem");
    expect(first).toHaveFocus();
    fireEvent.keyDown(first!, { key: "ArrowDown" });
    expect(second).toHaveFocus();
    fireEvent.keyDown(second!, { key: "ArrowDown" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first!, { key: "ArrowUp" });
    expect(second).toHaveFocus();
  });

  it("Esc fecha e devolve o foco ao botão", () => {
    render(<Harness items={items()} />);
    const trigger = screen.getByRole("button", { name: "Opções" });
    fireEvent.click(trigger);

    fireEvent.keyDown(screen.getAllByRole("menuitem")[0]!, { key: "Escape" });

    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("escolher um item fecha o menu e executa a ação", () => {
    const second = vi.fn();
    render(<Harness items={items(vi.fn(), second)} />);
    fireEvent.click(screen.getByRole("button", { name: "Opções" }));

    fireEvent.click(screen.getByRole("menuitem", { name: "Segundo" }));

    expect(second).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("clique fora fecha sem executar nada", () => {
    const first = vi.fn();
    render(<Harness items={items(first)} />);
    fireEvent.click(screen.getByRole("button", { name: "Opções" }));

    fireEvent.pointerDown(screen.getByRole("button", { name: "fora" }));

    expect(screen.queryByRole("menu")).toBeNull();
    expect(first).not.toHaveBeenCalled();
  });

  it("item desativado é anunciado, continua focável e não executa", () => {
    const onSelect = vi.fn();
    render(<Harness items={[{ label: "Exportar", onSelect, disabled: true }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Opções" }));

    const item = screen.getByRole("menuitem", { name: "Exportar" });
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveFocus();
    fireEvent.click(item);

    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });
});
