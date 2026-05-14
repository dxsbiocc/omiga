import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Button,
  Collapse,
  CircularProgress,
  Stack,
  Typography,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import { compactLabel } from "../../utils/compactLabel";

const ACCENT = "#6f8a64";
const INK = "#0f172a";
const MUTED = "#64748b";
const BORDER = "#dbe4d8";

export interface ExecutionRecordDto {
  id: string;
  kind: string;
  unitId?: string | null;
  canonicalId?: string | null;
  providerPlugin?: string | null;
  status: string;
  sessionId?: string | null;
  parentExecutionId?: string | null;
  startedAt?: string | null;
  endedAt?: string | null;
  inputHash?: string | null;
  paramHash?: string | null;
  outputSummaryJson?: string | null;
  runtimeJson?: string | null;
  metadataJson?: string | null;
}

interface ExecutionRecordLineageSummary {
  returnedRecords: number;
  returnedRootRecords: number;
  returnedRecordsWithParent: number;
  includedChildRecords: number;
  statusCounts: Record<string, number>;
  kindCounts: Record<string, number>;
  executionModeCounts: Record<string, number>;
}

export interface ExecutionRecordListResponse {
  database: string;
  count: number;
  records: ExecutionRecordDto[];
  lineageSummary: ExecutionRecordLineageSummary;
  note: string;
}

export interface ExecutionRecordDetailResponse {
  found: boolean;
  recordId: string;
  record?: ExecutionRecordDto | null;
  parsed?: unknown;
  children: ExecutionRecordDto[];
  lineage: unknown;
  database: string;
  note: string;
}

interface ExecutionRecordBrowserProps {
  projectRoot: string | null | undefined;
  sessionId?: string | null;
  refreshToken?: number;
}

export function ExecutionRecordBrowser({
  projectRoot,
  sessionId,
  refreshToken,
}: ExecutionRecordBrowserProps) {
  const [response, setResponse] = useState<ExecutionRecordListResponse | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<ExecutionRecordDetailResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadRecords = async () => {
    if (!projectRoot) {
      setResponse(null);
      setDetail(null);
      setSelectedId(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<ExecutionRecordListResponse>("list_execution_records", {
        projectRoot,
        limit: 50,
        sessionId: sessionId ?? undefined,
      });
      setResponse(next);
      const selectableRecords = rootExecutionRecords(next.records);
      const nextSelected =
        selectedId && selectableRecords.some((record) => record.id === selectedId)
          ? selectedId
          : selectableRecords[0]?.id ?? null;
      setSelectedId(nextSelected);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setResponse(null);
      setSelectedId(null);
      setDetail(null);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadRecords();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectRoot, sessionId, refreshToken]);

  useEffect(() => {
    if (!projectRoot || !selectedId) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    setDetailLoading(true);
    setError(null);
    invoke<ExecutionRecordDetailResponse>("read_execution_record", {
      projectRoot,
      recordId: selectedId,
      includeChildren: true,
    })
      .then((next) => {
        if (!cancelled) setDetail(next);
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
          setDetail(null);
        }
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectRoot, selectedId]);

  return (
    <ExecutionRecordBrowserView
      response={response}
      selectedId={selectedId}
      detail={detail}
      loading={loading}
      detailLoading={detailLoading}
      error={error}
      onRefresh={() => void loadRecords()}
      onSelect={setSelectedId}
    />
  );
}

export interface ExecutionRecordBrowserViewProps {
  response: ExecutionRecordListResponse | null;
  selectedId: string | null;
  detail: ExecutionRecordDetailResponse | null;
  loading?: boolean;
  detailLoading?: boolean;
  error?: string | null;
  onRefresh?: () => void;
  onSelect?: (recordId: string | null) => void;
}

export function ExecutionRecordBrowserView({
  response,
  selectedId,
  detail,
  loading = false,
  detailLoading = false,
  error,
  onRefresh,
  onSelect,
}: ExecutionRecordBrowserViewProps) {
  const records = response?.records ?? [];
  const visibleRecords = rootExecutionRecords(records);
  const childCounts = useMemo(() => executionChildCounts(records), [records]);

  return (
    <Stack spacing={1.05}>
      <Box
        sx={{
          p: 1,
          borderRadius: 2.25,
          border: `1px solid ${alpha(ACCENT, 0.16)}`,
          background: `linear-gradient(135deg, ${alpha("#ffffff", 0.98)} 0%, ${alpha(
            ACCENT,
            0.06,
          )} 100%)`,
          boxShadow: `0 8px 24px ${alpha("#0f172a", 0.035)}`,
        }}
      >
        <Stack direction="row" alignItems="center" justifyContent="space-between" spacing={1}>
          <Box sx={{ minWidth: 0 }}>
            <Typography
              variant="caption"
              sx={{ display: "block", color: INK, fontSize: 12, fontWeight: 950, lineHeight: 1.2 }}
            >
              运行记录
            </Typography>
            <Typography
              variant="caption"
              sx={{ display: "block", color: MUTED, fontSize: 9.5, lineHeight: 1.4, mt: 0.25 }}
            >
              查看最近任务的结果和状态；后台 Operator 只作为排错信息保留。
            </Typography>
          </Box>
          <Button
            size="small"
            variant="outlined"
            onClick={onRefresh}
            disabled={loading}
            sx={{
              minWidth: 0,
              px: 1.15,
              py: 0.25,
              borderRadius: 999,
              borderColor: alpha(ACCENT, 0.34),
              color: ACCENT,
              fontSize: 10,
              fontWeight: 900,
              whiteSpace: "nowrap",
              "&:hover": {
                borderColor: alpha(ACCENT, 0.5),
                bgcolor: alpha(ACCENT, 0.06),
              },
            }}
          >
            {loading ? "刷新中" : "刷新"}
          </Button>
        </Stack>

        <Stack direction="row" spacing={0.55} flexWrap="wrap" useFlexGap sx={{ mt: 0.85 }}>
          <MetricPill label={`${visibleRecords.length} 次任务`} emphasized />
          {response?.lineageSummary ? (
            <>
              <MetricPill label={`${response.lineageSummary.returnedRecordsWithParent} 个后台步骤`} />
              <MetricPill label={`${response.count} 条历史记录`} />
            </>
          ) : null}
        </Stack>
      </Box>

      {error ? (
        <Typography variant="caption" color="error" sx={{ fontSize: 10 }}>
          {error}
        </Typography>
      ) : null}

      {loading && visibleRecords.length === 0 ? (
        <Stack direction="row" alignItems="center" spacing={0.75}>
          <CircularProgress size={14} />
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
            正在读取运行记录…
          </Typography>
        </Stack>
      ) : visibleRecords.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10, lineHeight: 1.5 }}>
          当前会话暂无 Operator / Template ExecutionRecord。运行 Template 或 Operator 后会在这里出现。
        </Typography>
      ) : (
        <Stack spacing={0.75}>
          {visibleRecords.slice(0, 20).map((record) => {
            const selected = record.id === selectedId;
            return (
              <ExecutionRecordItem
                key={record.id}
                record={record}
                childCount={childCounts.get(record.id) ?? 0}
                open={selected}
                selectedId={selectedId}
                detail={detail}
                detailLoading={detailLoading}
                onToggle={() => onSelect?.(selected ? null : record.id)}
              />
            );
          })}
        </Stack>
      )}
    </Stack>
  );
}

function MetricPill({
  label,
  emphasized,
}: {
  label: string;
  emphasized?: boolean;
}) {
  return (
    <Box
      component="span"
      sx={{
        px: 0.9,
        py: 0.25,
        borderRadius: 999,
        fontSize: 9,
        fontWeight: 800,
        lineHeight: 1.4,
        bgcolor: emphasized ? alpha(ACCENT, 0.11) : alpha("#ffffff", 0.72),
        color: emphasized ? "#4f6f48" : MUTED,
        border: `1px solid ${emphasized ? alpha(ACCENT, 0.2) : alpha("#94a3b8", 0.18)}`,
      }}
    >
      {label}
    </Box>
  );
}

function ExecutionRecordItem({
  record,
  childCount,
  open,
  selectedId,
  detail,
  detailLoading,
  onToggle,
}: {
  record: ExecutionRecordDto;
  childCount: number;
  open: boolean;
  selectedId: string | null;
  detail: ExecutionRecordDetailResponse | null;
  detailLoading: boolean;
  onToggle: () => void;
}) {
  const when = formatRecordTime(record.endedAt ?? record.startedAt);
  const unit = record.unitId ?? record.canonicalId ?? record.id;
  const tone = statusTone(record.status);
  const childLabel = `${childCount} 个后台步骤`;

  return (
    <Box
      sx={{
        position: "relative",
        overflow: "hidden",
        borderRadius: 2.25,
        border: `1px solid ${open ? alpha(ACCENT, 0.42) : alpha("#94a3b8", 0.18)}`,
        background: open
          ? `linear-gradient(180deg, ${alpha("#ffffff", 0.98)} 0%, ${alpha(ACCENT, 0.045)} 100%)`
          : alpha("#ffffff", 0.92),
        boxShadow: open
          ? `0 12px 28px ${alpha("#0f172a", 0.075)}`
          : `0 1px 2px ${alpha("#0f172a", 0.035)}`,
        transition: "border-color 160ms ease, box-shadow 160ms ease, background-color 160ms ease",
        "&:hover": {
          borderColor: open ? alpha(ACCENT, 0.48) : alpha(ACCENT, 0.28),
          boxShadow: `0 10px 24px ${alpha("#0f172a", 0.06)}`,
        },
      }}
    >
      {open ? (
        <Box
          sx={{
            position: "absolute",
            inset: "0 auto 0 0",
            width: 3,
            bgcolor: tone.color,
            opacity: 0.82,
          }}
        />
      ) : null}
      <Box
        role="button"
        tabIndex={0}
        aria-expanded={open}
        onClick={onToggle}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onToggle();
          }
        }}
        sx={{
          p: 1.05,
          pl: open ? 1.25 : 1.05,
          display: "grid",
          gridTemplateColumns: "minmax(0, 1fr) auto",
          gap: 1,
          alignItems: "center",
          cursor: "pointer",
          outline: "none",
          "&:focus-visible": {
            boxShadow: `inset 0 0 0 2px ${alpha(ACCENT, 0.36)}`,
          },
        }}
      >
        <Box sx={{ minWidth: 0 }}>
          <Stack direction="row" alignItems="center" spacing={0.65} sx={{ minWidth: 0 }}>
            <Box
              sx={{
                width: 7,
                height: 7,
                flexShrink: 0,
                borderRadius: 999,
                bgcolor: tone.color,
                boxShadow: `0 0 0 3px ${tone.tint}`,
              }}
            />
            <Typography
              variant="caption"
              sx={{
                minWidth: 0,
                display: "block",
                color: INK,
                fontSize: 12,
                fontWeight: 950,
                lineHeight: 1.25,
                letterSpacing: 0.05,
              }}
            >
              {compactLabel(unit, 58)}
            </Typography>
          </Stack>
          <Typography
            variant="caption"
            sx={{
              display: "block",
              mt: 0.35,
              color: MUTED,
              fontSize: 9.5,
              lineHeight: 1.35,
              pl: 1.65,
            }}
          >
            <Box component="span" sx={{ fontWeight: 900, color: alpha(INK, 0.62) }}>
              {recordKindLabel(record.kind)}
            </Box>
            {when ? ` · ${when}` : ""}
            {childCount > 0 ? ` · ${childLabel}` : ""}
          </Typography>
        </Box>
        <Stack alignItems="flex-end" spacing={0.35} sx={{ flexShrink: 0 }}>
          <StatusBadge label={statusLabel(record.status)} color={tone.color} tint={tone.tint} />
          <Typography
            variant="caption"
            sx={{
              color: open ? "#4f6f48" : MUTED,
              fontSize: 9.5,
              fontWeight: 850,
              lineHeight: 1,
            }}
          >
            <Box
              component="span"
              sx={{
                display: "inline-block",
                mr: 0.35,
                transform: open ? "rotate(180deg)" : "rotate(0deg)",
                transition: "transform 160ms ease",
              }}
            >
              ▾
            </Box>
            {open ? "收起" : "详情"}
          </Typography>
        </Stack>
      </Box>
      <ExecutionRecordDetailCollapse
        open={open}
        selectedId={selectedId}
        detail={detail}
        loading={detailLoading}
      />
    </Box>
  );
}

function ExecutionRecordDetailCollapse({
  open,
  selectedId,
  detail,
  loading,
}: {
  open: boolean;
  selectedId: string | null;
  detail: ExecutionRecordDetailResponse | null;
  loading: boolean;
}) {
  const summary = detail?.found && detail.record ? userExecutionSummary(detail) : null;

  return (
    <Collapse in={open} timeout={180} unmountOnExit>
      {loading ? (
        <Box sx={{ px: 1.2, pb: 1 }}>
          <Typography variant="caption" sx={{ color: MUTED, fontSize: 10 }}>
            正在读取记录详情…
          </Typography>
        </Box>
      ) : detail?.found && detail.record ? (
        <Box
          sx={{
            mx: 1.15,
            pb: 1,
            pt: 0.75,
            borderTop: `1px solid ${alpha(BORDER, 0.9)}`,
          }}
        >
          {summary ? <UserExecutionSummaryPanel summary={summary} /> : null}
        </Box>
      ) : selectedId ? (
        <Box sx={{ px: 1.25, pb: 1 }}>
          <Typography variant="caption" sx={{ color: MUTED, fontSize: 10 }}>
            记录未找到：{selectedId}
          </Typography>
        </Box>
      ) : null}
    </Collapse>
  );
}

function StatusBadge({
  label,
  color,
  tint,
}: {
  label: string;
  color: string;
  tint: string;
}) {
  return (
    <Box
      component="span"
      sx={{
        px: 0.7,
        py: 0.22,
        borderRadius: 999,
        fontSize: 8.6,
        fontWeight: 900,
        lineHeight: 1.35,
        color,
        bgcolor: tint,
        border: `1px solid ${alpha(color, 0.2)}`,
      }}
    >
      {label}
    </Box>
  );
}

interface UserExecutionSummary {
  status: string;
  finishedAt: string;
  outputs: string;
  decisions: string;
}

function UserExecutionSummaryPanel({ summary }: { summary: UserExecutionSummary }) {
  return (
    <Box
      sx={{
        mt: 0.65,
        p: 0.8,
        borderRadius: 1.5,
        border: `1px solid ${alpha("#94a3b8", 0.13)}`,
        bgcolor: alpha("#f8fafc", 0.72),
      }}
    >
      <Stack spacing={0.45}>
        <SummaryLine label="结果" value={summary.status} emphasized />
        <SummaryLine label="完成时间" value={summary.finishedAt} />
        <SummaryLine label="产物" value={summary.outputs} />
        <SummaryLine label="参数" value={summary.decisions} />
      </Stack>
    </Box>
  );
}

function SummaryLine({
  label,
  value,
  emphasized,
}: {
  label: string;
  value: string;
  emphasized?: boolean;
}) {
  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: "56px minmax(0, 1fr)",
        gap: 0.8,
        alignItems: "baseline",
      }}
    >
      <Typography
        variant="caption"
        sx={{
          color: alpha(INK, 0.5),
          display: "block",
          fontSize: 8.8,
          fontWeight: 950,
        }}
      >
        {label}
      </Typography>
      <Typography
        variant="caption"
        sx={{
          color: emphasized ? "#2f7d4f" : MUTED,
          display: "block",
          fontSize: 9.5,
          fontWeight: emphasized ? 900 : 700,
          lineHeight: 1.35,
        }}
      >
        {value}
      </Typography>
    </Box>
  );
}

function rootExecutionRecords(records: ExecutionRecordDto[]): ExecutionRecordDto[] {
  const roots = records.filter((record) => !record.parentExecutionId);
  return roots.length > 0 ? roots : records;
}

function executionChildCounts(records: ExecutionRecordDto[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const record of records) {
    if (!record.parentExecutionId) continue;
    counts.set(record.parentExecutionId, (counts.get(record.parentExecutionId) ?? 0) + 1);
  }
  return counts;
}

function userExecutionSummary(detail: ExecutionRecordDetailResponse): UserExecutionSummary {
  const record = detail.record;
  const parsed = asObject(detail.parsed);
  const metadata = asObject(parsed?.metadata);
  const preflight = asObject(metadata?.preflight);
  const outputSummary = asObject(parsed?.outputSummary);
  const outputs = asObject(outputSummary?.outputs);
  const finishedAt = formatRecordTime(record?.endedAt ?? record?.startedAt) ?? "未记录";
  const answeredCount = asArray(preflight?.answeredParams).length;
  const paramSourceCount = Object.keys(asObject(metadata?.paramSources) ?? {}).length;

  return {
    status: statusLabel(record?.status ?? "unknown"),
    finishedAt,
    outputs: summarizeOutputs(outputs),
    decisions:
      answeredCount > 0
        ? `用户确认 ${answeredCount} 项`
        : paramSourceCount > 0
          ? `记录了 ${paramSourceCount} 项参数来源`
          : "未记录参数选择",
  };
}

function summarizeOutputs(outputs: Record<string, unknown> | null): string {
  if (!outputs || Object.keys(outputs).length === 0) return "未记录可见产物";
  return Object.entries(outputs)
    .slice(0, 3)
    .map(([kind, value]) => {
      const count = Array.isArray(value) ? value.length : 1;
      return `${outputKindLabel(kind)} × ${count}`;
    })
    .join("、");
}

function asObject(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function statusLabel(status: string): string {
  const normalized = status.trim().toLowerCase();
  if (normalized === "success" || normalized === "succeeded") return "已成功";
  if (normalized === "failed" || normalized === "error") return "失败";
  if (normalized === "running") return "运行中";
  if (normalized === "pending") return "等待中";
  return status || "未知";
}

function recordKindLabel(kind: string): string {
  const normalized = kind.trim().toLowerCase();
  if (normalized === "template") return "绘图模板";
  if (normalized === "operator") return "后台步骤";
  return kind;
}

function outputKindLabel(kind: string): string {
  const normalized = kind.trim().toLowerCase();
  if (normalized === "table") return "表格";
  if (normalized === "figure" || normalized === "image" || normalized === "plot") return "图形";
  if (normalized === "report") return "报告";
  if (normalized === "html") return "网页";
  return kind;
}

function formatRecordTime(value?: string | null): string | null {
  if (!value) return null;
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return value;
  return new Date(timestamp).toLocaleString();
}

function statusTone(status: string): { color: string; tint: string } {
  const normalized = status.trim().toLowerCase();
  if (normalized === "success" || normalized === "succeeded") {
    return { color: "#2f7d4f", tint: alpha("#2f7d4f", 0.09) };
  }
  if (normalized === "failed" || normalized === "error") {
    return { color: "#b4532a", tint: alpha("#b4532a", 0.1) };
  }
  if (normalized === "running" || normalized === "pending") {
    return { color: "#7c6f2d", tint: alpha("#7c6f2d", 0.1) };
  }
  return { color: "#64748b", tint: alpha("#64748b", 0.08) };
}
