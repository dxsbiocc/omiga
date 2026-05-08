import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  Collapse,
  CircularProgress,
  Divider,
  FormControl,
  FormControlLabel,
  IconButton,
  InputLabel,
  MenuItem,
  Select,
  Stack,
  Switch,
  Tab,
  Tabs,
  TextField,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import RefreshIcon from "@mui/icons-material/Refresh";
import SaveIcon from "@mui/icons-material/Save";
import HistoryIcon from "@mui/icons-material/History";
import TuneIcon from "@mui/icons-material/Tune";
import AutoFixHighIcon from "@mui/icons-material/AutoFixHigh";
import PolicyIcon from "@mui/icons-material/Policy";
import TimelineIcon from "@mui/icons-material/Timeline";
import CompareArrowsIcon from "@mui/icons-material/CompareArrows";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";

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

type RulePhaseFilter = "all" | "model" | "tool" | "post";
type TraceViewMode = "timeline" | "constraint" | "phase";
type ConfigValueSource = "project" | "session";
type ConfigDiffRow = {
  label: string;
  project: string;
  session: string;
  resolved: string;
  changed: boolean;
  source: ConfigValueSource;
};

type RuleDiffState = {
  projectEnabled: boolean;
  projectSeverity: ConstraintSeverity;
  sessionEnabled: boolean | null;
  sessionSeverity: ConstraintSeverity | null;
  resolvedEnabled: boolean;
  resolvedSeverity: ConstraintSeverity;
  resolvedSource: ConfigValueSource;
  changed: boolean;
};

const POLICY_PACK_META: Record<
  ConstraintPolicyPack,
  { label: string; description: string }
> = {
  balanced: {
    label: "Balanced",
    description:
      "General-purpose defaults. Good for mixed coding, Q&A, and planning sessions.",
  },
  coding_strict: {
    label: "Coding strict",
    description:
      "More defensive around edits and ambiguous implementation requests.",
  },
  explanation_strict: {
    label: "Explanation strict",
    description:
      "Pushes harder on evidence-first answers and explicit uncertainty handling.",
  },
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

function compactId(value: string, keepStart = 8, keepEnd = 6): string {
  if (!value) return "—";
  if (value.length <= keepStart + keepEnd + 3) return value;
  return `${value.slice(0, keepStart)}…${value.slice(-keepEnd)}`;
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

function phaseChipColor(
  phase: string,
): "default" | "primary" | "secondary" | "success" | "warning" {
  if (phase.toLowerCase().includes("tool")) return "warning";
  if (phase.toLowerCase().includes("post")) return "secondary";
  return "primary";
}

function normalizePhaseLabel(phase: string): string {
  switch (phase) {
    case "ModelNotice":
      return "Model notice";
    case "ToolGate":
      return "Tool gate";
    case "PostResponse":
      return "Post-response";
    default:
      return phase;
  }
}

function eventTone(eventType: string): "info" | "warning" | "error" | "success" {
  if (eventType.includes("gate")) return "error";
  if (eventType.includes("retry")) return "warning";
  if (eventType.includes("commit")) return "success";
  return "info";
}

function eventChipColor(
  eventType: string,
): "default" | "info" | "warning" | "error" | "success" {
  return eventTone(eventType);
}

function traceEventGroupPhase(eventType: string): string {
  if (eventType.includes("notice")) return "Model notices";
  if (eventType.includes("gate")) return "Tool gates";
  if (eventType.includes("retry")) return "Post-response retries";
  if (eventType.includes("commit")) return "Buffered commit";
  if (eventType.includes("config")) return "Resolved config";
  return "Other";
}

function configEquals(a: RuntimeConstraintConfig, b: RuntimeConstraintConfig): boolean {
  if (a.enabled !== b.enabled || a.policy_pack !== b.policy_pack) return false;
  const aKeys = Object.keys(a.rules);
  const bKeys = Object.keys(b.rules);
  if (aKeys.length !== bKeys.length) return false;
  for (const k of aKeys) {
    const ra = a.rules[k];
    const rb = b.rules[k];
    if (!rb) return false;
    if (ra.enabled !== rb.enabled || ra.severity !== rb.severity) return false;
  }
  return true;
}

function buildResolvedConfigView(
  snapshot: RuntimeConstraintConfigSnapshot | null,
): RuntimeConstraintConfig | null {
  if (!snapshot) return null;
  return {
    enabled: snapshot.resolved_enabled,
    buffer_responses: snapshot.resolved_buffer_responses,
    policy_pack: snapshot.resolved_policy_pack,
    rules: Object.fromEntries(
      snapshot.registry.map((rule) => [
        rule.id,
        {
          enabled: rule.enabled,
          severity: rule.severity,
        },
      ]),
    ),
  };
}

function ruleOverrideCount(cfg: RuntimeConstraintConfig): number {
  return Object.keys(cfg.rules ?? {}).length;
}

function buildConfigDiffRows(
  projectCfg: RuntimeConstraintConfig,
  sessionCfg: RuntimeConstraintConfig | null,
  resolvedCfg: RuntimeConstraintConfig | null,
) : ConfigDiffRow[] {
  if (!resolvedCfg) return [];
  const resolveSource = (sessionValue: string, resolvedValue: string): ConfigValueSource =>
    sessionCfg && sessionValue !== "—" && sessionValue === resolvedValue
      ? "session"
      : "project";

  const rows: ConfigDiffRow[] = [
    {
      label: "Harness enabled",
      project: projectCfg.enabled ? "On" : "Off",
      session: sessionCfg ? (sessionCfg.enabled ? "On" : "Off") : "—",
      resolved: resolvedCfg.enabled ? "On" : "Off",
      changed: false,
      source: "project",
    },
    {
      label: "Buffered commit",
      project: projectCfg.buffer_responses ? "Buffered" : "Streaming",
      session: sessionCfg
        ? sessionCfg.buffer_responses
          ? "Buffered"
          : "Streaming"
        : "—",
      resolved: resolvedCfg.buffer_responses ? "Buffered" : "Streaming",
      changed: false,
      source: "project",
    },
    {
      label: "Policy pack",
      project: projectCfg.policy_pack,
      session: sessionCfg ? sessionCfg.policy_pack : "—",
      resolved: resolvedCfg.policy_pack,
      changed: false,
      source: "project",
    },
    {
      label: "Rule overrides",
      project: String(ruleOverrideCount(projectCfg)),
      session: sessionCfg ? String(ruleOverrideCount(sessionCfg)) : "—",
      resolved: String(ruleOverrideCount(resolvedCfg)),
      changed: false,
      source: "project",
    },
  ];

  return rows.map((row) => ({
    ...row,
    changed:
      row.project !== row.resolved || (row.session !== "—" && row.session !== row.project),
    source: resolveSource(row.session, row.resolved),
  }));
}

function buildEffectiveRule(
  draft: RuntimeConstraintConfig,
  rule: RuntimeConstraintRuleStatus,
) {
  return {
    enabled: draft.rules[rule.id]?.enabled ?? rule.enabled,
    severity: draft.rules[rule.id]?.severity ?? rule.severity,
  };
}

function buildRuleDiffState(
  rule: RuntimeConstraintRuleStatus,
  projectDraft: RuntimeConstraintConfig,
  sessionDraft: RuntimeConstraintConfig | null,
  sessionOverrideEnabled: boolean,
): RuleDiffState {
  const projectOverride = projectDraft.rules[rule.id] ?? {};
  const sessionOverride =
    sessionOverrideEnabled && sessionDraft ? sessionDraft.rules[rule.id] ?? {} : null;

  const projectEnabled = projectOverride.enabled ?? rule.enabled;
  const projectSeverity = projectOverride.severity ?? rule.severity;
  const sessionEnabled =
    sessionOverride && sessionOverride.enabled != null ? sessionOverride.enabled : null;
  const sessionSeverity =
    sessionOverride && sessionOverride.severity != null ? sessionOverride.severity : null;
  const resolvedEnabled = sessionEnabled ?? projectEnabled;
  const resolvedSeverity = sessionSeverity ?? projectSeverity;
  const resolvedSource: ConfigValueSource =
    sessionOverrideEnabled &&
    ((sessionEnabled != null && sessionEnabled === resolvedEnabled) ||
      (sessionSeverity != null && sessionSeverity === resolvedSeverity))
      ? "session"
      : "project";

  return {
    projectEnabled,
    projectSeverity,
    sessionEnabled,
    sessionSeverity,
    resolvedEnabled,
    resolvedSeverity,
    resolvedSource,
    changed:
      resolvedEnabled !== rule.enabled ||
      resolvedSeverity !== rule.severity ||
      sessionEnabled != null ||
      sessionSeverity != null,
  };
}

function phaseFilterMatches(rule: RuntimeConstraintRuleStatus, filter: RulePhaseFilter) {
  if (filter === "all") return true;
  const phases = rule.phases.join(" ").toLowerCase();
  if (filter === "model") return phases.includes("model");
  if (filter === "tool") return phases.includes("tool");
  return phases.includes("post");
}

function ScopeSummaryCard({
  title,
  subtitle,
  tone,
  children,
  actions,
}: {
  title: string;
  subtitle: string;
  tone?: "default" | "resolved" | "override";
  children: React.ReactNode;
  actions?: React.ReactNode;
}) {
  const theme = useTheme();
  const borderColor =
    tone === "resolved"
      ? alpha(theme.palette.primary.main, 0.45)
      : tone === "override"
        ? alpha(theme.palette.warning.main, 0.4)
        : alpha(theme.palette.divider, 1);

  return (
    <Card
      variant="outlined"
      sx={{
        borderRadius: 3,
        borderColor,
        background:
          tone === "resolved"
            ? alpha(theme.palette.primary.main, 0.035)
            : tone === "override"
              ? alpha(theme.palette.warning.main, 0.04)
              : "background.paper",
      }}
    >
      <CardContent>
        <Stack
          direction={{ xs: "column", md: "row" }}
          justifyContent="space-between"
          alignItems={{ xs: "flex-start", md: "center" }}
          gap={2}
          sx={{ mb: 2 }}
        >
          <Box>
            <Typography variant="h6" fontWeight={700}>
              {title}
            </Typography>
            <Typography variant="body2" color="text.secondary">
              {subtitle}
            </Typography>
          </Box>
          {actions}
        </Stack>
        {children}
      </CardContent>
    </Card>
  );
}

function RuleEditorCard({
  scope,
  rule,
  draft,
  diffState,
  onChange,
}: {
  scope: "project" | "session";
  rule: RuntimeConstraintRuleStatus;
  draft: RuntimeConstraintConfig;
  diffState: RuleDiffState;
  onChange: (patch: Partial<RuntimeConstraintRuleConfig>) => void;
}) {
  const theme = useTheme();
  const [expanded, setExpanded] = useState(diffState.changed);
  const effective = buildEffectiveRule(draft, rule);

  return (
    <Card variant="outlined" sx={{ borderRadius: 2.5 }}>
      <CardContent sx={{ py: 1.75 }}>
        <Stack spacing={1.5}>
          <Stack
            direction={{ xs: "column", md: "row" }}
            justifyContent="space-between"
            alignItems={{ xs: "flex-start", md: "center" }}
            gap={1.5}
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
              color={severityColor(diffState.resolvedSeverity)}
              label={`resolved: ${diffState.resolvedSeverity}`}
            />
          </Stack>

          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap alignItems="center">
            {rule.phases.map((phase) => (
              <Chip
                key={`${rule.id}-${phase}`}
                size="small"
                color={phaseChipColor(phase)}
                variant="outlined"
                label={normalizePhaseLabel(phase)}
              />
            ))}
            <IconButton
              size="small"
              onClick={() => setExpanded((prev) => !prev)}
              sx={{
                ml: "auto",
                transform: expanded ? "rotate(180deg)" : "rotate(0deg)",
                transition: "transform 0.2s ease",
              }}
            >
              <ExpandMoreIcon fontSize="inherit" />
            </IconButton>
          </Stack>

          <Stack
            direction={{ xs: "column", md: "row" }}
            spacing={1.5}
            alignItems={{ xs: "flex-start", md: "center" }}
          >
            <FormControlLabel
              control={
                <Switch
                  checked={effective.enabled}
                  onChange={(_, checked) => onChange({ enabled: checked })}
                />
              }
              label={scope === "project" ? "Enabled by default" : "Override enabled"}
            />
            <FormControl size="small" sx={{ minWidth: 180 }}>
              <InputLabel id={`${scope}-${rule.id}-severity`}>Severity</InputLabel>
              <Select
                labelId={`${scope}-${rule.id}-severity`}
                label="Severity"
                value={effective.severity}
                onChange={(e) =>
                  onChange({ severity: e.target.value as ConstraintSeverity })
                }
              >
                <MenuItem value="info">info</MenuItem>
                <MenuItem value="warn">warn</MenuItem>
                <MenuItem value="error">error</MenuItem>
              </Select>
            </FormControl>
          </Stack>

          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            <Chip
              size="small"
              color={diffState.resolvedSource === "session" ? "warning" : "primary"}
              variant="outlined"
              label={`resolved from ${diffState.resolvedSource}`}
            />
            {diffState.changed ? (
              <Chip size="small" color="warning" label="rule changed" />
            ) : (
              <Chip size="small" variant="outlined" label="matches baseline" />
            )}
          </Stack>

          <Collapse in={expanded}>
            <Box
              sx={{
                display: "grid",
                gridTemplateColumns: { xs: "1fr", md: "repeat(3, minmax(0, 1fr))" },
                gap: 1.25,
                pt: 0.5,
              }}
            >
              {[
                {
                  label: "Project",
                  enabled: diffState.projectEnabled,
                  severity: diffState.projectSeverity,
                },
                {
                  label: "Session",
                  enabled: diffState.sessionEnabled,
                  severity: diffState.sessionSeverity,
                },
                {
                  label: "Resolved",
                  enabled: diffState.resolvedEnabled,
                  severity: diffState.resolvedSeverity,
                },
              ].map((cell) => (
                <Box
                  key={`${rule.id}-${cell.label}`}
                  sx={{
                    p: 1,
                    borderRadius: 2,
                    bgcolor:
                      cell.label === "Resolved"
                        ? alpha(theme.palette.primary.main, 0.08)
                        : "background.default",
                    border: "1px solid",
                    borderColor:
                      cell.label === "Resolved"
                        ? alpha(theme.palette.primary.main, 0.18)
                        : alpha(theme.palette.divider, 1),
                  }}
                >
                  <Typography variant="caption" color="text.secondary">
                    {cell.label}
                  </Typography>
                  <Typography variant="body2" fontWeight={600}>
                    Enabled:{" "}
                    {cell.enabled == null ? "—" : cell.enabled ? "On" : "Off"}
                  </Typography>
                  <Typography variant="body2" fontWeight={600}>
                    Severity: {cell.severity ?? "—"}
                  </Typography>
                </Box>
              ))}
            </Box>
          </Collapse>
        </Stack>
      </CardContent>
    </Card>
  );
}

export function RuntimeConstraintsPanel({
  projectPath,
  sessionId,
}: {
  projectPath: string;
  sessionId: string | null;
}) {
  const theme = useTheme();
  const [tab, setTab] = useState<"config" | "trace">("config");
  const [rulePhaseFilter, setRulePhaseFilter] = useState<RulePhaseFilter>("all");
  const [showChangedOnly, setShowChangedOnly] = useState(false);
  const [showChangedRulesOnly, setShowChangedRulesOnly] = useState(false);
  const [showRawConfigSnapshots, setShowRawConfigSnapshots] = useState(false);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{
    kind: "success" | "error";
    text: string;
  } | null>(null);
  const [snapshot, setSnapshot] =
    useState<RuntimeConstraintConfigSnapshot | null>(null);
  const [projectDraft, setProjectDraft] =
    useState<RuntimeConstraintConfig>(defaultConfig());
  const [sessionOverrideEnabled, setSessionOverrideEnabled] = useState(false);
  const [sessionDraft, setSessionDraft] =
    useState<RuntimeConstraintConfig>(defaultConfig());
  const [traceLoading, setTraceLoading] = useState(false);
  const [traceRounds, setTraceRounds] = useState<RuntimeConstraintTraceRound[]>([]);
  const [selectedRoundId, setSelectedRoundId] = useState<string>("");
  const [traceSummary, setTraceSummary] =
    useState<RuntimeConstraintTraceSummary | null>(null);
  const [traceEvents, setTraceEvents] = useState<RuntimeConstraintTraceEvent[]>([]);
  const [expandedEventIds, setExpandedEventIds] = useState<Record<string, boolean>>(
    {},
  );
  const [traceViewMode, setTraceViewMode] = useState<TraceViewMode>("timeline");
  const [traceEventTypeFilter, setTraceEventTypeFilter] = useState<string>("all");
  const [traceConstraintFilter, setTraceConstraintFilter] = useState<string>("all");
  const [traceTextFilter, setTraceTextFilter] = useState("");

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
          limit: 30,
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

  const loadTraceDetails = useCallback(async (roundId: string) => {
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
          { roundId },
        ),
        invoke<RuntimeConstraintTraceEvent[]>("get_runtime_constraint_trace", {
          roundId,
        }),
      ]);
      setTraceSummary(summary);
      setTraceEvents(events);
      setExpandedEventIds(
        Object.fromEntries(
          events.map((event) => [
            event.id,
            event.event_type.includes("gate") || event.event_type.includes("retry"),
          ]),
        ),
      );
    } catch (error) {
      setMessage({
        kind: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setTraceLoading(false);
    }
  }, []);

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
    if (!snapshot) return [];
    return [
      {
        label: "Harness",
        value: snapshot.resolved_enabled ? "Enabled" : "Disabled",
      },
      {
        label: "Commit mode",
        value: snapshot.resolved_buffer_responses ? "Buffered" : "Streaming",
      },
      {
        label: "Policy pack",
        value: snapshot.resolved_policy_pack,
      },
      {
        label: "Registered rules",
        value: String(snapshot.registry.length),
      },
    ];
  }, [snapshot]);

  const resolvedConfigView = useMemo(
    () => buildResolvedConfigView(snapshot),
    [snapshot],
  );

  const configDiffRows = useMemo(() => {
    if (!snapshot) return [];
    return buildConfigDiffRows(
      snapshot.project_config,
      snapshot.session_config,
      resolvedConfigView,
    );
  }, [resolvedConfigView, snapshot]);

  const visibleConfigDiffRows = useMemo(
    () => (showChangedOnly ? configDiffRows.filter((row) => row.changed) : configDiffRows),
    [configDiffRows, showChangedOnly],
  );

  const filteredRules = useMemo(
    () =>
      (snapshot?.registry ?? []).filter((rule) =>
        phaseFilterMatches(rule, rulePhaseFilter),
      ),
    [rulePhaseFilter, snapshot],
  );

  const projectRuleDiffMap = useMemo(
    () =>
      Object.fromEntries(
        filteredRules.map((rule) => [
          rule.id,
          buildRuleDiffState(rule, projectDraft, null, false),
        ]),
      ) as Record<string, RuleDiffState>,
    [filteredRules, projectDraft],
  );

  const sessionRuleDiffMap = useMemo(
    () =>
      Object.fromEntries(
        filteredRules.map((rule) => [
          rule.id,
          buildRuleDiffState(rule, projectDraft, sessionDraft, sessionOverrideEnabled),
        ]),
      ) as Record<string, RuleDiffState>,
    [filteredRules, projectDraft, sessionDraft, sessionOverrideEnabled],
  );

  const visibleProjectRules = useMemo(
    () =>
      showChangedRulesOnly
        ? filteredRules.filter((rule) => projectRuleDiffMap[rule.id]?.changed)
        : filteredRules,
    [filteredRules, projectRuleDiffMap, showChangedRulesOnly],
  );

  const visibleSessionRules = useMemo(
    () =>
      showChangedRulesOnly
        ? filteredRules.filter((rule) => sessionRuleDiffMap[rule.id]?.changed)
        : filteredRules,
    [filteredRules, sessionRuleDiffMap, showChangedRulesOnly],
  );

  const policyDescription = useMemo(() => {
    const policy =
      POLICY_PACK_META[
        (snapshot?.resolved_policy_pack ?? "balanced") as ConstraintPolicyPack
      ];
    return policy?.description ?? "";
  }, [snapshot?.resolved_policy_pack]);

  const projectDirty = useMemo(
    () =>
      snapshot ? !configEquals(projectDraft, snapshot.project_config) : false,
    [projectDraft, snapshot],
  );

  const sessionDirty = useMemo(() => {
    if (!snapshot) return false;
    const source = snapshot.session_config ?? snapshot.project_config;
    if (!sessionOverrideEnabled) return Boolean(snapshot.session_config);
    return !configEquals(sessionDraft, source);
  }, [sessionDraft, sessionOverrideEnabled, snapshot]);

  const traceEventTypes = useMemo(
    () => Array.from(new Set(traceEvents.map((event) => event.event_type))).sort(),
    [traceEvents],
  );

  const traceConstraintIds = useMemo(
    () =>
      Array.from(
        new Set(
          traceEvents
            .map((event) => event.constraint_id)
            .filter((value): value is string => Boolean(value)),
        ),
      ).sort(),
    [traceEvents],
  );

  const filteredTraceEvents = useMemo(() => {
    return traceEvents.filter((event) => {
      if (
        traceEventTypeFilter !== "all" &&
        event.event_type !== traceEventTypeFilter
      ) {
        return false;
      }
      if (
        traceConstraintFilter !== "all" &&
        (event.constraint_id ?? "none") !== traceConstraintFilter
      ) {
        return false;
      }
      if (traceTextFilter.trim()) {
        const haystack = `${event.event_type} ${event.constraint_id ?? ""} ${
          event.payload_json
        }`.toLowerCase();
        if (!haystack.includes(traceTextFilter.trim().toLowerCase())) {
          return false;
        }
      }
      return true;
    });
  }, [
    traceConstraintFilter,
    traceEventTypeFilter,
    traceEvents,
    traceTextFilter,
  ]);

  const groupedTraceByConstraint = useMemo(() => {
    return Object.entries(
      filteredTraceEvents.reduce<Record<string, RuntimeConstraintTraceEvent[]>>(
        (acc, event) => {
          const key = event.constraint_id ?? "unscoped";
          if (!acc[key]) acc[key] = [];
          acc[key].push(event);
          return acc;
        },
        {},
      ),
    ).sort(([a], [b]) => a.localeCompare(b));
  }, [filteredTraceEvents]);

  const groupedTraceByPhase = useMemo(() => {
    return Object.entries(
      filteredTraceEvents.reduce<Record<string, RuntimeConstraintTraceEvent[]>>(
        (acc, event) => {
          const key = traceEventGroupPhase(event.event_type);
          if (!acc[key]) acc[key] = [];
          acc[key].push(event);
          return acc;
        },
        {},
      ),
    ).sort(([a], [b]) => a.localeCompare(b));
  }, [filteredTraceEvents]);

  const toggleEventExpanded = useCallback((id: string) => {
    setExpandedEventIds((prev) => ({ ...prev, [id]: !prev[id] }));
  }, []);

  const updateRuleDraft = useCallback(
    (
      scope: "project" | "session",
      id: string,
      patch: Partial<RuntimeConstraintRuleConfig>,
    ) => {
      const updater = (
        prev: RuntimeConstraintConfig,
      ): RuntimeConstraintConfig => ({
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
    },
    [],
  );

  const saveProject = async () => {
    setSaving(true);
    setMessage(null);
    try {
      await invoke("save_project_runtime_constraints_config", {
        projectPath,
        config: projectDraft,
      });
      setMessage({
        kind: "success",
        text: "Project runtime constraints saved.",
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
    <Box
      sx={{
        mt: 1,
        height: "100%",
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <Tabs value={tab} onChange={(_, value) => setTab(value)} sx={{ mb: 2.5 }}>
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
        <Alert severity={message.kind} sx={{ mb: 2.5, borderRadius: 2.5 }}>
          {message.text}
        </Alert>
      ) : null}

      {tab === "config" && (
        <Stack spacing={2.5}>
          <Alert severity="info" sx={{ borderRadius: 2.5 }}>
            Runtime constraints are the agent harness guardrails: they shape
            what gets checked before an answer is committed, when edits are
            blocked pending clarification, and how strict evidence-first behavior
            should be.
          </Alert>

          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            {resolvedSummary.map((item) => (
              <Chip
                key={item.label}
                icon={<AutoFixHighIcon />}
                label={`${item.label}: ${item.value}`}
                variant="outlined"
              />
            ))}
          </Stack>

          <Alert severity="info" sx={{ borderRadius: 2.5 }}>
            <Stack spacing={0.5}>
              <Stack direction="row" spacing={1} alignItems="center" useFlexGap>
                <PolicyIcon fontSize="small" />
                <Typography variant="body2" fontWeight={700}>
                  Resolved harness profile
                </Typography>
                <Chip
                  size="small"
                  color="primary"
                  label={snapshot?.resolved_policy_pack ?? "balanced"}
                />
              </Stack>
              <Typography variant="body2" color="text.secondary">
                {policyDescription}
              </Typography>
              <Typography variant="caption" color="text.secondary">
                Resolution order: default pack → project config → session override.
              </Typography>
            </Stack>
          </Alert>

          {snapshot && resolvedConfigView ? (
            <ScopeSummaryCard
              title="Config diff view"
              subtitle="Compare project defaults, session override, and the final resolved harness state."
              tone="resolved"
            >
              <Stack spacing={2}>
                <Stack
                  direction={{ xs: "column", md: "row" }}
                  justifyContent="space-between"
                  alignItems={{ xs: "flex-start", md: "center" }}
                  gap={1.5}
                >
                  <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                    {configDiffRows.map((row) => (
                      <Chip
                        key={row.label}
                        icon={<CompareArrowsIcon />}
                        variant={row.changed ? "filled" : "outlined"}
                        color={row.changed ? "warning" : "default"}
                        label={`${row.label}: ${row.project} → ${row.resolved}`}
                      />
                    ))}
                  </Stack>
                  <FormControlLabel
                    control={
                      <Switch
                        checked={showChangedOnly}
                        onChange={(_, checked) => setShowChangedOnly(checked)}
                      />
                    }
                    label="Show changed fields only"
                  />
                </Stack>

                <Stack spacing={1.25}>
                  {showChangedOnly && visibleConfigDiffRows.length === 0 ? (
                    <Alert severity="success" sx={{ borderRadius: 2 }}>
                      No config-level differences are currently active. The resolved
                      harness matches the project defaults for the top-level fields shown here.
                    </Alert>
                  ) : null}
                  {visibleConfigDiffRows.map((row) => (
                    <Card
                      key={`diff-${row.label}`}
                      variant="outlined"
                      sx={{
                        borderRadius: 2.5,
                        borderColor: row.changed
                          ? alpha(theme.palette.warning.main, 0.4)
                          : alpha(theme.palette.divider, 1),
                        background: row.changed
                          ? alpha(theme.palette.warning.main, 0.04)
                          : "background.paper",
                      }}
                    >
                      <CardContent sx={{ py: 1.5 }}>
                        <Stack spacing={1}>
                          <Stack
                            direction={{ xs: "column", md: "row" }}
                            justifyContent="space-between"
                            alignItems={{ xs: "flex-start", md: "center" }}
                            gap={1}
                          >
                            <Typography variant="body2" fontWeight={700}>
                              {row.label}
                            </Typography>
                            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                              <Chip
                                size="small"
                                color={row.source === "session" ? "warning" : "primary"}
                                variant="outlined"
                                label={`resolved from ${row.source}`}
                              />
                              {row.changed ? (
                                <Chip
                                  size="small"
                                  color="warning"
                                  label="changed"
                                />
                              ) : (
                                <Chip size="small" variant="outlined" label="same as project" />
                              )}
                            </Stack>
                          </Stack>
                          <Box
                            sx={{
                              display: "grid",
                              gridTemplateColumns: {
                                xs: "1fr",
                                md: "repeat(3, minmax(0, 1fr))",
                              },
                              gap: 1.5,
                            }}
                          >
                            {[
                              { title: "Project", value: row.project },
                              { title: "Session", value: row.session },
                              { title: "Resolved", value: row.resolved },
                            ].map((cell) => (
                              <Box
                                key={`${row.label}-${cell.title}`}
                                sx={{
                                  p: 1.25,
                                  borderRadius: 2,
                                  bgcolor:
                                    cell.title === "Resolved"
                                      ? alpha(theme.palette.primary.main, 0.08)
                                      : "background.default",
                                  border: "1px solid",
                                  borderColor:
                                    cell.title === "Resolved"
                                      ? alpha(theme.palette.primary.main, 0.18)
                                      : alpha(theme.palette.divider, 1),
                                }}
                              >
                                <Typography
                                  variant="caption"
                                  color="text.secondary"
                                  sx={{ display: "block", mb: 0.5 }}
                                >
                                  {cell.title}
                                </Typography>
                                <Typography variant="body2" fontWeight={600}>
                                  {cell.value}
                                </Typography>
                              </Box>
                            ))}
                          </Box>
                        </Stack>
                      </CardContent>
                    </Card>
                  ))}
                </Stack>

                <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                  <Chip
                    icon={<ExpandMoreIcon />}
                    variant={showRawConfigSnapshots ? "filled" : "outlined"}
                    color={showRawConfigSnapshots ? "primary" : "default"}
                    label={showRawConfigSnapshots ? "Hide raw snapshots" : "Show raw snapshots"}
                    onClick={() => setShowRawConfigSnapshots((prev) => !prev)}
                  />
                </Stack>

                <Collapse in={showRawConfigSnapshots}>
                  <Box
                    sx={{
                      display: "grid",
                      gridTemplateColumns: {
                        xs: "1fr",
                        xl: "repeat(3, minmax(0, 1fr))",
                      },
                      gap: 2,
                    }}
                  >
                    {[
                      {
                        title: "Project",
                        subtitle: ".omiga/runtime_constraints.yaml",
                        value: snapshot.project_config,
                      },
                      {
                        title: "Session",
                        subtitle: sessionOverrideEnabled
                          ? "Per-session override"
                          : "Inherited / disabled",
                        value: sessionOverrideEnabled
                          ? sessionDraft
                          : snapshot.session_config,
                      },
                      {
                        title: "Resolved",
                        subtitle: "Effective state used by the runtime harness",
                        value: resolvedConfigView,
                      },
                    ].map((column) => (
                      <Card
                        key={column.title}
                        variant="outlined"
                        sx={{ borderRadius: 2.5 }}
                      >
                        <CardContent>
                          <Typography variant="subtitle2" fontWeight={700}>
                            {column.title}
                          </Typography>
                          <Typography
                            variant="caption"
                            color="text.secondary"
                            sx={{ display: "block", mb: 1.25 }}
                          >
                            {column.subtitle}
                          </Typography>
                          <Box
                            component="pre"
                            sx={{
                              m: 0,
                              p: 1.25,
                              borderRadius: 2,
                              bgcolor: "background.default",
                              overflow: "auto",
                              fontSize: "0.75rem",
                            }}
                          >
                            {prettyJson(column.value ?? "—")}
                          </Box>
                        </CardContent>
                      </Card>
                    ))}
                  </Box>
                </Collapse>
              </Stack>
            </ScopeSummaryCard>
          ) : null}

          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            <Chip
              label="All phases"
              color={rulePhaseFilter === "all" ? "primary" : "default"}
              variant={rulePhaseFilter === "all" ? "filled" : "outlined"}
              onClick={() => setRulePhaseFilter("all")}
            />
            <Chip
              label="Model notices"
              color={rulePhaseFilter === "model" ? "primary" : "default"}
              variant={rulePhaseFilter === "model" ? "filled" : "outlined"}
              onClick={() => setRulePhaseFilter("model")}
            />
            <Chip
              label="Tool gates"
              color={rulePhaseFilter === "tool" ? "warning" : "default"}
              variant={rulePhaseFilter === "tool" ? "filled" : "outlined"}
              onClick={() => setRulePhaseFilter("tool")}
            />
            <Chip
              label="Post-response"
              color={rulePhaseFilter === "post" ? "secondary" : "default"}
              variant={rulePhaseFilter === "post" ? "filled" : "outlined"}
              onClick={() => setRulePhaseFilter("post")}
            />
            <Chip
              label={showChangedRulesOnly ? "Changed rules only" : "All rules"}
              color={showChangedRulesOnly ? "warning" : "default"}
              variant={showChangedRulesOnly ? "filled" : "outlined"}
              onClick={() => setShowChangedRulesOnly((prev) => !prev)}
            />
          </Stack>

          <ScopeSummaryCard
            title="Project defaults"
            subtitle="Stored in .omiga/runtime_constraints.yaml. These values apply to all sessions in the workspace unless overridden."
            actions={
              <Stack direction="row" spacing={1}>
                <Button
                  variant="outlined"
                  startIcon={<RefreshIcon />}
                  onClick={() =>
                    snapshot && setProjectDraft(cloneConfig(snapshot.project_config))
                  }
                  disabled={loading || saving || !snapshot || !projectDirty}
                >
                  Reset
                </Button>
                <Button
                  variant="contained"
                  startIcon={
                    saving ? (
                      <CircularProgress size={16} color="inherit" />
                    ) : (
                      <SaveIcon />
                    )
                  }
                  onClick={() => void saveProject()}
                  disabled={saving || loading || !projectDirty}
                >
                  Save project
                </Button>
              </Stack>
            }
          >
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
                label="Buffer assistant text until validation / commit"
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
                  {Object.entries(POLICY_PACK_META).map(([id, meta]) => (
                    <MenuItem key={id} value={id}>
                      <Box>
                        <Typography variant="body2" fontWeight={700}>
                          {meta.label}
                        </Typography>
                        <Typography variant="caption" color="text.secondary">
                          {meta.description}
                        </Typography>
                      </Box>
                    </MenuItem>
                  ))}
                </Select>
              </FormControl>

              <Divider />

              <Typography variant="subtitle2" fontWeight={700}>
                Rule overrides ({visibleProjectRules.length})
              </Typography>
              <Stack spacing={1.5}>
                {showChangedRulesOnly && visibleProjectRules.length === 0 ? (
                  <Alert severity="success" sx={{ borderRadius: 2 }}>
                    No project-level rule differences are active for the selected filter.
                  </Alert>
                ) : null}
                {visibleProjectRules.map((rule) => (
                  <RuleEditorCard
                    key={`project-${rule.id}`}
                    scope="project"
                    rule={rule}
                    draft={projectDraft}
                    diffState={projectRuleDiffMap[rule.id]}
                    onChange={(patch) => updateRuleDraft("project", rule.id, patch)}
                  />
                ))}
              </Stack>
            </Stack>
          </ScopeSummaryCard>

          <ScopeSummaryCard
            title="Session override"
            subtitle={
              sessionId
                ? "Use this to make one conversation stricter or lighter without touching workspace defaults."
                : "Open a session to edit session-specific runtime constraint overrides."
            }
            tone="override"
            actions={
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
                    saving ? (
                      <CircularProgress size={16} color="inherit" />
                    ) : (
                      <SaveIcon />
                    )
                  }
                  onClick={() => void saveSession()}
                  disabled={saving || loading || !sessionId || !sessionDirty}
                >
                  Save session
                </Button>
              </Stack>
            }
          >
            {!sessionId ? (
              <Alert severity="warning" sx={{ borderRadius: 2 }}>
                No active session — session override controls are disabled.
              </Alert>
            ) : (
              <Stack spacing={2}>
                <FormControlLabel
                  control={
                    <Switch
                      checked={sessionOverrideEnabled}
                      onChange={(_, checked) =>
                        setSessionOverrideEnabled(checked)
                      }
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
                      label="Buffer assistant text until validation / commit"
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
                        {Object.entries(POLICY_PACK_META).map(([id, meta]) => (
                          <MenuItem key={id} value={id}>
                            <Box>
                              <Typography variant="body2" fontWeight={700}>
                                {meta.label}
                              </Typography>
                              <Typography variant="caption" color="text.secondary">
                                {meta.description}
                              </Typography>
                            </Box>
                          </MenuItem>
                        ))}
                      </Select>
                    </FormControl>

                    <Divider />

                    <Typography variant="subtitle2" fontWeight={700}>
                      Session rule overrides ({visibleSessionRules.length})
                    </Typography>
                    <Stack spacing={1.5}>
                      {showChangedRulesOnly && visibleSessionRules.length === 0 ? (
                        <Alert severity="info" sx={{ borderRadius: 2 }}>
                          No session-level rule differences are active for the selected filter.
                        </Alert>
                      ) : null}
                      {visibleSessionRules.map((rule) => (
                        <RuleEditorCard
                          key={`session-${rule.id}`}
                          scope="session"
                          rule={rule}
                          draft={sessionDraft}
                          diffState={sessionRuleDiffMap[rule.id]}
                          onChange={(patch) =>
                            updateRuleDraft("session", rule.id, patch)
                          }
                        />
                      ))}
                    </Stack>
                  </>
                ) : (
                  <Alert severity="info" sx={{ borderRadius: 2 }}>
                    Session override is disabled. This conversation inherits the
                    project harness configuration unchanged.
                  </Alert>
                )}
              </Stack>
            )}
          </ScopeSummaryCard>
        </Stack>
      )}

      {tab === "trace" && (
        <Box
          sx={{
            flex: 1,
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
          }}
        >
          <Stack spacing={2.5} sx={{ minHeight: 0 }}>
          {!sessionId ? (
            <Alert severity="warning" sx={{ borderRadius: 2.5 }}>
              Open a session to inspect runtime constraint traces.
            </Alert>
          ) : (
            <>
              <Card
                variant="outlined"
                sx={{
                  borderRadius: 3,
                  borderColor: alpha(theme.palette.info.main, 0.3),
                  background: alpha(theme.palette.info.main, 0.03),
                }}
              >
                <CardContent>
                  <Stack
                    direction={{ xs: "column", md: "row" }}
                    justifyContent="space-between"
                    alignItems={{ xs: "flex-start", md: "center" }}
                    gap={2}
                  >
                    <Box>
                      <Stack direction="row" spacing={1} alignItems="center">
                        <TimelineIcon color="info" />
                        <Typography variant="h6" fontWeight={700}>
                          Runtime trace inspector
                        </Typography>
                      </Stack>
                      <Typography variant="body2" color="text.secondary" sx={{ mt: 0.75 }}>
                        Inspect which constraints fired, when buffering committed,
                        and whether the round retried or gated before completing.
                      </Typography>
                    </Box>
                    <Button
                      variant="outlined"
                      startIcon={<RefreshIcon />}
                      onClick={() => void loadTraceRounds()}
                      disabled={traceLoading}
                    >
                      Refresh trace
                    </Button>
                  </Stack>
                </CardContent>
              </Card>

              <Stack
                direction={{ xs: "column", lg: "row" }}
                spacing={2}
                alignItems="stretch"
                sx={{ flex: 1, minHeight: 0 }}
              >
                <Box
                  sx={{
                    width: { xs: "100%", lg: 340 },
                    flexShrink: 0,
                    minHeight: 0,
                    display: "flex",
                    flexDirection: "column",
                  }}
                >
                  <ScopeSummaryCard
                    title="Recent traced rounds"
                    subtitle="The most recent rounds in this session that emitted runtime harness events."
                  >
                    <Stack
                      spacing={1.25}
                      sx={{
                        maxHeight: { xs: 320, lg: "calc(100vh - 360px)" },
                        minHeight: 0,
                        overflow: "auto",
                        pr: 0.5,
                      }}
                    >
                      {traceRounds.map((round) => (
                        <Card
                          key={round.round_id}
                          variant="outlined"
                          sx={{
                            borderRadius: 2.5,
                            cursor: "pointer",
                            borderColor:
                              selectedRoundId === round.round_id
                                ? "primary.main"
                                : undefined,
                            background:
                              selectedRoundId === round.round_id
                                ? alpha(theme.palette.primary.main, 0.05)
                                : undefined,
                          }}
                          onClick={() => setSelectedRoundId(round.round_id)}
                        >
                          <CardContent sx={{ py: 1.5 }}>
                            <Stack spacing={0.75}>
                              <Stack
                                direction="row"
                                justifyContent="space-between"
                                alignItems="flex-start"
                                gap={1}
                              >
                                <Box sx={{ minWidth: 0, flex: 1 }}>
                                  <Typography
                                    variant="body2"
                                    fontWeight={700}
                                    sx={{ lineHeight: 1.3 }}
                                  >
                                    {compactId(round.round_id, 12, 8)}
                                  </Typography>
                                </Box>
                                <Chip
                                  size="small"
                                  label={`${round.event_count} events`}
                                  color={
                                    selectedRoundId === round.round_id
                                      ? "primary"
                                      : "default"
                                  }
                                  variant={
                                    selectedRoundId === round.round_id
                                      ? "filled"
                                      : "outlined"
                                  }
                                  sx={{ flexShrink: 0 }}
                                />
                              </Stack>
                              <Typography variant="caption" color="text.secondary">
                                Last activity · {formatTs(round.last_event_at)}
                              </Typography>
                            </Stack>
                          </CardContent>
                        </Card>
                      ))}

                      {!traceLoading && traceRounds.length === 0 ? (
                        <Alert severity="info" sx={{ borderRadius: 2 }}>
                          No traced rounds have been recorded for this session yet.
                        </Alert>
                      ) : null}
                    </Stack>
                  </ScopeSummaryCard>
                </Box>

                <Box
                  sx={{
                    flex: 1,
                    minWidth: 0,
                    minHeight: 0,
                    overflow: "auto",
                    pr: 0.5,
                  }}
                >
                  {traceLoading ? (
                    <Box sx={{ display: "flex", justifyContent: "center", py: 6 }}>
                      <CircularProgress size={28} />
                    </Box>
                  ) : (
                    <Stack spacing={2}>
                      {traceSummary ? (
                        <ScopeSummaryCard
                          title="Round summary"
                          subtitle={`Selected round · ${compactId(traceSummary.round_id, 12, 8)}`}
                          tone="resolved"
                        >
                          <Stack spacing={2}>
                            <Box
                              sx={{
                                p: 1.25,
                                borderRadius: 2,
                                bgcolor: "background.default",
                              }}
                            >
                              <Typography
                                variant="caption"
                                color="text.secondary"
                                sx={{ display: "block", mb: 0.5 }}
                              >
                                Full round ID
                              </Typography>
                              <Typography
                                variant="body2"
                                fontWeight={600}
                                sx={{
                                  fontFamily:
                                    'ui-monospace, SFMono-Regular, Menlo, monospace',
                                  overflowWrap: "anywhere",
                                  lineHeight: 1.45,
                                }}
                              >
                                {traceSummary.round_id}
                              </Typography>
                            </Box>

                            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                              <Chip label={`Events: ${traceSummary.total_events}`} />
                              <Chip label={`First: ${formatTs(traceSummary.first_event_at)}`} />
                              <Chip label={`Last: ${formatTs(traceSummary.last_event_at)}`} />
                            </Stack>

                            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                              {Object.entries(traceSummary.event_type_counts).map(
                                ([eventType, count]) => (
                                  <Chip
                                    key={eventType}
                                    color={eventChipColor(eventType)}
                                    variant="outlined"
                                    label={`${eventType}: ${count}`}
                                  />
                                ),
                              )}
                            </Stack>

                            <Stack spacing={1}>
                              <Typography variant="subtitle2" fontWeight={700}>
                                Constraint activity
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

                            <Divider />

                            <Typography variant="subtitle2" fontWeight={700}>
                              Resolved config snapshot
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
                          </Stack>
                        </ScopeSummaryCard>
                      ) : null}

                      <ScopeSummaryCard
                        title="Trace timeline"
                        subtitle="Filter and inspect raw events recorded for this round."
                      >
                        <Stack spacing={2}>
                          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                            <Chip
                              icon={<TimelineIcon />}
                              label="Timeline"
                              color={traceViewMode === "timeline" ? "primary" : "default"}
                              variant={
                                traceViewMode === "timeline" ? "filled" : "outlined"
                              }
                              onClick={() => setTraceViewMode("timeline")}
                            />
                            <Chip
                              icon={<PolicyIcon />}
                              label="Group by constraint"
                              color={traceViewMode === "constraint" ? "primary" : "default"}
                              variant={
                                traceViewMode === "constraint" ? "filled" : "outlined"
                              }
                              onClick={() => setTraceViewMode("constraint")}
                            />
                            <Chip
                              icon={<AutoFixHighIcon />}
                              label="Group by phase"
                              color={traceViewMode === "phase" ? "primary" : "default"}
                              variant={traceViewMode === "phase" ? "filled" : "outlined"}
                              onClick={() => setTraceViewMode("phase")}
                            />
                          </Stack>

                          <Stack
                            direction={{ xs: "column", md: "row" }}
                            spacing={1.5}
                            alignItems={{ xs: "stretch", md: "center" }}
                          >
                            <TextField
                              size="small"
                              label="Search payload"
                              value={traceTextFilter}
                              onChange={(e) => setTraceTextFilter(e.target.value)}
                              sx={{ flex: 1, minWidth: 220 }}
                            />
                            <FormControl size="small" sx={{ minWidth: 220 }}>
                              <InputLabel id="trace-event-type-filter">
                                Event type
                              </InputLabel>
                              <Select
                                labelId="trace-event-type-filter"
                                label="Event type"
                                value={traceEventTypeFilter}
                                onChange={(e) => setTraceEventTypeFilter(e.target.value)}
                              >
                                <MenuItem value="all">All event types</MenuItem>
                                {traceEventTypes.map((eventType) => (
                                  <MenuItem key={eventType} value={eventType}>
                                    {eventType}
                                  </MenuItem>
                                ))}
                              </Select>
                            </FormControl>
                            <FormControl size="small" sx={{ minWidth: 220 }}>
                              <InputLabel id="trace-constraint-filter">
                                Constraint
                              </InputLabel>
                              <Select
                                labelId="trace-constraint-filter"
                                label="Constraint"
                                value={traceConstraintFilter}
                                onChange={(e) =>
                                  setTraceConstraintFilter(e.target.value)
                                }
                              >
                                <MenuItem value="all">All constraints</MenuItem>
                                {traceConstraintIds.map((constraintId) => (
                                  <MenuItem key={constraintId} value={constraintId}>
                                    {constraintId}
                                  </MenuItem>
                                ))}
                              </Select>
                            </FormControl>
                          </Stack>

                          <Typography variant="caption" color="text.secondary">
                            Showing {filteredTraceEvents.length} / {traceEvents.length} events
                          </Typography>

                          {traceViewMode === "timeline" ? (
                            <>
                              <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                                <Chip
                                  icon={<TimelineIcon />}
                                  label="Expand all"
                                  variant="outlined"
                                  onClick={() =>
                                    setExpandedEventIds(
                                      Object.fromEntries(
                                        filteredTraceEvents.map((event) => [event.id, true]),
                                      ),
                                    )
                                  }
                                />
                                <Chip
                                  icon={<TimelineIcon />}
                                  label="Collapse all"
                                  variant="outlined"
                                  onClick={() =>
                                    setExpandedEventIds(
                                      Object.fromEntries(
                                        filteredTraceEvents.map((event) => [event.id, false]),
                                      ),
                                    )
                                  }
                                />
                              </Stack>

                              <Stack spacing={1.5}>
                                {filteredTraceEvents.map((event, index) => (
                                  <Stack
                                    key={event.id}
                                    direction="row"
                                    spacing={1.5}
                                    alignItems="stretch"
                                  >
                                    <Stack
                                      alignItems="center"
                                      sx={{ width: 22, flexShrink: 0 }}
                                    >
                                      <Box
                                        sx={{
                                          width: 12,
                                          height: 12,
                                          borderRadius: "50%",
                                          bgcolor: `${eventTone(event.event_type)}.main`,
                                          mt: 0.5,
                                        }}
                                      />
                                      {index < filteredTraceEvents.length - 1 ? (
                                        <Box
                                          sx={{
                                            width: 2,
                                            flex: 1,
                                            bgcolor: alpha(
                                              theme.palette.divider,
                                              0.9,
                                            ),
                                            minHeight: 24,
                                            mt: 0.5,
                                          }}
                                        />
                                      ) : null}
                                    </Stack>
                                    <Card
                                      variant="outlined"
                                      sx={{ flex: 1, borderRadius: 2.5 }}
                                    >
                                      <CardContent sx={{ py: 1.5 }}>
                                        <Stack spacing={1}>
                                          <Stack
                                            direction={{ xs: "column", md: "row" }}
                                            justifyContent="space-between"
                                            alignItems={{ xs: "flex-start", md: "center" }}
                                            gap={1}
                                          >
                                            <Stack
                                              direction="row"
                                              spacing={1}
                                              alignItems="center"
                                              flexWrap="wrap"
                                              useFlexGap
                                            >
                                              <Chip
                                                size="small"
                                                color={eventChipColor(event.event_type)}
                                                label={event.event_type}
                                              />
                                              {event.constraint_id ? (
                                                <Chip
                                                  size="small"
                                                  variant="outlined"
                                                  color="primary"
                                                  label={event.constraint_id}
                                                />
                                              ) : null}
                                            </Stack>
                                            <Stack
                                              direction="row"
                                              spacing={0.5}
                                              alignItems="center"
                                            >
                                              <Typography
                                                variant="caption"
                                                color="text.secondary"
                                              >
                                                {formatTs(event.created_at)}
                                              </Typography>
                                              <IconButton
                                                size="small"
                                                onClick={() => toggleEventExpanded(event.id)}
                                                sx={{
                                                  transform: expandedEventIds[event.id]
                                                    ? "rotate(180deg)"
                                                    : "rotate(0deg)",
                                                  transition: "transform 0.2s ease",
                                                }}
                                              >
                                                <ExpandMoreIcon fontSize="inherit" />
                                              </IconButton>
                                            </Stack>
                                          </Stack>

                                          <Collapse in={Boolean(expandedEventIds[event.id])}>
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
                                          </Collapse>
                                        </Stack>
                                      </CardContent>
                                    </Card>
                                  </Stack>
                                ))}
                              </Stack>
                            </>
                          ) : traceViewMode === "constraint" ? (
                            <Stack spacing={1.5}>
                              {groupedTraceByConstraint.map(([groupKey, events]) => (
                                <Card key={groupKey} variant="outlined" sx={{ borderRadius: 2.5 }}>
                                  <CardContent>
                                    <Stack spacing={1.25}>
                                      <Stack
                                        direction={{ xs: "column", md: "row" }}
                                        justifyContent="space-between"
                                        alignItems={{ xs: "flex-start", md: "center" }}
                                        gap={1}
                                      >
                                        <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                                          <Chip
                                            color={groupKey === "unscoped" ? "default" : "primary"}
                                            label={
                                              groupKey === "unscoped"
                                                ? "No constraint id"
                                                : groupKey
                                            }
                                          />
                                          <Chip
                                            variant="outlined"
                                            label={`${events.length} events`}
                                          />
                                        </Stack>
                                        <Typography variant="caption" color="text.secondary">
                                          {formatTs(events[0]?.created_at)} →{" "}
                                          {formatTs(events[events.length - 1]?.created_at)}
                                        </Typography>
                                      </Stack>

                                      <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                                        {Array.from(
                                          new Set(events.map((event) => event.event_type)),
                                        ).map((eventType) => (
                                          <Chip
                                            key={`${groupKey}-${eventType}`}
                                            size="small"
                                            color={eventChipColor(eventType)}
                                            variant="outlined"
                                            label={eventType}
                                          />
                                        ))}
                                      </Stack>
                                    </Stack>
                                  </CardContent>
                                </Card>
                              ))}
                            </Stack>
                          ) : (
                            <Stack spacing={1.5}>
                              {groupedTraceByPhase.map(([groupKey, events]) => (
                                <Card key={groupKey} variant="outlined" sx={{ borderRadius: 2.5 }}>
                                  <CardContent>
                                    <Stack spacing={1.25}>
                                      <Stack
                                        direction={{ xs: "column", md: "row" }}
                                        justifyContent="space-between"
                                        alignItems={{ xs: "flex-start", md: "center" }}
                                        gap={1}
                                      >
                                        <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                                          <Chip color="secondary" label={groupKey} />
                                          <Chip
                                            variant="outlined"
                                            label={`${events.length} events`}
                                          />
                                        </Stack>
                                        <Typography variant="caption" color="text.secondary">
                                          {formatTs(events[0]?.created_at)} →{" "}
                                          {formatTs(events[events.length - 1]?.created_at)}
                                        </Typography>
                                      </Stack>

                                      <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                                        {Array.from(
                                          new Set(
                                            events.map((event) => event.constraint_id ?? "unscoped"),
                                          ),
                                        ).map((constraintId) => (
                                          <Chip
                                            key={`${groupKey}-${constraintId}`}
                                            size="small"
                                            variant="outlined"
                                            color={
                                              constraintId === "unscoped"
                                                ? "default"
                                                : "primary"
                                            }
                                            label={constraintId}
                                          />
                                        ))}
                                      </Stack>
                                    </Stack>
                                  </CardContent>
                                </Card>
                              ))}
                            </Stack>
                          )}
                        </Stack>
                      </ScopeSummaryCard>
                    </Stack>
                  )}
                </Box>
              </Stack>
            </>
          )}
        </Stack>
        </Box>
      )}
    </Box>
  );
}
