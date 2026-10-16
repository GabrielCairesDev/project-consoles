import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WelcomeScreen } from "./WelcomeScreen";

describe("WelcomeScreen", () => {
  it("exibe o título de boas-vindas como heading principal", () => {
    render(<WelcomeScreen />);

    expect(
      screen.getByRole("heading", {
        level: 1,
        name: "Seja bem-vindo ao project-consoles",
      }),
    ).toBeInTheDocument();
  });

  it("expõe a seção nomeada pelo título para leitores de tela", () => {
    render(<WelcomeScreen />);

    expect(
      screen.getByRole("region", { name: "Seja bem-vindo ao project-consoles" }),
    ).toBeInTheDocument();
  });

  it("esconde o ícone decorativo da árvore de acessibilidade", () => {
    const { container } = render(<WelcomeScreen />);

    const icon = container.querySelector("svg");
    expect(icon?.closest("[aria-hidden='true']")).not.toBeNull();
  });
});
