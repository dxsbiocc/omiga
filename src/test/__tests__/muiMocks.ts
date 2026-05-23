import React, { type ReactNode } from "react";

type Props = Record<string, unknown> & {
  children?: ReactNode;
  component?: string;
  label?: ReactNode;
  startIcon?: ReactNode;
};

const theme = {
  palette: {
    mode: "light",
    common: { black: "#000", white: "#fff" },
    primary: { main: "#1976d2" },
    warning: { main: "#ed6c02" },
    background: { paper: "#fff", default: "#fafafa" },
  },
  shadows: Array.from({ length: 25 }, () => "none"),
  zIndex: { drawer: 1200 },
};

const host = (type: string) => {
  const Component = ({ children, ...props }: Props) =>
    React.createElement(type, props, children);
  Component.displayName = `Mock${type}`;
  return Component;
};

const container = (type: string) => {
  const Component = ({ children, component, ...props }: Props) =>
    React.createElement(component ?? type, props, children);
  Component.displayName = `Mock${type}`;
  return Component;
};

const button = (type = "button") => {
  const Component = ({ children, startIcon, ...props }: Props) =>
    React.createElement(type, props, startIcon, children);
  Component.displayName = `Mock${type}`;
  return Component;
};

const icon = (name: string) => {
  const Icon = (props: Props) => React.createElement("icon", { ...props, name });
  Icon.displayName = name;
  return Icon;
};

const alpha = (color: string, value: number) => `${color}/${value}`;
const useTheme = () => theme;
const createTheme = () => theme;

const ThemeProvider = ({ children }: Props) =>
  React.createElement("theme-provider", {}, children);

const Dialog = ({ children, open, ...props }: Props) =>
  open ? React.createElement("dialog", props, children) : null;

const Drawer = ({ children, open, ...props }: Props) =>
  open ? React.createElement("drawer", props, children) : null;

const Badge = ({ children, badgeContent, ...props }: Props) =>
  React.createElement("badge", { ...props, badgeContent }, children, String(badgeContent ?? ""));

const Chip = ({ label, children, ...props }: Props) =>
  React.createElement("chip", { ...props, label }, children ?? label);

const Tooltip = ({ children, title, ...props }: Props) =>
  React.createElement("tooltip", { ...props, title }, children);

const TextField = ({
  children,
  inputRef,
  label,
  select,
  SelectProps,
  value,
  ...props
}: Props) => {
  const currentValue = value == null ? "" : String(value);
  const inputNode = {
    selectionStart: currentValue.length,
    selectionEnd: currentValue.length,
    focus: () => undefined,
    setSelectionRange(start: number, end: number) {
      this.selectionStart = start;
      this.selectionEnd = end;
    },
  };

  if (typeof inputRef === "function") {
    inputRef(inputNode);
  }

  return React.createElement(
    select ? "select" : "input",
    {
      ...props,
      ...(typeof SelectProps === "object" && SelectProps ? SelectProps : {}),
      label,
      select: Boolean(select),
      value,
      "aria-label": props["aria-label"] ?? label,
    },
    children,
  );
};

const MenuItem = ({ children, ...props }: Props) =>
  React.createElement("option", props, children);

const Autocomplete = ({
  renderInput,
  value,
  options,
  onChange,
  getOptionLabel,
  disabled,
  ...props
}: Props) => {
  const input =
    typeof renderInput === "function"
      ? renderInput({ inputProps: {}, InputProps: {}, InputLabelProps: {} })
      : null;

  return React.createElement(
    "autocomplete",
    { ...props, value, options, onChange, getOptionLabel, disabled, label: "Operator" },
    input,
  );
};

export const createMuiMaterialMock = () => ({
  __esModule: true,
  Alert: host("alert"),
  Autocomplete,
  Badge,
  Box: container("box"),
  Button: button(),
  Chip,
  CircularProgress: host("progress"),
  Dialog,
  DialogActions: host("dialog-actions"),
  DialogContent: host("dialog-content"),
  DialogTitle: host("dialog-title"),
  Divider: host("divider"),
  Drawer,
  Fab: button(),
  IconButton: button(),
  MenuItem,
  Paper: host("paper"),
  Stack: host("stack"),
  TextField,
  Tooltip,
  Typography: host("typography"),
  alpha,
  createTheme,
  useTheme,
});

export const createMuiStylesMock = () => ({
  __esModule: true,
  ThemeProvider,
  alpha,
  createTheme,
  useTheme,
});

export const createIconMock = (names: string[]) => ({
  __esModule: true,
  ...Object.fromEntries(names.map((name) => [name, icon(name)])),
});
