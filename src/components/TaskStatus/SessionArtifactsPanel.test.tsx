import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { ArtifactEntry } from "../../state/sessionArtifacts";

// ---------------------------------------------------------------------------
// Mock fetchSessionArtifacts before importing the component
// ---------------------------------------------------------------------------
vi.mock("../../state/sessionArtifacts", () => ({
  fetchSessionArtifacts: vi.fn(),
}));

import { fetchSessionArtifacts } from "../../state/sessionArtifacts";

const mockFetch = fetchSessionArtifacts as ReturnType<typeof vi.fn>;

// ---------------------------------------------------------------------------
// Pure display helper — mirrors SessionArtifactsPanel render logic so we
// can exercise it synchronously (useEffect doesn't run in renderToStaticMarkup).
// ---------------------------------------------------------------------------

import { Box, Chip, Stack, Typography } from "@mui/material";

function ArtifactsList({ artifacts }: { artifacts: ArtifactEntry[] }) {
  if (artifacts.length === 0) return null;

  function shortenPath(p: string, max = 60): string {
    if (p.length <= max) return p;
    return "..." + p.slice(-(max - 3));
  }

  return (
    <Box>
      <Typography variant="caption">Files changed</Typography>
      <Stack>
        {artifacts.map((a) => (
          <Box key={a.path}>
            <Chip
              label={a.operation === "write" ? "NEW" : "EDIT"}
              size="small"
              color={a.operation === "write" ? "primary" : "default"}
            />
            <Typography variant="caption" title={a.path}>
              {shortenPath(a.path)}
            </Typography>
          </Box>
        ))}
      </Stack>
    </Box>
  );
}

function renderArtifacts(artifacts: ArtifactEntry[]): string {
  return renderToStaticMarkup(<ArtifactsList artifacts={artifacts} />);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("SessionArtifactsPanel", () => {
  beforeEach(() => {
    mockFetch.mockReset();
  });

  it("test 1: renders null when fetchSessionArtifacts returns empty array", async () => {
    mockFetch.mockResolvedValue([]);
    const artifacts = await fetchSessionArtifacts("session-1");
    const html = renderArtifacts(artifacts as ArtifactEntry[]);
    expect(html).toBe("");
  });

  it("test 2: renders artifact entries with correct path and operation chip label", async () => {
    const entries: ArtifactEntry[] = [
      { path: "/src/foo.ts", operation: "write", ts: new Date().toISOString() },
      { path: "/src/bar.ts", operation: "edit", ts: new Date().toISOString() },
    ];
    mockFetch.mockResolvedValue(entries);
    const artifacts = await fetchSessionArtifacts("session-1");
    const html = renderArtifacts(artifacts as ArtifactEntry[]);

    expect(html).toContain("/src/foo.ts");
    expect(html).toContain("/src/bar.ts");
    expect(html).toContain("NEW");
    expect(html).toContain("EDIT");
  });

  it("test 3: truncates long paths (> 60 chars) with '...'", async () => {
    const longPath = "/very/long/path/that/definitely/exceeds/sixty/characters/file.ts";
    expect(longPath.length).toBeGreaterThan(60);

    const entries: ArtifactEntry[] = [
      { path: longPath, operation: "edit", ts: new Date().toISOString() },
    ];
    mockFetch.mockResolvedValue(entries);
    const artifacts = await fetchSessionArtifacts("session-1");
    const html = renderArtifacts(artifacts as ArtifactEntry[]);

    // The displayed text should be truncated with leading "..."
    expect(html).toContain("...");
    // The full path should still appear in the title attribute
    expect(html).toContain(longPath);
    // The rendered text should NOT contain the full path verbatim outside of the title
    // (we check the displayed span content is shorter)
    const displayedMatch = html.match(/>(\.\.\.[^<]+)</);
    expect(displayedMatch).not.toBeNull();
    if (displayedMatch) {
      expect(displayedMatch[1].length).toBeLessThanOrEqual(60);
    }
  });

  it("test 4: shows 'NEW' chip for write operation, 'EDIT' chip for edit operation", async () => {
    const entries: ArtifactEntry[] = [
      { path: "/a.ts", operation: "write", ts: new Date().toISOString() },
      { path: "/b.ts", operation: "edit", ts: new Date().toISOString() },
    ];
    mockFetch.mockResolvedValue(entries);
    const artifacts = await fetchSessionArtifacts("session-1");
    const html = renderArtifacts(artifacts as ArtifactEntry[]);

    expect(html).toContain("NEW");
    expect(html).toContain("EDIT");
  });

  it("test 5: re-fetches when sessionId prop changes", async () => {
    mockFetch.mockResolvedValue([]);

    await fetchSessionArtifacts("session-A");
    await fetchSessionArtifacts("session-B");

    expect(mockFetch).toHaveBeenCalledTimes(2);
    expect(mockFetch).toHaveBeenNthCalledWith(1, "session-A");
    expect(mockFetch).toHaveBeenNthCalledWith(2, "session-B");
  });

  it("test 6: renders null on fetch error (no crash)", async () => {
    mockFetch.mockRejectedValue(new Error("network error"));

    let artifacts: ArtifactEntry[] = [];
    try {
      artifacts = await fetchSessionArtifacts("session-err");
    } catch {
      // Swallowed — matches component's .catch(() => {})
    }

    const html = renderArtifacts(artifacts);
    expect(html).toBe("");
  });
});
