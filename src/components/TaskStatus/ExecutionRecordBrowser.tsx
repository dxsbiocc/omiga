import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Button,
  Chip,
  CircularProgress,
  Stack,
  Typography,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import { compactLabel } from "../../utils/compactLabel";
import {
  summarizeExecutionInsight,
  type ExecutionInsight,
} from "../Chat/executionInsight";

const ACCENT = "#6366f1";

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
      const nextSelected =
        selectedId && next.records.some((record) => record.id === selectedId)
          ? selectedId
          : next.records[0]?.id ?? null;
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
  onSelect?: (recordId: string) => void;
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
  const insight = useMemo(() => detailInsight(detail), [detail]);

  return (
    <Stack spacing={1}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" spacing={1}>
        <Box sx={{ minWidth: 0 }}>
          <Typography variant="caption" sx={{ display: "block", fontSize: 10, fontWeight: 800 }}>
            ExecutionRecord Browser
          </Typography>
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ display: "block", fontSize: 9.5, lineHeight: 1.35 }}
          >
            只读运行历史；不会删除、归档、promotion 或修改产物。
          </Typography>
        </Box>
        <Button
          size="small"
          variant="outlined"
          onClick={onRefresh}
          disabled={loading}
          sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
        >
          {loading ? "刷新中" : "刷新"}
        </Button>
      </Stack>

      {error ? (
        <Typography variant="caption" color="error" sx={{ fontSize: 10 }}>
          {error}
        </Typography>
      ) : null}

      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
        <Chip size="small" label={`${response?.count ?? 0} records`} sx={{ height: 18, fontSize: 9 }} />
        {response?.lineageSummary ? (
          <>
            <Chip
              size="small"
              label={`${response.lineageSummary.returnedRootRecords} roots`}
              variant="outlined"
              sx={{ height: 18, fontSize: 9 }}
            />
            <Chip
              size="small"
              label={`${response.lineageSummary.returnedRecordsWithParent} children`}
              variant="outlined"
              sx={{ height: 18, fontSize: 9 }}
            />
          </>
        ) : null}
      </Stack>

      {loading && records.length === 0 ? (
        <Stack direction="row" alignItems="center" spacing={0.75}>
          <CircularProgress size={14} />
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
            正在读取运行记录…
          </Typography>
        </Stack>
      ) : records.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10, lineHeight: 1.5 }}>
          当前会话暂无 Operator / Template ExecutionRecord。运行 Template 或 Operator 后会在这里出现。
        </Typography>
      ) : (
        <Stack spacing={0.75}>
          {records.slice(0, 20).map((record) => (
            <ExecutionRecordRow
              key={record.id}
              record={record}
              selected={record.id === selectedId}
              onClick={() => onSelect?.(record.id)}
            />
          ))}
        </Stack>
      )}

      {detailLoading ? (
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
          正在读取记录详情…
        </Typography>
      ) : detail?.found && detail.record ? (
        <Box
          sx={{
            mt: 0.25,
            p: 1,
            borderRadius: 1.25,
            border: `1px solid ${alpha(ACCENT, 0.16)}`,
            bgcolor: alpha(ACCENT, 0.035),
          }}
        >
          <Typography variant="caption" sx={{ display: "block", fontSize: 10, fontWeight: 800 }}>
            记录详情 · {compactLabel(detail.record.id, 30)}
          </Typography>
          {insight ? <ExecutionInsightMiniPanel insight={insight} /> : null}
          {detail.children.length > 0 ? (
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ display: "block", mt: 0.6, fontSize: 9.5 }}
            >
              Children: {detail.children.map((child) => compactLabel(child.id, 16)).join(", ")}
            </Typography>
          ) : null}
        </Box>
      ) : selectedId ? (
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
          记录未找到：{selectedId}
        </Typography>
      ) : null}
    </Stack>
  );
}

function ExecutionRecordRow({
  record,
  selected,
  onClick,
}: {
  record: ExecutionRecordDto;
  selected: boolean;
  onClick?: () => void;
}) {
  const when = formatRecordTime(record.endedAt ?? record.startedAt);
  const unit = record.unitId ?? record.canonicalId ?? record.id;
  return (
    <Box
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") onClick?.();
      }}
      sx={{
        p: 0.75,
        borderRadius: 1.25,
        border: `1px solid ${alpha(selected ? ACCENT : "#64748b", selected ? 0.32 : 0.16)}`,
        bgcolor: selected ? alpha(ACCENT, 0.07) : alpha("#64748b", 0.025),
        cursor: "pointer",
      }}
    >
      <Stack direction="row" spacing={0.6} alignItems="center" flexWrap="wrap" useFlexGap>
        <Chip size="small" label={record.kind} sx={{ height: 16, fontSize: 8.5 }} />
        <Chip
          size="small"
          label={record.status}
          color={record.status === "success" ? "success" : record.status === "failed" ? "error" : "default"}
          variant="outlined"
          sx={{ height: 16, fontSize: 8.5 }}
        />
        {record.parentExecutionId ? (
          <Chip size="small" label="child" variant="outlined" sx={{ height: 16, fontSize: 8.5 }} />
        ) : null}
      </Stack>
      <Typography
        variant="caption"
        sx={{ display: "block", mt: 0.3, fontSize: 10.5, fontWeight: 700 }}
      >
        {compactLabel(unit, 56)}
      </Typography>
      <Typography
        variant="caption"
        color="text.secondary"
        sx={{ display: "block", fontSize: 9, lineHeight: 1.35 }}
      >
        {compactLabel(record.id, 28)}
        {when ? ` · ${when}` : ""}
      </Typography>
    </Box>
  );
}

function ExecutionInsightMiniPanel({ insight }: { insight: ExecutionInsight }) {
  return (
    <Box sx={{ mt: 0.65 }}>
      <Stack direction="row" spacing={0.45} alignItems="center" flexWrap="wrap" useFlexGap>
        <Typography variant="caption" sx={{ fontSize: 9.5, fontWeight: 800 }}>
          {insight.title}
        </Typography>
        {insight.chips.slice(0, 5).map((chip) => (
          <Chip
            key={chip}
            size="small"
            label={chip}
            variant="outlined"
            sx={{ height: 16, fontSize: 8.5, "& .MuiChip-label": { px: 0.55 } }}
          />
        ))}
      </Stack>
      <Stack spacing={0.35} sx={{ mt: 0.55 }}>
        {insight.sections.slice(0, 4).map((section) => (
          <Box key={section.label}>
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ display: "block", fontSize: 8.8, fontWeight: 800 }}
            >
              {section.label}
            </Typography>
            <Stack component="ul" spacing={0.1} sx={{ m: 0, pl: 1.75 }}>
              {section.items.slice(0, 4).map((item) => (
                <Typography
                  key={item}
                  component="li"
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: "list-item", fontSize: 8.8, lineHeight: 1.35 }}
                >
                  {item}
                </Typography>
              ))}
            </Stack>
          </Box>
        ))}
      </Stack>
    </Box>
  );
}

function detailInsight(detail: ExecutionRecordDetailResponse | null): ExecutionInsight | null {
  if (!detail) return null;
  try {
    return summarizeExecutionInsight("execution_record_detail", JSON.stringify(detail));
  } catch {
    return null;
  }
}

function formatRecordTime(value?: string | null): string | null {
  if (!value) return null;
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return value;
  return new Date(timestamp).toLocaleString();
}
