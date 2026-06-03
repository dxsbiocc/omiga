import { beforeEach, describe, expect, it } from "vitest";
import { useUiStore } from "./uiStore";

describe("uiStore", () => {
  beforeEach(() => {
    useUiStore.setState({
      leftPanelCollapsed: false,
      leftPanelWidth: 260,
      rightPanelCollapsed: false,
      rightPanelWidth: 300,
      terminalPanelOpen: false,
    });
  });

  it("toggles the left sidebar without losing the user's expanded width", () => {
    const store = useUiStore.getState();

    store.setLeftWidth(360);
    store.setLeftPanelCollapsed(true);

    expect(useUiStore.getState().leftPanelCollapsed).toBe(true);
    expect(useUiStore.getState().leftPanelWidth).toBe(360);

    useUiStore.getState().toggleLeftPanelCollapsed();

    expect(useUiStore.getState().leftPanelCollapsed).toBe(false);
    expect(useUiStore.getState().leftPanelWidth).toBe(360);
  });

  it("toggles the right sidebar without losing the user's expanded width", () => {
    const store = useUiStore.getState();

    store.setRightWidth(420);
    store.setRightPanelCollapsed(true);

    expect(useUiStore.getState().rightPanelCollapsed).toBe(true);
    expect(useUiStore.getState().rightPanelWidth).toBe(420);

    useUiStore.getState().toggleRightPanelCollapsed();

    expect(useUiStore.getState().rightPanelCollapsed).toBe(false);
    expect(useUiStore.getState().rightPanelWidth).toBe(420);
  });

  it("toggles the embedded terminal panel independently from sidebars", () => {
    const store = useUiStore.getState();

    store.setTerminalPanelOpen(true);

    expect(useUiStore.getState().terminalPanelOpen).toBe(true);
    expect(useUiStore.getState().leftPanelCollapsed).toBe(false);
    expect(useUiStore.getState().rightPanelCollapsed).toBe(false);

    useUiStore.getState().toggleTerminalPanelOpen();

    expect(useUiStore.getState().terminalPanelOpen).toBe(false);
    expect(useUiStore.getState().leftPanelCollapsed).toBe(false);
    expect(useUiStore.getState().rightPanelCollapsed).toBe(false);
  });
});
