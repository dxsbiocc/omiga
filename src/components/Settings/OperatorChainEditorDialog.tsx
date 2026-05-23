import { useEffect, useMemo, useRef, useState, type ChangeEvent } from "react";
import {
  Alert,
  Autocomplete,
  Box,
  Button,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  IconButton,
  MenuItem,
  Paper,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  AccountTreeRounded,
  AddRounded,
  ArrowDownwardRounded,
  ArrowUpwardRounded,
  CloseRounded,
  DeleteOutlineRounded,
  FolderOpenRounded,
  SaveRounded,
} from "@mui/icons-material";
import {
  usePluginStore,
  type ChainTemplate,
  type OperatorChainStep,
  type OperatorFieldSpec,
  type OperatorInvocationArguments,
  type OperatorSummary,
} from "../../state/pluginStore";

type OperatorChainEditorDialogProps = {
  open: boolean;
  onClose: () => void;
  operators: OperatorSummary[];
  onRun: (steps: OperatorChainStep[]) => Promise<void>;
};

type FieldGroup = "inputs" | "params";

type FocusedField = {
  stepId: string;
  group: FieldGroup;
  name: string;
};

type ChainEditorStep = {
  id: string;
  operatorKey: string | null;
  values: {
    inputs: Record<string, string>;
    params: Record<string, string>;
  };
};

let nextStepId = 1;

const createStepId = () => `operator-chain-step-${nextStepId++}`;

const operatorKey = (operator: OperatorSummary): string =>
  `${operator.id}:${operator.version}:${operator.sourcePlugin}:${operator.manifestPath}`;

const operatorDisplayName = (operator: OperatorSummary): string =>
  operator.name?.trim() || operator.id;

const operatorPrimaryAlias = (operator: OperatorSummary): string =>
  operator.enabledAliases.find((alias) => alias.trim().length > 0) || operator.id;

const normalizeOperatorAlias = (alias: string): string => {
  const trimmed = alias.trim();
  return trimmed.startsWith("operator__") ? trimmed.slice("operator__".length) : trimmed;
};

const fieldKey = (field: FocusedField): string =>
  `${field.stepId}::${field.group}::${field.name}`;

const sortedFieldEntries = (
  fields?: Record<string, OperatorFieldSpec>,
): Array<[string, OperatorFieldSpec]> =>
  Object.entries(fields ?? {}).sort(([left], [right]) => left.localeCompare(right));

const fieldDefaultValue = (field: OperatorFieldSpec): string => {
  if (field.default === undefined || field.default === null) return "";
  if (typeof field.default === "string") return field.default;
  if (typeof field.default === "number" || typeof field.default === "boolean") {
    return String(field.default);
  }
  try {
    return JSON.stringify(field.default);
  } catch {
    return String(field.default);
  }
};

const createValuesForOperator = (operator: OperatorSummary) => ({
  inputs: Object.fromEntries(
    sortedFieldEntries(operator.interface?.inputs).map(([name, field]) => [
      name,
      fieldDefaultValue(field),
    ]),
  ),
  params: Object.fromEntries(
    sortedFieldEntries(operator.interface?.params).map(([name, field]) => [
      name,
      fieldDefaultValue(field),
    ]),
  ),
});

const createStep = (operator: OperatorSummary | null): ChainEditorStep => ({
  id: createStepId(),
  operatorKey: operator ? operatorKey(operator) : null,
  values: operator ? createValuesForOperator(operator) : { inputs: {}, params: {} },
});

const fieldKind = (field: OperatorFieldSpec): string =>
  field.kind?.trim().toLowerCase() ?? "";

const isNumericField = (field: OperatorFieldSpec): boolean => {
  const kind = fieldKind(field);
  return kind === "number" || kind === "integer" || typeof field.default === "number";
};

const isBooleanField = (field: OperatorFieldSpec): boolean =>
  fieldKind(field) === "boolean" || typeof field.default === "boolean";

const fieldHelperText = (field: OperatorFieldSpec): string | undefined => {
  const parts = [
    field.description?.trim(),
    field.formats?.length ? `Format: ${field.formats.join(", ")}` : null,
  ].filter((part): part is string => Boolean(part));
  return parts.length > 0 ? parts.join(" · ") : undefined;
};

const parseArgumentValue = (value: string, field: OperatorFieldSpec): unknown => {
  const trimmed = value.trim();
  if (trimmed.length === 0) return "";

  if (isNumericField(field)) {
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : value;
  }

  if (isBooleanField(field)) {
    if (trimmed.toLowerCase() === "true") return true;
    if (trimmed.toLowerCase() === "false") return false;
  }

  return value;
};

const buildArgumentGroup = (
  values: Record<string, string>,
  fields?: Record<string, OperatorFieldSpec>,
): Record<string, unknown> => {
  const result: Record<string, unknown> = {};
  for (const [name, field] of sortedFieldEntries(fields)) {
    const value = values[name] ?? "";
    if (value.trim().length === 0 && !field.required) continue;
    result[name] = parseArgumentValue(value, field);
  }
  return result;
};

const requiredFieldsComplete = (
  values: Record<string, string>,
  fields?: Record<string, OperatorFieldSpec>,
): boolean =>
  sortedFieldEntries(fields).every(([name, field]) => {
    if (!field.required && !field.nonEmpty) return true;
    return (values[name] ?? "").trim().length > 0;
  });

const stringifyArgumentValue = (value: unknown): string => {
  if (value === undefined || value === null) return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
};

const valuesFromTemplateStep = (
  operator: OperatorSummary,
  step: OperatorChainStep,
): ChainEditorStep["values"] => {
  const values = createValuesForOperator(operator);
  const inputs = step.arguments.inputs ?? {};
  const params = step.arguments.params ?? {};
  for (const [name] of sortedFieldEntries(operator.interface?.inputs)) {
    if (Object.prototype.hasOwnProperty.call(inputs, name)) {
      values.inputs[name] = stringifyArgumentValue(inputs[name]);
    }
  }
  for (const [name] of sortedFieldEntries(operator.interface?.params)) {
    if (Object.prototype.hasOwnProperty.call(params, name)) {
      values.params[name] = stringifyArgumentValue(params[name]);
    }
  }
  return values;
};

const slugifyTemplateName = (name: string): string =>
  name
    .trim()
    .replace(/[^a-z0-9-]+/gi, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase() || "chain-template";

const formatTemplateRelativeTime = (timestampMs: number): string => {
  if (!Number.isFinite(timestampMs) || timestampMs <= 0) return "unknown";
  const diffMs = timestampMs - Date.now();
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ["year", 1000 * 60 * 60 * 24 * 365],
    ["month", 1000 * 60 * 60 * 24 * 30],
    ["week", 1000 * 60 * 60 * 24 * 7],
    ["day", 1000 * 60 * 60 * 24],
    ["hour", 1000 * 60 * 60],
    ["minute", 1000 * 60],
  ];
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  for (const [unit, unitMs] of units) {
    if (Math.abs(diffMs) >= unitMs) {
      return formatter.format(Math.round(diffMs / unitMs), unit);
    }
  }
  return formatter.format(Math.round(diffMs / 1000), "second");
};

export function OperatorChainEditorDialog({
  open,
  onClose,
  operators,
  onRun,
}: OperatorChainEditorDialogProps) {
  const theme = useTheme();
  const fieldRefs = useRef(new Map<string, HTMLInputElement>());
  const [steps, setSteps] = useState<ChainEditorStep[]>([]);
  const [focusedField, setFocusedField] = useState<FocusedField | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [saveTemplateOpen, setSaveTemplateOpen] = useState(false);
  const [loadTemplateOpen, setLoadTemplateOpen] = useState(false);
  const [templateName, setTemplateName] = useState("");
  const [templateDescription, setTemplateDescription] = useState("");
  const [templateBusy, setTemplateBusy] = useState(false);
  const [templateError, setTemplateError] = useState<string | null>(null);
  const chainTemplates = usePluginStore((state) => state.chainTemplates);
  const loadChainTemplates = usePluginStore((state) => state.loadChainTemplates);
  const saveChainTemplate = usePluginStore((state) => state.saveChainTemplate);
  const deleteChainTemplate = usePluginStore((state) => state.deleteChainTemplate);

  const exposedOperators = useMemo(
    () =>
      operators
        .filter((operator) => operator.exposed)
        .sort((left, right) =>
          operatorDisplayName(left).localeCompare(operatorDisplayName(right))
          || left.sourcePlugin.localeCompare(right.sourcePlugin)
          || left.version.localeCompare(right.version),
        ),
    [operators],
  );

  const operatorsByKey = useMemo(() => {
    const byKey = new Map<string, OperatorSummary>();
    for (const operator of exposedOperators) {
      byKey.set(operatorKey(operator), operator);
    }
    return byKey;
  }, [exposedOperators]);

  const operatorsByAlias = useMemo(() => {
    const byAlias = new Map<string, OperatorSummary>();
    for (const operator of exposedOperators) {
      const aliases = [
        operator.id,
        operatorPrimaryAlias(operator),
        ...operator.enabledAliases,
      ];
      for (const alias of aliases) {
        const normalized = normalizeOperatorAlias(alias);
        if (normalized.length > 0 && !byAlias.has(normalized)) {
          byAlias.set(normalized, operator);
        }
      }
    }
    return byAlias;
  }, [exposedOperators]);

  useEffect(() => {
    if (!open) return;
    setSteps([]);
    setFocusedField(null);
    setLocalError(null);
    setTemplateError(null);
    setSaveTemplateOpen(false);
    setLoadTemplateOpen(false);
  }, [open]);

  useEffect(() => {
    if (!open || chainTemplates.length > 0) return;
    void loadChainTemplates();
  }, [chainTemplates.length, loadChainTemplates, open]);

  const updateStep = (stepId: string, patch: Partial<ChainEditorStep>) => {
    setSteps((current) =>
      current.map((step) => (step.id === stepId ? { ...step, ...patch } : step)),
    );
  };

  const updateFieldValue = (
    stepId: string,
    group: FieldGroup,
    name: string,
    value: string,
  ) => {
    setSteps((current) =>
      current.map((step) =>
        step.id === stepId
          ? {
              ...step,
              values: {
                ...step.values,
                [group]: {
                  ...step.values[group],
                  [name]: value,
                },
              },
            }
          : step,
      ),
    );
  };

  const handleOperatorChange = (stepId: string, operator: OperatorSummary | null) => {
    updateStep(stepId, {
      operatorKey: operator ? operatorKey(operator) : null,
      values: operator ? createValuesForOperator(operator) : { inputs: {}, params: {} },
    });
    setFocusedField((current) => (current?.stepId === stepId ? null : current));
  };

  const handleAddStep = () => {
    setSteps((current) => [...current, createStep(exposedOperators[0] ?? null)]);
  };

  const handleMoveStep = (index: number, direction: -1 | 1) => {
    setSteps((current) => {
      const nextIndex = index + direction;
      if (nextIndex < 0 || nextIndex >= current.length) return current;
      const next = [...current];
      [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
      return next;
    });
  };

  const handleRemoveStep = (stepId: string) => {
    setSteps((current) => current.filter((step) => step.id !== stepId));
    setFocusedField((current) => (current?.stepId === stepId ? null : current));
  };

  const insertOutputReference = (targetStepId: string, sourceIndex: number) => {
    const targetField =
      focusedField?.stepId === targetStepId ? focusedField : null;
    if (!targetField) return;

    const step = steps.find((candidate) => candidate.id === targetStepId);
    if (!step) return;

    const input = fieldRefs.current.get(fieldKey(targetField));
    const placeholder = `{{step${sourceIndex + 1}.outputDir}}`;
    const currentValue = step.values[targetField.group][targetField.name] ?? "";
    const selectionStart = input?.selectionStart ?? currentValue.length;
    const selectionEnd = input?.selectionEnd ?? currentValue.length;
    const nextValue =
      currentValue.slice(0, selectionStart)
      + placeholder
      + currentValue.slice(selectionEnd);

    updateFieldValue(targetStepId, targetField.group, targetField.name, nextValue);

    window.requestAnimationFrame(() => {
      const target = fieldRefs.current.get(fieldKey(targetField));
      const cursor = selectionStart + placeholder.length;
      target?.focus();
      target?.setSelectionRange(cursor, cursor);
    });
  };

  const stepIsValid = (step: ChainEditorStep): boolean => {
    if (!step.operatorKey) return false;
    const operator = operatorsByKey.get(step.operatorKey);
    if (!operator) return false;
    return (
      requiredFieldsComplete(step.values.inputs, operator.interface?.inputs)
      && requiredFieldsComplete(step.values.params, operator.interface?.params)
    );
  };

  const canRun = steps.length > 0 && steps.every(stepIsValid) && !submitting;
  const canSaveTemplate = steps.length > 0 && !submitting && !templateBusy;

  const buildSteps = (): OperatorChainStep[] =>
    steps.map((step) => {
      const operator = operatorsByKey.get(step.operatorKey ?? "");
      if (!operator) {
        throw new Error("Select an operator for every step.");
      }
      const args: OperatorInvocationArguments = {
        inputs: buildArgumentGroup(step.values.inputs, operator.interface?.inputs),
        params: buildArgumentGroup(step.values.params, operator.interface?.params),
        resources: {},
      };
      return {
        alias: operatorPrimaryAlias(operator),
        arguments: args,
      };
    });

  const handleRun = async () => {
    if (!canRun) return;
    setSubmitting(true);
    setLocalError(null);
    try {
      await onRun(buildSteps());
      onClose();
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  const handleOpenSaveTemplate = () => {
    if (steps.length === 0) return;
    setTemplateName("");
    setTemplateDescription("");
    setTemplateError(null);
    setSaveTemplateOpen(true);
  };

  const handleSaveTemplate = async () => {
    const name = templateName.trim();
    if (name.length === 0) {
      setTemplateError("Template name is required.");
      return;
    }
    if (!steps.every(stepIsValid)) {
      setTemplateError("Complete all required step fields before saving.");
      return;
    }

    let chainSteps: OperatorChainStep[];
    try {
      chainSteps = buildSteps();
    } catch (error) {
      setTemplateError(error instanceof Error ? error.message : String(error));
      return;
    }

    setTemplateBusy(true);
    setTemplateError(null);
    try {
      await saveChainTemplate({
        id: slugifyTemplateName(name),
        name,
        description: templateDescription.trim() || null,
        steps: chainSteps,
      });
      setSaveTemplateOpen(false);
      setTemplateName("");
      setTemplateDescription("");
    } catch (error) {
      setTemplateError(error instanceof Error ? error.message : String(error));
    } finally {
      setTemplateBusy(false);
    }
  };

  const handleOpenLoadTemplate = () => {
    setTemplateError(null);
    setLoadTemplateOpen(true);
    void loadChainTemplates();
  };

  const templateToEditorSteps = (template: ChainTemplate) => {
    const missingAliases: string[] = [];
    const nextSteps = template.steps.map((step) => {
      const operator = operatorsByAlias.get(normalizeOperatorAlias(step.alias)) ?? null;
      if (!operator) {
        missingAliases.push(step.alias);
      }
      return {
        id: createStepId(),
        operatorKey: operator ? operatorKey(operator) : null,
        values: operator
          ? valuesFromTemplateStep(operator, step)
          : { inputs: {}, params: {} },
      };
    });
    return { nextSteps, missingAliases };
  };

  const handleLoadTemplate = (template: ChainTemplate) => {
    if (!window.confirm(`Load "${template.name}" and replace the current chain?`)) {
      return;
    }
    const { nextSteps, missingAliases } = templateToEditorSteps(template);
    setSteps(nextSteps);
    setFocusedField(null);
    setLoadTemplateOpen(false);
    if (missingAliases.length > 0) {
      setLocalError(
        `Loaded template, but ${missingAliases.length} operator alias${missingAliases.length === 1 ? "" : "es"} could not be matched.`,
      );
    } else {
      setLocalError(null);
    }
  };

  const handleDeleteTemplate = async (template: ChainTemplate) => {
    if (!window.confirm(`Delete template "${template.name}"?`)) return;
    setTemplateBusy(true);
    setTemplateError(null);
    try {
      await deleteChainTemplate(template.id);
    } catch (error) {
      setTemplateError(error instanceof Error ? error.message : String(error));
    } finally {
      setTemplateBusy(false);
    }
  };

  const renderFields = (
    step: ChainEditorStep,
    group: FieldGroup,
    fields?: Record<string, OperatorFieldSpec>,
  ) => {
    const entries = sortedFieldEntries(fields);
    if (entries.length === 0) return null;

    return (
      <Stack spacing={0.75} useFlexGap>
        <Typography variant="caption" fontWeight={800} color="text.secondary">
          {group === "inputs" ? "Inputs" : "Params"}
        </Typography>
        <Box
          sx={{
            display: "grid",
            gridTemplateColumns: { xs: "1fr", md: "repeat(2, minmax(0, 1fr))" },
            gap: 1,
          }}
        >
          {entries.map(([name, field]) => {
            const currentField = { stepId: step.id, group, name };
            const key = fieldKey(currentField);
            const enumValues = (field.enum ?? []).map((value) => String(value));
            const helperText = fieldHelperText(field);
            const commonProps = {
              key,
              size: "small" as const,
              label: name,
              required: Boolean(field.required || field.nonEmpty),
              value: step.values[group][name] ?? "",
              onChange: (event: ChangeEvent<HTMLInputElement>) =>
                updateFieldValue(step.id, group, name, event.target.value),
              onFocus: () => setFocusedField(currentField),
              inputRef: (node: HTMLInputElement | null) => {
                if (node) {
                  fieldRefs.current.set(key, node);
                } else {
                  fieldRefs.current.delete(key);
                }
              },
              helperText,
              fullWidth: true,
            };

            if (enumValues.length > 0) {
              return (
                <TextField {...commonProps} select>
                  {!field.required && <MenuItem value="">Unset</MenuItem>}
                  {enumValues.map((value) => (
                    <MenuItem key={value} value={value}>
                      {value}
                    </MenuItem>
                  ))}
                </TextField>
              );
            }

            if (isBooleanField(field)) {
              return (
                <TextField {...commonProps} select>
                  {!field.required && <MenuItem value="">Unset</MenuItem>}
                  <MenuItem value="true">true</MenuItem>
                  <MenuItem value="false">false</MenuItem>
                </TextField>
              );
            }

            return (
              <TextField
                {...commonProps}
                multiline
                minRows={1}
                maxRows={4}
              />
            );
          })}
        </Box>
      </Stack>
    );
  };

  return (
    <>
      <Dialog
        open={open}
        onClose={submitting ? undefined : onClose}
        fullWidth
        maxWidth="lg"
        aria-labelledby="operator-chain-editor-title"
      >
        <DialogTitle id="operator-chain-editor-title" sx={{ px: 3, py: 2, pr: 7 }}>
        <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
          <AccountTreeRounded fontSize="small" color="action" />
          <Typography variant="subtitle1" fontWeight={850}>
            Operator chain editor
          </Typography>
          <Chip size="small" variant="outlined" label={`${steps.length} steps`} />
          <Box sx={{ flexGrow: 1, minWidth: { xs: "100%", sm: 12 } }} />
          <Button
            size="small"
            variant="outlined"
            startIcon={<FolderOpenRounded />}
            disabled={submitting || templateBusy}
            onClick={handleOpenLoadTemplate}
            sx={{ textTransform: "none", borderRadius: 1.5 }}
          >
            Load template
          </Button>
          <Button
            size="small"
            variant="outlined"
            startIcon={<SaveRounded />}
            disabled={!canSaveTemplate}
            onClick={handleOpenSaveTemplate}
            sx={{ textTransform: "none", borderRadius: 1.5 }}
          >
            Save as template
          </Button>
        </Stack>
        <IconButton
          aria-label="Close chain editor"
          disabled={submitting}
          onClick={onClose}
          sx={{ position: "absolute", right: 12, top: 10 }}
        >
          <CloseRounded />
        </IconButton>
        </DialogTitle>

        <DialogContent sx={{ px: 3, pt: 1, pb: 2 }}>
        <Stack spacing={1.25} useFlexGap>
          {exposedOperators.length === 0 && (
            <Alert severity="info" sx={{ borderRadius: 2 }}>
              Register an operator before creating a chain.
            </Alert>
          )}

          {steps.length === 0 ? (
            <Paper
              variant="outlined"
              sx={{
                p: 2.5,
                borderRadius: 2,
                bgcolor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.08 : 0.04),
                borderColor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.22 : 0.14),
                textAlign: "center",
              }}
            >
              <Typography variant="body2" color="text.secondary">
                No steps yet.
              </Typography>
            </Paper>
          ) : (
            steps.map((step, index) => {
              const operator = operatorsByKey.get(step.operatorKey ?? "") ?? null;
              const priorSteps = steps.slice(0, index);
              const focusedInStep = focusedField?.stepId === step.id;
              const invalid = !stepIsValid(step);

              return (
                <Paper
                  key={step.id}
                  variant="outlined"
                  sx={{
                    p: 1.5,
                    borderRadius: 2,
                    borderColor: invalid
                      ? alpha(theme.palette.warning.main, 0.42)
                      : "divider",
                  }}
                >
                  <Stack spacing={1.25} useFlexGap>
                    <Stack
                      direction={{ xs: "column", sm: "row" }}
                      spacing={1}
                      alignItems={{ xs: "stretch", sm: "center" }}
                      justifyContent="space-between"
                    >
                      <Stack direction="row" spacing={0.75} alignItems="center" flexWrap="wrap">
                        <Chip size="small" color="primary" variant="outlined" label={`Step ${index + 1}`} />
                        {operator && (
                          <Chip
                            size="small"
                            variant="outlined"
                            label={operatorPrimaryAlias(operator)}
                            sx={{ maxWidth: 220 }}
                          />
                        )}
                      </Stack>

                      <Stack direction="row" spacing={0.5} alignItems="center">
                        <Tooltip title="Move up">
                          <span>
                            <IconButton
                              aria-label={`Move step ${index + 1} up`}
                              size="small"
                              disabled={submitting || index === 0}
                              onClick={() => handleMoveStep(index, -1)}
                            >
                              <ArrowUpwardRounded fontSize="small" />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Tooltip title="Move down">
                          <span>
                            <IconButton
                              aria-label={`Move step ${index + 1} down`}
                              size="small"
                              disabled={submitting || index === steps.length - 1}
                              onClick={() => handleMoveStep(index, 1)}
                            >
                              <ArrowDownwardRounded fontSize="small" />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Tooltip title="Remove">
                          <span>
                            <IconButton
                              aria-label={`Remove step ${index + 1}`}
                              size="small"
                              color="warning"
                              disabled={submitting}
                              onClick={() => handleRemoveStep(step.id)}
                            >
                              <DeleteOutlineRounded fontSize="small" />
                            </IconButton>
                          </span>
                        </Tooltip>
                      </Stack>
                    </Stack>

                    <Stack direction={{ xs: "column", md: "row" }} spacing={1} alignItems="flex-start">
                      <Autocomplete
                        size="small"
                        options={exposedOperators}
                        value={operator}
                        disabled={submitting}
                        isOptionEqualToValue={(option, value) => operatorKey(option) === operatorKey(value)}
                        getOptionLabel={operatorDisplayName}
                        onChange={(_event, value) => handleOperatorChange(step.id, value)}
                        sx={{ flex: 1, minWidth: { xs: "100%", md: 320 } }}
                        renderOption={(props, option) => {
                          const { key, ...optionProps } = props;
                          return (
                            <Box component="li" {...optionProps} key={key}>
                              <Box sx={{ minWidth: 0 }}>
                                <Typography variant="body2" fontWeight={700}>
                                  {operatorDisplayName(option)}
                                </Typography>
                                <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                                  {option.id} · {option.sourcePlugin}
                                </Typography>
                              </Box>
                            </Box>
                          );
                        }}
                        renderInput={(params) => (
                          <TextField
                            {...params}
                            label="Operator"
                            required
                            error={!operator}
                            helperText={!operator ? "Select an operator." : undefined}
                          />
                        )}
                      />

                      <TextField
                        select
                        size="small"
                        label="Use output from"
                        value=""
                        disabled={submitting || priorSteps.length === 0 || !focusedInStep}
                        onChange={(event) => {
                          const sourceIndex = Number(event.target.value);
                          if (Number.isInteger(sourceIndex)) {
                            insertOutputReference(step.id, sourceIndex);
                          }
                        }}
                        helperText={
                          priorSteps.length === 0
                            ? "No prior steps"
                            : focusedInStep
                              ? "Inserts into focused field"
                              : "Focus a field first"
                        }
                        sx={{ minWidth: { xs: "100%", md: 220 } }}
                        SelectProps={{ displayEmpty: true }}
                      >
                        <MenuItem value="" disabled>
                          Select step
                        </MenuItem>
                        {priorSteps.map((priorStep, priorIndex) => {
                          const priorOperator = operatorsByKey.get(priorStep.operatorKey ?? "");
                          const label = priorOperator
                            ? operatorDisplayName(priorOperator)
                            : "Unselected operator";
                          return (
                            <MenuItem key={priorStep.id} value={priorIndex}>
                              {`Step ${priorIndex + 1}: ${label}`}
                            </MenuItem>
                          );
                        })}
                      </TextField>
                    </Stack>

                    {operator ? (
                      <Stack spacing={1.25} useFlexGap>
                        {renderFields(step, "inputs", operator.interface?.inputs)}
                        {renderFields(step, "params", operator.interface?.params)}
                        {!operator.interface?.inputs && !operator.interface?.params && (
                          <Typography variant="caption" color="text.secondary">
                            This operator does not declare inputs or params.
                          </Typography>
                        )}
                      </Stack>
                    ) : null}
                  </Stack>
                </Paper>
              );
            })
          )}

          <Button
            size="small"
            variant="outlined"
            startIcon={<AddRounded />}
            disabled={submitting || exposedOperators.length === 0}
            onClick={handleAddStep}
            sx={{ alignSelf: "flex-start", textTransform: "none", borderRadius: 1.5 }}
          >
            Add step
          </Button>

          {localError && (
            <Alert severity="error" sx={{ borderRadius: 2 }}>
              {localError}
            </Alert>
          )}
        </Stack>
        </DialogContent>

        <DialogActions sx={{ px: 3, py: 2 }}>
        <Button
          onClick={onClose}
          disabled={submitting}
          sx={{ textTransform: "none", borderRadius: 1.5 }}
        >
          Cancel
        </Button>
        <Button
          variant="contained"
          startIcon={submitting ? <CircularProgress size={18} color="inherit" /> : <AccountTreeRounded />}
          disabled={!canRun}
          onClick={() => void handleRun()}
          sx={{ textTransform: "none", borderRadius: 1.5 }}
        >
          Run chain
        </Button>
        </DialogActions>
      </Dialog>

      <Dialog
        open={saveTemplateOpen}
        onClose={templateBusy ? undefined : () => setSaveTemplateOpen(false)}
        fullWidth
        maxWidth="sm"
      >
        <DialogTitle>Save as template</DialogTitle>
        <DialogContent>
          <Stack spacing={1.5} sx={{ pt: 1 }}>
            <TextField
              label="Name"
              required
              autoFocus
              value={templateName}
              disabled={templateBusy}
              onChange={(event) => setTemplateName(event.target.value)}
              fullWidth
            />
            <TextField
              label="Description"
              value={templateDescription}
              disabled={templateBusy}
              onChange={(event) => setTemplateDescription(event.target.value)}
              multiline
              minRows={2}
              fullWidth
            />
            {templateError && (
              <Alert severity="error" sx={{ borderRadius: 2 }}>
                {templateError}
              </Alert>
            )}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button
            onClick={() => setSaveTemplateOpen(false)}
            disabled={templateBusy}
            sx={{ textTransform: "none", borderRadius: 1.5 }}
          >
            Cancel
          </Button>
          <Button
            variant="contained"
            startIcon={templateBusy ? <CircularProgress size={18} color="inherit" /> : <SaveRounded />}
            disabled={templateBusy || templateName.trim().length === 0}
            onClick={() => void handleSaveTemplate()}
            sx={{ textTransform: "none", borderRadius: 1.5 }}
          >
            Save
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog
        open={loadTemplateOpen}
        onClose={templateBusy ? undefined : () => setLoadTemplateOpen(false)}
        fullWidth
        maxWidth="md"
      >
        <DialogTitle>Load template</DialogTitle>
        <DialogContent>
          <Stack spacing={1.25} sx={{ pt: 1 }}>
            {templateError && (
              <Alert severity="error" sx={{ borderRadius: 2 }}>
                {templateError}
              </Alert>
            )}

            {chainTemplates.length === 0 ? (
              <Paper
                variant="outlined"
                sx={{ p: 2.5, borderRadius: 2, textAlign: "center" }}
              >
                <Typography variant="body2" color="text.secondary">
                  No templates saved.
                </Typography>
              </Paper>
            ) : (
              <Box
                sx={{
                  border: 1,
                  borderColor: "divider",
                  borderRadius: 1.5,
                  overflow: "hidden",
                }}
              >
                {chainTemplates.map((template, index) => (
                  <Box key={template.id}>
                    <Stack
                      direction={{ xs: "column", sm: "row" }}
                      spacing={1}
                      alignItems={{ xs: "stretch", sm: "center" }}
                      justifyContent="space-between"
                      sx={{ p: 1.5 }}
                    >
                      <Box sx={{ minWidth: 0 }}>
                        <Stack direction="row" spacing={0.75} alignItems="center" flexWrap="wrap">
                          <Typography variant="body2" fontWeight={800}>
                            {template.name}
                          </Typography>
                          <Chip
                            size="small"
                            variant="outlined"
                            label={formatTemplateRelativeTime(template.updatedAtMs)}
                          />
                          <Chip
                            size="small"
                            variant="outlined"
                            label={`${template.steps.length} steps`}
                          />
                        </Stack>
                        {template.description && (
                          <Typography
                            variant="body2"
                            color="text.secondary"
                            sx={{ mt: 0.5, wordBreak: "break-word" }}
                          >
                            {template.description}
                          </Typography>
                        )}
                      </Box>

                      <Stack direction="row" spacing={0.75} alignItems="center" justifyContent="flex-end">
                        <Button
                          size="small"
                          variant="outlined"
                          disabled={templateBusy}
                          onClick={() => handleLoadTemplate(template)}
                          sx={{ textTransform: "none", borderRadius: 1.5 }}
                        >
                          Load
                        </Button>
                        <Tooltip title="Delete">
                          <span>
                            <IconButton
                              aria-label={`Delete template ${template.name}`}
                              color="warning"
                              disabled={templateBusy}
                              onClick={() => void handleDeleteTemplate(template)}
                            >
                              <DeleteOutlineRounded />
                            </IconButton>
                          </span>
                        </Tooltip>
                      </Stack>
                    </Stack>
                    {index < chainTemplates.length - 1 && <Divider />}
                  </Box>
                ))}
              </Box>
            )}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button
            onClick={() => setLoadTemplateOpen(false)}
            disabled={templateBusy}
            sx={{ textTransform: "none", borderRadius: 1.5 }}
          >
            Close
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
