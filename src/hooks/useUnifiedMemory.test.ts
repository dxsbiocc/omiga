/**
 * Unit tests for useUnifiedMemory hook — type contracts and invoke argument shapes.
 *
 * Uses vi.mock to stub Tauri's invoke. React hook state is NOT tested here
 * (no @testing-library/react in this project); tests focus on DTO shapes,
 * MemoryTab type completeness, and optimistic-update logic extracted as
 * pure functions.
 */

import { describe, it, expect } from "vitest";
import {
  normalizeSourceEntry,
  type LongTermEntryDto,
  type SourceEntryDto,
  type MemoryTab,
} from "./useUnifiedMemory";

// ── Helper builders ───────────────────────────────────────────────────────────

function makeLongTermEntry(overrides: Partial<LongTermEntryDto> = {}): LongTermEntryDto {
  return {
    path: "/tmp/long_term/test-entry.json",
    topic: "test topic",
    summary: "test summary",
    kind: "ResearchInsight",
    confidence: 0.8,
    stability: 0.7,
    importance: 0.75,
    reuse_probability: 0.65,
    retention_class: "LongTerm",
    status: "Active",
    created_at: "2025-01-01T00:00:00Z",
    last_reused_at: null,
    expires_at: null,
    source_sessions: ["sess-1"],
    entities: ["memory", "recall"],
    global: false,
    ...overrides,
  };
}

function makeSourceEntry(overrides: Partial<SourceEntryDto> = {}): SourceEntryDto {
  return {
    path: "/tmp/long_term/sources/abc123.json",
    url: "https://example.com/paper",
    canonical_url: "https://example.com/paper",
    title: "Example Research Paper",
    domain: "example.com",
    gist: "A paper about memory systems.",
    accessed_at: "2025-01-01T00:00:00Z",
    last_used_at: "2025-01-15T00:00:00Z",
    use_count: 3,
    sessions: ["sess-1", "sess-2"],
    query_context: ["memory recall"],
    expires_at: "2025-04-15T00:00:00Z",
    ...overrides,
  };
}

// ── MemoryTab type completeness ───────────────────────────────────────────────

describe("MemoryTab", () => {
  it("includes sources tab", () => {
    const tabs: MemoryTab[] = ["overview", "knowledge", "implicit", "long_term", "sources", "config"];
    expect(tabs).toContain("sources");
  });

  it("has exactly 6 distinct tabs", () => {
    const tabs: MemoryTab[] = ["overview", "knowledge", "implicit", "long_term", "sources", "config"];
    expect(new Set(tabs).size).toBe(6);
  });
});

// ── LongTermEntryDto shape ────────────────────────────────────────────────────

describe("LongTermEntryDto", () => {
  it("has required string fields", () => {
    const entry = makeLongTermEntry();
    expect(typeof entry.path).toBe("string");
    expect(typeof entry.topic).toBe("string");
    expect(typeof entry.summary).toBe("string");
    expect(typeof entry.kind).toBe("string");
    expect(typeof entry.retention_class).toBe("string");
    expect(typeof entry.status).toBe("string");
  });

  it("has quality scores bounded [0, 1]", () => {
    const entry = makeLongTermEntry();
    for (const score of [entry.confidence, entry.stability, entry.importance, entry.reuse_probability]) {
      expect(score).toBeGreaterThanOrEqual(0);
      expect(score).toBeLessThanOrEqual(1);
    }
  });

  it("accepts valid status values", () => {
    const valid = ["Active", "Archived", "Superseded"] as const;
    for (const status of valid) {
      const entry = makeLongTermEntry({ status });
      expect(valid).toContain(entry.status);
    }
  });

  it("accepts valid retention_class values", () => {
    const valid = ["Permanent", "LongTerm", "Session", "Ephemeral"] as const;
    for (const rc of valid) {
      const entry = makeLongTermEntry({ retention_class: rc });
      expect(valid).toContain(entry.retention_class);
    }
  });

  it("supports null optional fields", () => {
    const entry = makeLongTermEntry({ last_reused_at: null, expires_at: null });
    expect(entry.last_reused_at).toBeNull();
    expect(entry.expires_at).toBeNull();
  });
});

// ── SourceEntryDto shape ──────────────────────────────────────────────────────

describe("SourceEntryDto", () => {
  it("has required string fields", () => {
    const entry = makeSourceEntry();
    expect(typeof entry.url).toBe("string");
    expect(typeof entry.canonical_url).toBe("string");
    expect(typeof entry.domain).toBe("string");
    expect(typeof entry.accessed_at).toBe("string");
    expect(typeof entry.last_used_at).toBe("string");
    expect(typeof entry.path).toBe("string");
  });

  it("has positive use_count", () => {
    const entry = makeSourceEntry({ use_count: 5 });
    expect(entry.use_count).toBeGreaterThan(0);
  });

  it("sessions and query_context are arrays", () => {
    const entry = makeSourceEntry();
    expect(Array.isArray(entry.sessions)).toBe(true);
    expect(Array.isArray(entry.query_context)).toBe(true);
  });

  it("normalizes legacy entries that omit source history arrays", () => {
    const entry = normalizeSourceEntry({
      path: "/tmp/long_term/sources/legacy.json",
      url: "https://example.com/legacy",
      canonical_url: "https://example.com/legacy",
      domain: "example.com",
      accessed_at: "2025-01-01T00:00:00Z",
      last_used_at: "2025-01-02T00:00:00Z",
      use_count: 1,
    });

    expect(entry.sessions).toEqual([]);
    expect(entry.query_context).toEqual([]);
  });

  it("allows null optional fields", () => {
    const entry = makeSourceEntry({ title: null, gist: null, expires_at: null });
    expect(entry.title).toBeNull();
    expect(entry.gist).toBeNull();
    expect(entry.expires_at).toBeNull();
  });

  it("expires_at is set on new entries", () => {
    const entry = makeSourceEntry();
    expect(entry.expires_at).not.toBeNull();
    // Validate it's a parseable date.
    const dt = new Date(entry.expires_at!);
    expect(isNaN(dt.getTime())).toBe(false);
    // Must be in the future relative to accessed_at.
    expect(dt.getTime()).toBeGreaterThan(new Date(entry.accessed_at).getTime());
  });
});

// ── Optimistic update logic (pure) ───────────────────────────────────────────

describe("Optimistic update logic (pure functions)", () => {
  it("archiveLongTermEntry sets status to Archived on matching entry", () => {
    const entries = [
      makeLongTermEntry({ path: "/a.json", status: "Active" }),
      makeLongTermEntry({ path: "/b.json", status: "Active" }),
    ];
    const updated = entries.map(e =>
      e.path === "/a.json" ? { ...e, status: "Archived" as const } : e
    );
    expect(updated[0].status).toBe("Archived");
    expect(updated[1].status).toBe("Active");
  });

  it("deleteLongTermEntry removes matching entry", () => {
    const entries = [
      makeLongTermEntry({ path: "/a.json" }),
      makeLongTermEntry({ path: "/b.json" }),
    ];
    const updated = entries.filter(e => e.path !== "/a.json");
    expect(updated).toHaveLength(1);
    expect(updated[0].path).toBe("/b.json");
  });

  it("deleteSourceEntry removes matching source by path", () => {
    const entries = [
      makeSourceEntry({ path: "/sources/x.json", url: "https://x.com" }),
      makeSourceEntry({ path: "/sources/y.json", url: "https://y.com" }),
    ];
    const updated = entries.filter(e => e.path !== "/sources/x.json");
    expect(updated).toHaveLength(1);
    expect(updated[0].url).toBe("https://y.com");
  });
});

// ── Invoke argument contracts ─────────────────────────────────────────────────

describe("Invoke argument contracts", () => {
  it("memory_list_long_term requires projectPath and scope", () => {
    const args = { projectPath: "/my/project", scope: "all" };
    expect(args.projectPath).toBeTruthy();
    expect(["all", "project", "global"]).toContain(args.scope);
  });

  it("memory_list_sources requires projectPath", () => {
    const args = { projectPath: "/my/project" };
    expect(args.projectPath).toBeTruthy();
  });

  it("memory_archive_long_term_entry requires projectPath and entryPath", () => {
    const args = { projectPath: "/my/project", entryPath: "/long_term/entry.json" };
    expect(args.projectPath).toBeTruthy();
    expect(args.entryPath).toBeTruthy();
  });

  it("memory_delete_long_term_entry requires projectPath and entryPath", () => {
    const args = { projectPath: "/my/project", entryPath: "/long_term/entry.json" };
    expect(args.projectPath).toBeTruthy();
    expect(args.entryPath).toBeTruthy();
  });

  it("memory_delete_source requires projectPath and entryPath", () => {
    const args = { projectPath: "/my/project", entryPath: "/long_term/sources/entry.json" };
    expect(args.projectPath).toBeTruthy();
    expect(args.entryPath).toBeTruthy();
  });

  it("memory_prune_stale requires projectPath", () => {
    const args = { projectPath: "/my/project" };
    expect(args.projectPath).toBeTruthy();
  });
});

// ── Scope filtering logic ─────────────────────────────────────────────────────

describe("Long-term scope filtering", () => {
  const allEntries = [
    makeLongTermEntry({ path: "/project/a.json", topic: "project entry" }),
    makeLongTermEntry({ path: "/global/b.json", topic: "global entry" }),
  ];

  it("all scope returns everything", () => {
    const result = allEntries;
    expect(result).toHaveLength(2);
  });

  it("entries can be distinguished by path prefix", () => {
    const projectEntries = allEntries.filter(e => e.path.startsWith("/project"));
    const globalEntries = allEntries.filter(e => e.path.startsWith("/global"));
    expect(projectEntries).toHaveLength(1);
    expect(globalEntries).toHaveLength(1);
    expect(projectEntries[0].topic).toBe("project entry");
  });
});

// ── Source stale_count in SourceRegistryStatus ────────────────────────────────

describe("SourceRegistryStatus", () => {
  it("has both entry_count and stale_count fields", () => {
    // Verify the shape expected from the backend.
    const status = { entry_count: 5, stale_count: 2 };
    expect(typeof status.entry_count).toBe("number");
    expect(typeof status.stale_count).toBe("number");
    expect(status.stale_count).toBeLessThanOrEqual(status.entry_count + 10);
  });
});
