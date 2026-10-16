import { describe, expect, it } from "vitest";
import { type Bindings, keyLabel, keyName, rebind, sameBindings } from "./controls";

const DEFAULTS: Bindings = {
  up: "ArrowUp",
  down: "ArrowDown",
  left: "ArrowLeft",
  right: "ArrowRight",
  button1: "KeyZ",
  button2: "KeyX",
  pause: "Enter",
};

describe("keyLabel / keyName", () => {
  it("abrevia letras, dígitos e setas", () => {
    expect(keyLabel("KeyZ")).toBe("Z");
    expect(keyLabel("Digit7")).toBe("7");
    expect(keyLabel("Numpad4")).toBe("Num 4");
    expect(keyLabel("ArrowUp")).toBe("↑");
    expect(keyLabel("Space")).toBe("Espaço");
  });

  it("descreve por extenso para leitores de tela", () => {
    expect(keyName("ArrowLeft")).toBe("Seta para a esquerda");
    expect(keyName("Comma")).toBe("Vírgula");
    expect(keyName("KeyQ")).toBe("Q");
  });
});

describe("rebind", () => {
  it("atribui uma tecla livre sem mexer nas outras", () => {
    const { bindings, swappedWith } = rebind(DEFAULTS, "button1", "KeyJ");
    expect(bindings.button1).toBe("KeyJ");
    expect(swappedWith).toBeNull();
    expect(sameBindings(bindings, { ...DEFAULTS, button1: "KeyJ" })).toBe(true);
  });

  it("troca com a ação que já usava a tecla", () => {
    const { bindings, swappedWith } = rebind(DEFAULTS, "button1", "KeyX");
    expect(bindings.button1).toBe("KeyX");
    expect(bindings.button2).toBe("KeyZ");
    expect(swappedWith).toBe("button2");
  });

  it("reatribuir a mesma tecla não muda nada", () => {
    const { bindings, swappedWith } = rebind(DEFAULTS, "up", "ArrowUp");
    expect(sameBindings(bindings, DEFAULTS)).toBe(true);
    expect(swappedWith).toBeNull();
  });

  it("não altera o objeto original", () => {
    rebind(DEFAULTS, "up", "KeyW");
    expect(DEFAULTS.up).toBe("ArrowUp");
  });
});
