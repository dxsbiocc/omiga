import { Fragment, type ReactElement, type ReactNode } from "react";

export type RenderedNode = {
  type: string;
  props: Record<string, unknown>;
  children: RenderedTree[];
};

export type RenderedTree = RenderedNode | string | null;

type EffectState = {
  deps: readonly unknown[] | undefined;
  cleanup?: void | (() => void);
};

type MemoState<T> = {
  deps: readonly unknown[] | undefined;
  value: T;
};

const depsChanged = (
  previous: readonly unknown[] | undefined,
  next: readonly unknown[] | undefined,
): boolean => {
  if (!previous || !next) return true;
  if (previous.length !== next.length) return true;
  return previous.some((value, index) => !Object.is(value, next[index]));
};

export const installComponentTestWindow = () => {
  const target = globalThis as typeof globalThis & {
    window?: typeof globalThis & {
      requestAnimationFrame?: (callback: FrameRequestCallback) => number;
      cancelAnimationFrame?: (handle: number) => void;
    };
  };

  target.window ??= target;
  target.window.requestAnimationFrame ??= (callback: FrameRequestCallback) => {
    callback(Date.now());
    return 0;
  };
  target.window.cancelAnimationFrame ??= () => undefined;
};

export const createHookRuntime = () => {
  const hooks: unknown[] = [];
  const pendingEffects: Array<{ index: number; effect: () => void | (() => void) }> = [];
  let hookIndex = 0;
  let dirty = false;

  const markDirty = () => {
    dirty = true;
  };

  const runtime = {
    beginRender() {
      hookIndex = 0;
      pendingEffects.length = 0;
      dirty = false;
    },

    finishRender() {
      const effects = pendingEffects.splice(0);
      for (const { index, effect } of effects) {
        const state = hooks[index] as EffectState;
        if (typeof state.cleanup === "function") {
          state.cleanup();
        }
        state.cleanup = effect();
      }
    },

    consumeDirty() {
      const wasDirty = dirty;
      dirty = false;
      return wasDirty;
    },

    cleanup() {
      for (const hook of hooks) {
        if (!hook || typeof hook !== "object") continue;
        const maybeEffect = hook as Partial<EffectState>;
        if (typeof maybeEffect.cleanup === "function") {
          maybeEffect.cleanup();
        }
      }
      hooks.length = 0;
      pendingEffects.length = 0;
      hookIndex = 0;
      dirty = false;
    },

    useState<T>(initial: T | (() => T)): [T, (next: T | ((current: T) => T)) => void] {
      const index = hookIndex++;
      if (hooks[index] === undefined) {
        hooks[index] =
          typeof initial === "function" ? (initial as () => T)() : initial;
      }

      const setState = (next: T | ((current: T) => T)) => {
        const current = hooks[index] as T;
        const value =
          typeof next === "function" ? (next as (current: T) => T)(current) : next;
        if (!Object.is(current, value)) {
          hooks[index] = value;
          markDirty();
        }
      };

      return [hooks[index] as T, setState];
    },

    useMemo<T>(factory: () => T, deps: readonly unknown[] | undefined): T {
      const index = hookIndex++;
      const previous = hooks[index] as MemoState<T> | undefined;
      if (!previous || depsChanged(previous.deps, deps)) {
        const value = factory();
        hooks[index] = { deps, value };
        return value;
      }
      return previous.value;
    },

    useRef<T>(initial: T): { current: T } {
      const index = hookIndex++;
      if (hooks[index] === undefined) {
        hooks[index] = { current: initial };
      }
      return hooks[index] as { current: T };
    },

    useEffect(effect: () => void | (() => void), deps?: readonly unknown[]) {
      const index = hookIndex++;
      const previous = hooks[index] as EffectState | undefined;
      if (!previous || depsChanged(previous.deps, deps)) {
        hooks[index] = { deps, cleanup: previous?.cleanup };
        pendingEffects.push({ index, effect });
      }
    },
  };

  return runtime;
};

export type HookRuntimeApi = ReturnType<typeof createHookRuntime>;

const isReactElement = (value: unknown): value is ReactElement =>
  Boolean(
    value
      && typeof value === "object"
      && "type" in value
      && "props" in value,
  );

const childArray = (children: ReactNode): ReactNode[] => {
  if (children === undefined || children === null || typeof children === "boolean") {
    return [];
  }
  return Array.isArray(children) ? children.flatMap(childArray) : [children];
};

export const renderTestElement = (value: ReactNode): RenderedTree => {
  if (value === undefined || value === null || typeof value === "boolean") {
    return null;
  }
  if (typeof value === "string" || typeof value === "number") {
    return String(value);
  }
  if (Array.isArray(value)) {
    return {
      type: "fragment",
      props: {},
      children: value.map(renderTestElement).filter(Boolean),
    };
  }
  if (!isReactElement(value)) {
    return String(value);
  }

  if (value.type === Fragment) {
    return {
      type: "fragment",
      props: {},
      children: childArray(value.props.children)
        .map(renderTestElement)
        .filter(Boolean),
    };
  }

  if (typeof value.type === "function") {
    return renderTestElement(value.type(value.props));
  }

  if (typeof value.type !== "string") {
    return null;
  }

  return {
    type: value.type,
    props: value.props as Record<string, unknown>,
    children: childArray(value.props.children)
      .map(renderTestElement)
      .filter(Boolean),
  };
};

export const textContent = (tree: RenderedTree): string => {
  if (!tree) return "";
  if (typeof tree === "string") return tree;
  return tree.children.map(textContent).join("");
};

export const findAllNodes = (
  tree: RenderedTree,
  predicate: (node: RenderedNode) => boolean,
): RenderedNode[] => {
  if (!tree || typeof tree === "string") return [];
  const children = tree.children.flatMap((child) => findAllNodes(child, predicate));
  return predicate(tree) ? [tree, ...children] : children;
};

export class ComponentHarness {
  tree: RenderedTree = null;

  constructor(
    private readonly runtime: HookRuntimeApi,
    private readonly elementFactory: () => ReactElement,
  ) {}

  render() {
    let iterations = 0;
    do {
      this.runtime.beginRender();
      this.tree = renderTestElement(this.elementFactory());
      this.runtime.finishRender();
      iterations += 1;
      if (iterations > 25) {
        throw new Error("Component test harness exceeded rerender limit.");
      }
    } while (this.runtime.consumeDirty());
  }

  flush() {
    this.render();
  }

  click(node: RenderedNode) {
    const onClick = node.props.onClick as
      | undefined
      | ((event: { currentTarget: RenderedNode; target: RenderedNode }) => unknown);
    if (!onClick) throw new Error(`Node ${node.type} does not have an onClick handler.`);
    const result = onClick({ currentTarget: node, target: node });
    this.render();
    return result;
  }

  change(node: RenderedNode, value: unknown) {
    const onChange = node.props.onChange as
      | undefined
      | ((event: { target: { value: unknown } }) => unknown);
    if (!onChange) throw new Error(`Node ${node.type} does not have an onChange handler.`);
    const result = onChange({ target: { value } });
    this.render();
    return result;
  }

  focus(node: RenderedNode) {
    const onFocus = node.props.onFocus as
      | undefined
      | ((event: { currentTarget: RenderedNode; target: RenderedNode }) => unknown);
    if (!onFocus) throw new Error(`Node ${node.type} does not have an onFocus handler.`);
    const result = onFocus({ currentTarget: node, target: node });
    this.render();
    return result;
  }

  cleanup() {
    this.runtime.cleanup();
  }
}
