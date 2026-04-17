import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  Divider,
  FormControl,
  FormControlLabel,
  InputLabel,
  MenuItem,
  Select,
  Stack,
  Switch,
  Tab,
  Tabs,
  Typography,
} from "@mui/material";
import RefreshIcon from "@mui/icons-material/Refresh";
import SaveIcon from "@mui/icons-material/Save";
import HistoryIcon from "@mui/icons-material/History";
import TuneIcon from "@mui/icons-material/Tune";

type ConstraintSeverity = "info" | "warn" | "error";
type ConstraintPolicyPack =
  | "balanced"
  | "coding_strict"
  | "explanation_strict";

type RuntimeConstraintRuleConfig = {
  enabled?: boolean | null;
  severity?: ConstraintSeverity | null;
};

type RuntimeConstraintConfig = {
  enabled: boolean;
  buffer_responses: boolean;
  policy_pack: ConstraintPolicyPack;
  rules: Record<string, RuntimeConstraintRuleConfig>;
};

type RuntimeConstraintRuleStatus = {
  id: string;
  description: string;
  severity: ConstraintSeverity;
  enabled: boolean;
  phases: string[];
};

type RuntimeConstraintConfigSnapshot = {
  project_config: RuntimeConstraintConfig;
  session_config: RuntimeConstraintConfig | null;
  resolved_enabled: boolean;
  resolved_buffer_responses: boolean;
  resolved_policy_pack: ConstraintPolicyPack;
  registry: RuntimeConstraintRuleStatus[];
};

type RuntimeConstraintTraceRound = {
  round_id: string;
  session_id: string;
  message_id: string;
  event_count: number;
  first_event_at: string;
  last_event_at: string;
};

type RuntimeConstraintTraceEvent = {
  id: string;
  session_id: string;
  round_id: string;
  message_id: string;
  event_type: string;
  constraint_id: string | null;
  payload_json: string;
  created_at: string;
};

type RuntimeConstraintTraceSummary = {
  round_id: string;
  session_id: string;
  message_id: string;
  total_events: number;
  first_event_at: string | null;
  last_event_at: string | null;
  event_type_counts: Record<string, number>;
  constraint_counts: Record<string, number>;
  noticed_constraints: string[];
  gate_constraints: string[];
  retry_constraints: string[];
  commit_phases: string[];
  config_payload: unknown;
};

function cloneConfig(cfg: RuntimeConstraintConfig): RuntimeConstraintConfig {
  return {
    enabled: cfg.enabled,
    buffer_responses: cfg.buffer_responses,
    policy_pack: cfg.policy_pack,
    rules: Object.fromEntries(
      Object.entries(cfg.rules ?? {}).map(([key, value]) => [
        key,
        {
          enabled: value?.enabled ?? null,
          severity: value?.severity ?? null,
        },
      ]),
    ),
  };
}

function defaultConfig(): RuntimeConstraintConfig {
  return {
    enabled: true,
    buffer_responses: true,
    policy_pack: "balanced",
    rules: {},
  };
}

function prettyJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function parseJsonLoose(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch {
    return raw;
  }
}

function formatTs(value: string | null | undefined): string {
  if (!value) return "—";
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? value : d.toLocaleString();
}

function severityColor(
  severity: ConstraintSeverity,
): "info" | "warning" | "error" {
  if (severity === "warn") return "warning";
  if (severity === "error") return "error";
  return "info";
}

export function RuntimeConstraintsPanel({
  projectPath,
  sessionId,
}: {
  projectPath: string;
  sessionId: string | null;
}) {
  const [tab, setTab] = useState<"config" | "trace">("config");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{
    kind: "success" | "error";
    text: string;
  } | null>(null);
  const [snapshot, setSnapshot] = useState<RuntimeConstraintConfigSnapshot | null>(
    null,
  );
  const [projectDraft, setProjectDraft] = useState<RuntimeConstraintConfig>(
    defaultConfig(),
  );
  const [sessionOverrideEnabled, setSessionOverrideEnabled] = useState(false);
  const [sessionDraft, setSessionDraft] = useState<RuntimeConstraintConfig>(
    defaultConfig(),
  );
  const [traceLoading, setTraceLoading] = useState(false);
  const [traceRounds, setTraceRounds] = useState<RuntimeConstraintTraceRound[]>([]);
  const [selectedRoundId, setSelectedRoundId] = useState<string>("");
  const [traceSummary, setTraceSummary] =
    useState<RuntimeConstraintTraceSummary | null>(null);
  const [traceEvents, setTraceEvents] = useState<RuntimeConstraintTraceEvent[]>([]);

  const loadConfig = useCallback(async () => {
    setLoading(true);
    setMessage(null);
    try {
      const data = await invoke<RuntimeConstraintConfigSnapshot>(
        "get_runtime_constraints_config",
        {
          sessionId,
          projectPath,
        },
      );
      setSnapshot(data);
      setProjectDraft(cloneConfig(data.project_config));
      setSessionOverrideEnabled(Boolean(data.session_config));
      setSessionDraft(cloneConfig(data.session_config ?? data.project_config));
    } catch (error) {
      setMessage({
        kind: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setLoading(false);
    }
  }, [projectPath, sessionId]);

  const loadTraceRounds = useCallback(async () => {
    if (!sessionId) {
      setTraceRounds([]);
      setSelectedRoundId("");
      setTraceSummary(null);
      setTraceEvents([]);
      return;
    }
    setTraceLoading(true);
    try {
      const rounds = await invoke<RuntimeConstraintTraceRound[]>(
        "list_runtime_constraint_trace_rounds",
        {
          sessionId,
          limit: 20,
        },
      );
      setTraceRounds(rounds);
      setSelectedRoundId((prev) =>
        prev && rounds.some((r) => r.round_id === prev)
          ? prev
          : rounds[0]?.round_id ?? "",
      );
    } catch (error) {
      setMessage({
        kind: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setTraceLoading(false);
    }
  }, [sessionId]);

  const loadTraceDetails = useCallback(
    async (roundId: string) => {
      if (!roundId) {
        setTraceSummary(null);
        setTraceEvents([]);
        return;
      }
      setTraceLoading(true);
      try {
        const [summary, events] = await Promise.all([
          invoke<RuntimeConstraintTraceSummary | null>(
            "summarize_runtime_constraint_trace",
            {
              roundId,
            },
          ),
          invoke<RuntimeConstraintTraceEvent[]>("get_runtime_constraint_trace", {
            roundId,
          }),
        ]);
        setTraceSummary(summary);
        setTraceEvents(events);
      } catch (error) {
        setMessage({
          kind: "error",
          text: error instanceof Error ? error.message : String(error),
        });
      } finally {
        setTraceLoading(false);
      }
    },
    [],
  );

  useEffect(() => {
    void loadConfig();
  }, [loadConfig]);

  useEffect(() => {
    if (tab === "trace") {
      void loadTraceRounds();
    }
  }, [tab, loadTraceRounds]);

  useEffect(() => {
    if (tab === "trace" && selectedRoundId) {
      void loadTraceDetails(selectedRoundId);
    }
  }, [tab, selectedRoundId, loadTraceDetails]);

  const resolvedSummary = useMemo(() => {
    if (!snapshot) return null;
    return [
      {
        label: "Resolved enabled",
        value: snapshot.resolved_enabled ? "On" : "Off",
      },
      {
        label: "Buffered commit",
        value: snapshot.resolved_buffer_responses ? "On" : "Off",
      },
      {
        label: "Policy pack",
        value: snapshot.resolved_policy_pack,
      },
      {
        label: "Rules",
        value: String(snapshot.registry.length),
      },
    ];
  }, [snapshot]);

  const updateRuleDraft = (
    scope: "project" | "session",
    id: string,
    patch: Partial<RuntimeConstraintRuleConfig>,
  ) => {
    const updater = (prev: RuntimeConstraintConfig): RuntimeConstraintConfig => ({
      ...prev,
      rules: {
        ...prev.rules,
        [id]: {
          enabled: prev.rules[id]?.enabled ?? null,
          severity: prev.rules[id]?.severity ?? null,
          ...patch,
        },
      },
    });
    if (scope === "project") {
      setProjectDraft((prev) => updater(prev));
    } else {
      setSessionDraft((prev) => updater(prev));
    }
  };

  const saveProject = async () => {
    setSaving(true);
    setMessage(null);
    try {
      await invoke("save_project_runtime_constraints_config", {
        projectPath,
        config: projectDraft,
      });
      setMessage({ kind: "success", text: "Project runtime constraints saved." });
      await loadConfig();
    } catch (error) {
      setMessage({
        kind: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  const saveSession = async () => {
    if (!sessionId) return;
    setSaving(true);
    setMessage(null);
    try {
      await invoke("save_session_runtime_constraints_config", {
        sessionId,
        config: sessionOverrideEnabled ? sessionDraft : null,
      });
      setMessage({
        kind: "success",
        text: sessionOverrideEnabled
          ? "Session runtime constraints saved."
          : "Session runtime constraint override cleared.",
      });
      await loadConfig();
    } catch (error) {
      setMessage({
        kind: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Box sx={{ mt: 1 }}>
      <Tabs
        value={tab}
        onChange={(_, value) => setTab(value)}
        sx={{ mb: 2 }}
      >
        <Tab
          value="config"
          icon={<TuneIcon fontSize="small" />}
          iconPosition="start"
          label="Config"
        />
        <Tab
          value="trace"
          icon={<HistoryIcon fontSize="small" />}
          iconPosition="start"
          label="Trace"
        />
      </Tabs>

      {message ? (
        <Alert severity={message.kind} sx={{ mb: 2, borderRadius: 2 }}>
          {message.text}
        </Alert>
      ) : null}

      {tab === "config" && (
        <Stack spacing={2.5}>
          <Alert severity="info" sx={{ borderRadius: 2 }}>
            Runtime constraints shape how the main agent reasons and acts at
            runtime. Use project config for workspace defaults and session
            overrides for per-conversation tuning.
          </Alert>

          {resolvedSummary ? (
            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
              {resolvedSummary.map((item) => (
                <Chip
                  key={item.label}
                  label={`${item.label}: ${item.value}`}
                  variant="outlined"
                />
              ))}
            </Stack>
          ) : null}

          <Card variant="outlined" sx={{ borderRadius: 2.5 }}>
            <CardContent>
              <Stack
                direction="row"
                justifyContent="space-between"
                alignItems="center"
                sx={{ mb: 2 }}
              >
                <Box>
                  <Typography variant="h6" fontWeight={700}>
                    Project defaults
                  </Typography>
                  <Typography variant="body2" color="text.secondary">
                    Stored in <code>.omiga/runtime_constraints.yaml</code>
                  </Typography>
                </Box>
                <Button
                  variant="contained"
                  startIcon={
                    saving ? <CircularProgress size={16} color="inherit" /> : <SaveIcon />
                  }
                  onClick={() => void saveProject()}
                  disabled={saving || loading}
                >
                  Save project
                </Button>
              </Stack>

              <Stack spacing={2}>
                <FormControlLabel
                  control={
                    <Switch
                      checked={projectDraft.enabled}
                      onChange={(_, checked) =>
                        setProjectDraft((prev) => ({ ...prev, enabled: checked }))
                      }
                    />
                  }
                  label="Enable runtime constraint harness"
                />
                <FormControlLabel
                  control={
                    <Switch
                      checked={projectDraft.buffer_responses}
                      onChange={(_, checked) =>
                        setProjectDraft((prev) => ({
                          ...prev,
                          buffer_responses: checked,
                        }))
                      }
                    />
                  }
                  label="Buffer assistant text until validation/commit"
                />
                <FormControl fullWidth size="small">
                  <InputLabel id="project-policy-pack">Policy pack</InputLabel>
                  <Select
                    labelId="project-policy-pack"
                    label="Policy pack"
                    value={projectDraft.policy_pack}
                    onChange={(e) =>
                      setProjectDraft((prev) => ({
                        ...prev,
                        policy_pack: e.target.value as ConstraintPolicyPack,
                      }))
                    }
                  >
                    <MenuItem value="balanced">balanced</MenuItem>
                    <MenuItem value="coding_strict">coding_strict</MenuItem>
                    <MenuItem value="explanation_strict">
                      explanation_strict
                    </MenuItem>
                  </Select>
                </FormControl>

                <Divider />

                <Typography variant="subtitle2" fontWeight={700}>
                  Rule overrides
                </Typography>
                <Stack spacing={1.5}>
                  {(snapshot?.registry ?? []).map((rule) => (
                    <Card key={rule.id} variant="outlined" sx={{ borderRadius: 2 }}>
                      <CardContent sx={{ py: 1.5 }}>
                        <Stack spacing={1.25}>
                          <Stack
                            direction="row"
                            justifyContent="space-between"
                            alignItems="center"
                            gap={2}
                          >
                            <Box>
                              <Typography variant="body2" fontWeight={700}>
                                {rule.id}
                              </Typography>
                              <Typography variant="caption" color="text.secondary">
                                {rule.description}
                              </Typography>
                            </Box>
                            <Chip
                              size="small"
                              color={severityColor(rule.severity)}
                              label={`resolved: ${rule.severity}`}
                            />
                          </Stack>
                          <Stack direction="row" spacing={1.5} alignItems="center">
                            <FormControlLabel
                              control={
                                <Switch
                                  checked={
                                    projectDraft.rules[rule.id]?.enabled ?? rule.enabled
                                  }
                                  onChange={(_, checked) =>
                                    updateRuleDraft("project", rule.id, {
                                      enabled: checked,
                                    })
                                  }
                                />
                              }
                              label="Enabled"
                            />
                            <FormControl size="small" sx={{ minWidth: 180 }}>
                              <InputLabel id={`project-${rule.id}-severity`}>
                                Severity
                              </InputLabel>
                              <Select
                                labelId={`project-${rule.id}-severity`}
                                label="Severity"
                                value={
                                  projectDraft.rules[rule.id]?.severity ?? rule.severity
                                }
                                onChange={(e) =>
                                  updateRuleDraft("project", rule.id, {
                                    severity: e.target.value as ConstraintSeverity,
                                  })
                                }
                              >
                                <MenuItem value="info">info</MenuItem>
                                <MenuItem value="warn">warn</MenuItem>
                                <MenuItem value="error">error</MenuItem>
                              </Select>
                            </FormControl>
                            <Typography variant="caption" color="text.secondary">
                              {rule.phases.join(", ")}
                            </Typography>
                          </Stack>
                        </Stack>
                      </CardContent>
                    </Card>
                  ))}
                </Stack>
              </Stack>
            </CardContent>
          </Card>

          <Card variant="outlined" sx={{ borderRadius: 2.5 }}>
            <CardContent>
              <Stack
                direction="row"
                justifyContent="space-between"
                alignItems="center"
                sx={{ mb: 2 }}
              >
                <Box>
                  <Typography variant="h6" fontWeight={700}>
                    Session override
                  </Typography>
                  <Typography variant="body2" color="text.secondary">
                    Use this when you want a stricter or lighter harness only for
                    the current session.
                  </Typography>
                </Box>
                <Stack direction="row" spacing={1}>
                  <Button
                    variant="outlined"
                    startIcon={<RefreshIcon />}
                    onClick={() => void loadConfig()}
                    disabled={loading || saving}
                  >
                    Reload
                  </Button>
                  <Button
                    variant="contained"
                    startIcon={
                      saving ? <CircularProgress size={16} color="inherit" /> : <SaveIcon />
                    }
                    onClick={() => void saveSession()}
                    disabled={saving || loading || !sessionId}
                  >
                    Save session
                  </Button>
                </Stack>
              </Stack>

              {!sessionId ? (
                <Alert severity="warning" sx={{ borderRadius: 2 }}>
                  Open a session to edit session-specific runtime constraint
                  overrides.
                </Alert>
              ) : (
                <Stack spacing={2}>
                  <FormControlLabel
                    control={
                      <Switch
                        checked={sessionOverrideEnabled}
                        onChange={(_, checked) => setSessionOverrideEnabled(checked)}
                      />
                    }
                    label="Enable session override"
                  />

                  {sessionOverrideEnabled ? (
                    <>
                      <FormControlLabel
                        control={
                          <Switch
                            checked={sessionDraft.enabled}
                            onChange={(_, checked) =>
                              setSessionDraft((prev) => ({
                                ...prev,
                                enabled: checked,
                              }))
                            }
                          />
                        }
                        label="Enable runtime constraint harness"
                      />
                      <FormControlLabel
                        control={
                          <Switch
                            checked={sessionDraft.buffer_responses}
                            onChange={(_, checked) =>
                              setSessionDraft((prev) => ({
                                ...prev,
                                buffer_responses: checked,
                              }))
                            }
                          />
                        }
                        label="Buffer assistant text until validation/commit"
                      />
                      <FormControl fullWidth size="small">
                        <InputLabel id="session-policy-pack">Policy pack</InputLabel>
                        <Select
                          labelId="session-policy-pack"
                          label="Policy pack"
                          value={sessionDraft.policy_pack}
                          onChange={(e) =>
                            setSessionDraft((prev) => ({
                              ...prev,
                              policy_pack: e.target.value as ConstraintPolicyPack,
                            }))
                          }
                        >
                          <MenuItem value="balanced">balanced</MenuItem>
                          <MenuItem value="coding_strict">coding_strict</MenuItem>
                          <MenuItem value="explanation_strict">
                            explanation_strict
                          </MenuItem>
                        </Select>
                      </FormControl>

                      <Divider />
                      <Stack spacing={1.5}>
                        {(snapshot?.registry ?? []).map((rule) => (
                          <Card
                            key={`session-${rule.id}`}
                            variant="outlined"
                            sx={{ borderRadius: 2 }}
                          >
                            <CardContent sx={{ py: 1.5 }}>
                              <Stack
                                direction="row"
                                spacing={1.5}
                                alignItems="center"
                                flexWrap="wrap"
                                useFlexGap
                              >
                                <Typography variant="body2" fontWeight={700}>
                                  {rule.id}
                                </Typography>
                                <FormControlLabel
                                  control={
                                    <Switch
                                      checked={
                                        sessionDraft.rules[rule.id]?.enabled ?? rule.enabled
                                      }
                                      onChange={(_, checked) =>
                                        updateRuleDraft("session", rule.id, {
                                          enabled: checked,
                                        })
                                      }
                                    />
                                  }
                                  label="Enabled"
                                />
                                <FormControl size="small" sx={{ minWidth: 180 }}>
                                  <InputLabel id={`session-${rule.id}-severity`}>
                                    Severity
                                  </InputLabel>
                                  <Select
                                    labelId={`session-${rule.id}-severity`}
                                    label="Severity"
                                    value={
                                      sessionDraft.rules[rule.id]?.severity ?? rule.severity
                                    }
                                    onChange={(e) =>
                                      updateRuleDraft("session", rule.id, {
                                        severity: e.target.value as ConstraintSeverity,
                                      })
                                    }
                                  >
                                    <MenuItem value="info">info</MenuItem>
                                    <MenuItem value="warn">warn</MenuItem>
                                    <MenuItem value="error">error</MenuItem>
                                  </Select>
                                </FormControl>
                              </Stack>
                            </CardContent>
                          </Card>
                        ))}
                      </Stack>
                    </>
                  ) : (
                    <Alert severity="info" sx={{ borderRadius: 2 }}>
                      Session override is disabled. This session currently inherits
                      the project runtime constraint config.
                    </Alert>
                  )}
                </Stack>
              )}
            </CardContent>
          </Card>
        </Stack>
      )}

      {tab === "trace" && (
        <Stack spacing={2.5}>
          {!sessionId ? (
            <Alert severity="warning" sx={{ borderRadius: 2 }}>
              Open a session to inspect runtime constraint traces.
            </Alert>
          ) : (
            <>
              <Stack direction="row" justifyContent="space-between" alignItems="center">
                <Typography variant="body2" color="text.secondary">
                  Recent rounds that emitted runtime constraint events for this
                  session.
                </Typography>
                <Button
                  variant="outlined"
                  startIcon={<RefreshIcon />}
                  onClick={() => void loadTraceRounds()}
                  disabled={traceLoading}
                >
                  Refresh trace
                </Button>
              </Stack>

              <Stack spacing={1.5}>
                {traceRounds.map((round) => (
                  <Card
                    key={round.round_id}
                    variant="outlined"
                    sx={{
                      borderRadius: 2,
                      borderColor:
                        selectedRoundId === round.round_id ? "primary.main" : undefined,
                    }}
                  >
                    <CardContent
                      sx={{ cursor: "pointer" }}
                      onClick={() => setSelectedRoundId(round.round_id)}
                    >
                      <Stack
                        direction="row"
                        justifyContent="space-between"
                        alignItems="center"
                        gap={2}
                      >
                        <Box>
                          <Typography variant="body2" fontWeight={700}>
                            {round.round_id}
                          </Typography>
                          <Typography variant="caption" color="text.secondary">
                            {round.event_count} events · last {formatTs(round.last_event_at)}
                          </Typography>
                        </Box>
                        {selectedRoundId === round.round_id ? (
                          <Chip size="small" color="primary" label="Selected" />
                        ) : null}
                      </Stack>
                    </CardContent>
                  </Card>
                ))}

                {!traceLoading && traceRounds.length === 0 ? (
                  <Alert severity="info" sx={{ borderRadius: 2 }}>
                    No runtime constraint traces have been recorded for this
                    session yet.
                  </Alert>
                ) : null}
              </Stack>

              {traceLoading ? (
                <Box sx={{ display: "flex", justifyContent: "center", py: 4 }}>
                  <CircularProgress size={28} />
                </Box>
              ) : null}

              {traceSummary ? (
                <Card variant="outlined" sx={{ borderRadius: 2.5 }}>
                  <CardContent>
                    <Typography variant="h6" fontWeight={700} sx={{ mb: 1.5 }}>
                      Trace summary
                    </Typography>
                    <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap sx={{ mb: 2 }}>
                      <Chip label={`Events: ${traceSummary.total_events}`} />
                      <Chip label={`First: ${formatTs(traceSummary.first_event_at)}`} />
                      <Chip label={`Last: ${formatTs(traceSummary.last_event_at)}`} />
                    </Stack>

                    <Stack spacing={1.25}>
                      <Typography variant="subtitle2" fontWeight={700}>
                        Constraint hits
                      </Typography>
                      <Typography variant="body2" color="text.secondary">
                        Notices: {traceSummary.noticed_constraints.join(", ") || "—"}
                      </Typography>
                      <Typography variant="body2" color="text.secondary">
                        Gates: {traceSummary.gate_constraints.join(", ") || "—"}
                      </Typography>
                      <Typography variant="body2" color="text.secondary">
                        Retries: {traceSummary.retry_constraints.join(", ") || "—"}
                      </Typography>
                      <Typography variant="body2" color="text.secondary">
                        Commit phases: {traceSummary.commit_phases.join(", ") || "—"}
                      </Typography>
                    </Stack>

                    <Divider sx={{ my: 2 }} />

                    <Typography variant="subtitle2" fontWeight={700} sx={{ mb: 1 }}>
                      Resolved config payload
                    </Typography>
                                <Box
                                  component="pre"
                                  sx={{
                        m: 0,
                        p: 1.5,
                        borderRadius: 2,
                        bgcolor: "background.default",
                        overflow: "auto",
                        fontSize: "0.78rem",
                      }}
                    >
                      {prettyJson(traceSummary.config_payload)}
                    </Box>
                  </CardContent>
                </Card>
              ) : null}

              {traceEvents.length > 0 ? (
                <Card variant="outlined" sx={{ borderRadius: 2.5 }}>
                  <CardContent>
                    <Typography variant="h6" fontWeight={700} sx={{ mb: 1.5 }}>
                      Raw events
                    </Typography>
                    <Stack spacing={1.25}>
                      {traceEvents.map((event) => (
                        <Card
                          key={event.id}
                          variant="outlined"
                          sx={{ borderRadius: 2 }}
                        >
                          <CardContent sx={{ py: 1.5 }}>
                            <Stack spacing={1}>
                              <Stack
                                direction="row"
                                justifyContent="space-between"
                                alignItems="center"
                                gap={2}
                              >
                                <Typography variant="body2" fontWeight={700}>
                                  {event.event_type}
                                </Typography>
                                <Typography variant="caption" color="text.secondary">
                                  {formatTs(event.created_at)}
                                </Typography>
                              </Stack>
                              {event.constraint_id ? (
                                <Chip
                                  size="small"
                                  color="primary"
                                  variant="outlined"
                                  label={event.constraint_id}
                                  sx={{ alignSelf: "flex-start" }}
                                />
                              ) : null}
                              <Box
                                component="pre"
                                sx={{
                                  m: 0,
                                  p: 1.25,
                                  borderRadius: 2,
                                  bgcolor: "background.default",
                                  overflow: "auto",
                                  fontSize: "0.76rem",
                                }}
                                >
                                {prettyJson(parseJsonLoose(event.payload_json))}
                              </Box>
                            </Stack>
                          </CardContent>
                        </Card>
                      ))}
                    </Stack>
                  </CardContent>
                </Card>
              ) : null}
            </>
          )}
        </Stack>
      )}
    </Box>
  );
}
