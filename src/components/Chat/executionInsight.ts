export interface ExecutionInsightSection {
  label: string;
  items: string[];
}

export interface ExecutionInsight {
  title: string;
  chips: string[];
  sections: ExecutionInsightSection[];
}

type JsonObject = Record<string, unknown>;

function isObject(value: unknown): value is JsonObject {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

function asString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function parseJsonObject(raw: string | undefined): JsonObject | null {
  if (!raw?.trim()) return null;
  try {
    const parsed = JSON.parse(raw) as unknown;
    return isObject(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function parseNestedJsonObject(value: unknown): JsonObject | null {
  if (isObject(value)) return value;
  if (typeof value !== "string" || !value.trim()) return null;
  try {
    const parsed = JSON.parse(value) as unknown;
    return isObject(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function compact(value: unknown, max = 72): string | null {
  if (value == null) return null;
  const raw =
    typeof value === "string" ? value : JSON.stringify(value) ?? String(value);
  const trimmed = raw.trim();
  if (!trimmed) return null;
  return trimmed.length > max ? `${trimmed.slice(0, max)}…` : trimmed;
}

function entriesFromCountObject(value: unknown, max = 4): string[] {
  if (!isObject(value)) return [];
  return Object.entries(value)
    .filter(([, count]) => typeof count === "number")
    .slice(0, max)
    .map(([key, count]) => `${key}: ${count}`);
}

function countByValue(value: unknown): string[] {
  if (!isObject(value)) return [];
  const counts = new Map<string, number>();
  for (const source of Object.values(value)) {
    const key = asString(source);
    if (!key) continue;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return Array.from(counts.entries()).map(([key, count]) => `${key}: ${count}`);
}

function selectedParamItems(value: unknown): string[] {
  if (!isObject(value)) return [];
  return Object.entries(value)
    .slice(0, 5)
    .map(([key, raw]) => {
      const rendered = compact(raw, 56);
      return rendered ? `${key}=${rendered}` : key;
    });
}

function preflightItems(preflight: unknown, selectedParams?: unknown): string[] {
  if (!isObject(preflight)) return [];
  const items: string[] = [];
  const answered = asArray(preflight.answeredParams);
  if (answered.length > 0) {
    items.push(
      `${answered.length} answered question${answered.length === 1 ? "" : "s"}`,
    );
  }
  const sources = countByValue(preflight.paramsBySource);
  if (sources.length > 0) {
    items.push(`sources: ${sources.join(", ")}`);
  }
  const selected = selectedParamItems(selectedParams);
  if (selected.length > 0) {
    items.push(`selected: ${selected.join(", ")}`);
  }
  return items;
}

function operatorIdentity(operator: unknown): string | null {
  if (!isObject(operator)) return null;
  const alias = asString(operator.alias);
  const id = asString(operator.id);
  const version = asString(operator.version);
  if (alias && id && alias !== id) {
    return `${alias} → ${id}${version ? `@${version}` : ""}`;
  }
  if (id) return `${id}${version ? `@${version}` : ""}`;
  return alias;
}

function operatorRunInsight(
  toolName: string,
  parsed: JsonObject,
): ExecutionInsight | null {
  const status = asString(parsed.status);
  const runId = asString(parsed.runId);
  const operator = operatorIdentity(parsed.operator);
  const runContext = isObject(parsed.runContext) ? parsed.runContext : null;
  const isOperatorTool = toolName.startsWith("operator__");
  const isTemplateTool =
    toolName === "template_execute" || runContext?.kind === "template";
  if (!isOperatorTool && !isTemplateTool && !operator) return null;

  const chips = [
    isTemplateTool ? "Template run" : "Operator run",
    status,
    runContext?.kind ? `kind:${runContext.kind}` : null,
    parsed.cache && isObject(parsed.cache) && parsed.cache.hit === true
      ? "cache hit"
      : null,
  ].filter(Boolean) as string[];

  const sections: ExecutionInsightSection[] = [];
  const runItems = [
    runId ? `run: ${runId}` : null,
    operator ? `unit: ${operator}` : null,
    asString(parsed.runDir) ? `runDir: ${asString(parsed.runDir)}` : null,
    asString(runContext?.parentExecutionId)
      ? `parent: ${asString(runContext?.parentExecutionId)}`
      : null,
  ].filter(Boolean) as string[];
  if (runItems.length > 0) sections.push({ label: "Run", items: runItems });

  const sourceItems = countByValue(parsed.paramSources);
  const selected = selectedParamItems(parsed.selectedParams);
  const preflight = preflightItems(parsed.preflight, parsed.selectedParams);
  if (sourceItems.length > 0 || selected.length > 0 || preflight.length > 0) {
    sections.push({
      label: "Decisions",
      items: [
        ...sourceItems.map((item) => `paramSources ${item}`),
        ...preflight,
        ...(selected.length > 0 && preflight.length === 0
          ? [`selected: ${selected.join(", ")}`]
          : []),
      ],
    });
  }

  const outputs = isObject(parsed.outputs)
    ? Object.entries(parsed.outputs).map(([key, value]) => {
        const count = Array.isArray(value) ? value.length : 1;
        return `${key}: ${count}`;
      })
    : [];
  if (outputs.length > 0) {
    sections.push({ label: "Outputs", items: outputs.slice(0, 6) });
  }

  return sections.length > 0
    ? {
        title: isTemplateTool ? "Template execution" : "Operator execution",
        chips,
        sections,
      }
    : null;
}

function recordMetadata(record: unknown): JsonObject | null {
  if (!isObject(record)) return null;
  return (
    parseNestedJsonObject(record.metadataJson) ??
    parseNestedJsonObject(record.metadata_json)
  );
}

function executionRecordInsight(parsed: JsonObject): ExecutionInsight | null {
  if (!Array.isArray(parsed.records) && !parsed.lineageSummary) return null;
  const records = asArray(parsed.records);
  const summary = isObject(parsed.lineageSummary) ? parsed.lineageSummary : null;
  const chips = [
    "Execution records",
    `${records.length} returned`,
    asNumber(summary?.returnedRecordsWithParent)
      ? `${summary?.returnedRecordsWithParent} child`
      : null,
    asNumber(summary?.includedChildRecords)
      ? `${summary?.includedChildRecords} included child`
      : null,
  ].filter(Boolean) as string[];

  const sections: ExecutionInsightSection[] = [];
  if (summary) {
    const lineageItems = [
      `roots: ${asNumber(summary.returnedRootRecords) ?? 0}`,
      `children: ${asNumber(summary.returnedRecordsWithParent) ?? 0}`,
      ...entriesFromCountObject(summary.executionModeCounts),
    ];
    sections.push({ label: "Lineage", items: lineageItems });
  }

  const decisionItems = records
    .flatMap((record) => {
      const metadata = recordMetadata(record);
      if (!metadata) return [];
      const preflight = preflightItems(
        metadata.preflight,
        metadata.selectedParams,
      );
      const sourceItems = countByValue(metadata.paramSources);
      const id = isObject(record)
        ? asString(record.unitId) ?? asString(record.id) ?? "record"
        : "record";
      return [
        ...sourceItems.map((item) => `${id} paramSources ${item}`),
        ...preflight.map((item) => `${id} ${item}`),
      ];
    })
    .slice(0, 6);
  if (decisionItems.length > 0) {
    sections.push({ label: "Recorded decisions", items: decisionItems });
  }

  return sections.length > 0
    ? { title: "Execution record insight", chips, sections }
    : null;
}

function executionRecordDetailInsight(parsed: JsonObject): ExecutionInsight | null {
  if (parsed.found == null || !("record" in parsed) || !("lineage" in parsed)) {
    return null;
  }
  const record = isObject(parsed.record) ? parsed.record : null;
  const parsedRecord = isObject(parsed.parsed) ? parsed.parsed : null;
  const metadata = isObject(parsedRecord?.metadata) ? parsedRecord.metadata : null;
  const outputSummary = isObject(parsedRecord?.outputSummary)
    ? parsedRecord.outputSummary
    : null;
  const lineage = isObject(parsed.lineage) ? parsed.lineage : null;
  const children = asArray(parsed.children);
  const found = parsed.found === true;

  const chips = [
    "Execution detail",
    found ? "found" : "missing",
    asString(record?.kind),
    asString(record?.status),
    children.length > 0 ? `${children.length} child` : null,
  ].filter(Boolean) as string[];

  const sections: ExecutionInsightSection[] = [];
  const runItems = [
    asString(parsed.recordId) ? `record: ${asString(parsed.recordId)}` : null,
    asString(record?.unitId) ? `unit: ${asString(record?.unitId)}` : null,
    asString(record?.canonicalId)
      ? `canonical: ${asString(record?.canonicalId)}`
      : null,
    asString(lineage?.parentExecutionId)
      ? `parent: ${asString(lineage?.parentExecutionId)}`
      : null,
    asNumber(lineage?.childCount) ? `children: ${lineage?.childCount}` : null,
  ].filter(Boolean) as string[];
  if (runItems.length > 0) sections.push({ label: "Run", items: runItems });

  const sourceItems = countByValue(metadata?.paramSources);
  const preflight = preflightItems(metadata?.preflight, metadata?.selectedParams);
  const selected = selectedParamItems(metadata?.selectedParams);
  if (sourceItems.length > 0 || preflight.length > 0 || selected.length > 0) {
    sections.push({
      label: "Decisions",
      items: [
        ...sourceItems.map((item) => `paramSources ${item}`),
        ...preflight,
        ...(selected.length > 0 && preflight.length === 0
          ? [`selected: ${selected.join(", ")}`]
          : []),
      ],
    });
  }

  const outputs = isObject(outputSummary?.outputs)
    ? Object.entries(outputSummary.outputs).map(([key, value]) => {
        const count = Array.isArray(value) ? value.length : 1;
        return `${key}: ${count}`;
      })
    : [];
  if (outputs.length > 0) {
    sections.push({ label: "Outputs", items: outputs.slice(0, 6) });
  }

  return sections.length > 0
    ? {
        title: found ? "Execution record detail" : "Execution record missing",
        chips,
        sections,
      }
    : null;
}

function lineageReportInsight(parsed: JsonObject): ExecutionInsight | null {
  if (parsed.scannedRecordCount == null || parsed.rootRecordCount == null) return null;
  const chips = [
    "Lineage report",
    `${asNumber(parsed.scannedRecordCount) ?? 0} scanned`,
    `${asNumber(parsed.childRecordCount) ?? 0} child`,
    `${asNumber(parsed.fallbackRunCount) ?? 0} fallback`,
  ];
  return {
    title: "Execution lineage",
    chips,
    sections: [
      {
        label: "Buckets",
        items: [
          ...entriesFromCountObject(parsed.statusCounts),
          ...entriesFromCountObject(parsed.kindCounts),
          ...entriesFromCountObject(parsed.executionModeCounts),
        ],
      },
    ],
  };
}

function archiveAdvisorInsight(parsed: JsonObject): ExecutionInsight | null {
  if (!isObject(parsed.summary) || !Array.isArray(parsed.recommendations)) {
    return null;
  }
  const summary = parsed.summary;
  const recommendations = asArray(parsed.recommendations).filter(isObject);
  const chips = [
    "Archive advisor",
    `${asNumber(summary.recommendationCount) ?? recommendations.length} recommendations`,
    `${asNumber(summary.highPriorityCount) ?? 0} high`,
    `${asNumber(summary.mediumPriorityCount) ?? 0} medium`,
  ];
  const top = recommendations.slice(0, 4).map((recommendation) => {
    const action = asString(recommendation.action) ?? "action";
    const priority = asString(recommendation.priority) ?? "priority";
    const unit =
      asString(recommendation.unitId) ??
      asString(recommendation.recordId) ??
      "record";
    const reason = compact(recommendation.reason, 96);
    return `${priority} ${action}: ${unit}${reason ? ` — ${reason}` : ""}`;
  });
  return {
    title: "Archive recommendations",
    chips,
    sections: [
      {
        label: "Actions",
        items:
          top.length > 0 ? top : entriesFromCountObject(summary.actionCounts),
      },
    ],
  };
}

function archiveSuggestionWriteInsight(
  toolName: string,
  parsed: JsonObject,
): ExecutionInsight | null {
  if (toolName !== "execution_archive_suggestion_write") return null;
  const reportPath = asString(parsed.reportPath);
  const jsonPath = asString(parsed.jsonPath);
  if (!reportPath && !jsonPath) return null;

  const recommendationCount = asNumber(parsed.recommendationCount) ?? 0;
  const chips = [
    "Archive report",
    `${recommendationCount} recommendations`,
    `${asNumber(parsed.highPriorityCount) ?? 0} high`,
    `${asNumber(parsed.mediumPriorityCount) ?? 0} medium`,
  ];

  const files = [
    reportPath ? `markdown: ${reportPath}` : null,
    jsonPath ? `json: ${jsonPath}` : null,
  ].filter(Boolean) as string[];
  const notes = [
    compact(parsed.markdownSummary, 128),
    compact(parsed.safetyNote, 128),
  ].filter(Boolean) as string[];

  return {
    title: "Archive suggestion report",
    chips,
    sections: [
      ...(files.length > 0 ? [{ label: "Files", items: files }] : []),
      ...(notes.length > 0 ? [{ label: "Notes", items: notes }] : []),
    ],
  };
}

function environmentProfileInsight(
  toolName: string,
  parsed: JsonObject,
): ExecutionInsight | null {
  if (
    toolName !== "environment_profile_check" &&
    toolName !== "environment_profile_prepare_plan"
  ) {
    return null;
  }
  const resolution = isObject(parsed.resolution) ? parsed.resolution : null;
  if (!resolution) return null;
  const check = isObject(parsed.check) ? parsed.check : null;
  const plan = isObject(parsed.plan) ? parsed.plan : null;

  const chips = [
    toolName === "environment_profile_prepare_plan"
      ? "Environment plan"
      : "Environment check",
    asString(resolution.status) ?? "unknown",
    check ? `check:${asString(check.status) ?? "unknown"}` : null,
    plan ? `plan:${asString(plan.status) ?? "unknown"}` : null,
  ].filter(Boolean) as string[];

  const resolutionItems = [
    asString(parsed.envRef) ? `envRef: ${asString(parsed.envRef)}` : null,
    asString(resolution.canonicalId)
      ? `canonical: ${asString(resolution.canonicalId)}`
      : null,
    ...asArray(resolution.diagnostics)
      .map((item) => compact(item, 96))
      .filter(Boolean),
  ].filter(Boolean) as string[];

  const sections: ExecutionInsightSection[] = [];
  if (resolutionItems.length > 0) {
    sections.push({ label: "Resolution", items: resolutionItems });
  }

  if (check) {
    const checkItems = [
      asString(check.status) ? `status: ${asString(check.status)}` : null,
      Array.isArray(check.command)
        ? `command: ${check.command.join(" ")}`
        : null,
    ].filter(Boolean) as string[];
    if (checkItems.length > 0) {
      sections.push({ label: "Check", items: checkItems });
    }
  }

  if (plan) {
    const actionItems = asArray(plan.actions)
      .filter(isObject)
      .slice(0, 4)
      .map((action) => {
        const kind = asString(action.kind) ?? "action";
        const status = asString(action.status) ?? "manual";
        const description = compact(action.description, 96);
        return `${kind} (${status})${description ? ` — ${description}` : ""}`;
      });
    const fileItems = [
      asString(parsed.planPath) ? `markdown: ${asString(parsed.planPath)}` : null,
      asString(parsed.jsonPath) ? `json: ${asString(parsed.jsonPath)}` : null,
    ].filter(Boolean) as string[];
    sections.push({
      label: "Preparation",
      items: [...fileItems, ...actionItems].slice(0, 6),
    });
  }

  return sections.length > 0
    ? {
        title:
          toolName === "environment_profile_prepare_plan"
            ? "Environment preparation plan"
            : "Environment profile check",
        chips,
        sections,
      }
    : null;
}

function selfEvolutionReportInsight(
  toolName: string,
  parsed: JsonObject,
): ExecutionInsight | null {
  if (toolName !== "learning_self_evolution_report") return null;
  const summary = isObject(parsed.summary) ? parsed.summary : null;
  const candidates = asArray(parsed.candidates).filter(isObject);
  if (!summary && candidates.length === 0) return null;

  const chips = [
    "Self-evolution",
    `${asNumber(summary?.candidateCount) ?? candidates.length} candidates`,
    `${asNumber(summary?.templateCandidateCount) ?? 0} template`,
    `${asNumber(summary?.preferenceCandidateCount) ?? 0} preference`,
  ];
  const files = [
    asString(parsed.reportPath) ? `markdown: ${asString(parsed.reportPath)}` : null,
    asString(parsed.jsonPath) ? `json: ${asString(parsed.jsonPath)}` : null,
  ].filter(Boolean) as string[];
  const topCandidates = candidates.slice(0, 4).map((candidate) => {
    const priority = asString(candidate.priority) ?? "priority";
    const kind = asString(candidate.kind) ?? "candidate";
    const title =
      asString(candidate.title) ?? asString(candidate.id) ?? "candidate";
    const rationale = compact(candidate.rationale, 96);
    return `${priority} ${kind}: ${title}${rationale ? ` — ${rationale}` : ""}`;
  });

  const sections: ExecutionInsightSection[] = [];
  if (files.length > 0) sections.push({ label: "Files", items: files });
  sections.push({
    label: "Candidates",
    items: topCandidates.length > 0 ? topCandidates : ["No candidates"],
  });
  const safety = compact(parsed.safetyNote, 128);
  if (safety) sections.push({ label: "Safety", items: [safety] });

  return {
    title: "Learning self-evolution report",
    chips,
    sections,
  };
}

function selfEvolutionDraftInsight(
  toolName: string,
  parsed: JsonObject,
): ExecutionInsight | null {
  if (toolName !== "learning_self_evolution_draft_write") return null;
  const drafts = asArray(parsed.drafts).filter(isObject);
  const batchDir = asString(parsed.batchDir);
  if (!batchDir && drafts.length === 0) return null;

  const chips = [
    "Draft scaffolds",
    `${asNumber(parsed.draftCount) ?? drafts.length} drafts`,
  ];
  const files = [
    batchDir ? `batch: ${batchDir}` : null,
    asString(parsed.indexPath) ? `index: ${asString(parsed.indexPath)}` : null,
  ].filter(Boolean) as string[];
  const topDrafts = drafts.slice(0, 4).map((draft) => {
    const kind = asString(draft.kind) ?? "draft";
    const title =
      asString(draft.title) ?? asString(draft.candidateId) ?? "candidate";
    const dir = asString(draft.draftDir);
    return `${kind}: ${title}${dir ? ` — ${dir}` : ""}`;
  });
  const sections: ExecutionInsightSection[] = [];
  if (files.length > 0) sections.push({ label: "Files", items: files });
  sections.push({
    label: "Drafts",
    items: topDrafts.length > 0 ? topDrafts : ["No drafts written"],
  });
  const safety = compact(parsed.safetyNote, 128);
  if (safety) sections.push({ label: "Safety", items: [safety] });

  return {
    title: "Learning self-evolution drafts",
    chips,
    sections,
  };
}

export function summarizeExecutionInsight(
  toolName: string,
  output: string | undefined,
): ExecutionInsight | null {
  const parsed = parseJsonObject(output);
  if (!parsed) return null;
  return (
    selfEvolutionDraftInsight(toolName, parsed) ??
    selfEvolutionReportInsight(toolName, parsed) ??
    environmentProfileInsight(toolName, parsed) ??
    archiveSuggestionWriteInsight(toolName, parsed) ??
    archiveAdvisorInsight(parsed) ??
    lineageReportInsight(parsed) ??
    executionRecordDetailInsight(parsed) ??
    executionRecordInsight(parsed) ??
    operatorRunInsight(toolName, parsed)
  );
}
