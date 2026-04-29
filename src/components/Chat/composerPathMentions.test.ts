import { describe, expect, it } from "vitest";
import {
  buildComposerMentionChildPath,
  buildComposerPathInjection,
  filterComposerMentionRows,
  formatComposerPathPreview,
  joinWorkspaceMentionDirectory,
  mergeComposerPathsAndBody,
  parentComposerMentionDirectory,
  parseComposerFileMentionInput,
  pathsStillMatchMergedContent,
  splitLeadingPathPrefixFromMerged,
  sortComposerMentionRows,
  stripLeadingPathPrefixFromMerged,
} from "./composerPathMentions";

describe("composerPathMentions", () => {
  it("parses nested @ file picker queries into directory and basename filter", () => {
    expect(parseComposerFileMentionInput("@")).toEqual({
      active: true,
      query: "",
      directory: "",
      filter: "",
    });
    expect(parseComposerFileMentionInput("@src/components/Cha")).toEqual({
      active: true,
      query: "src/components/Cha",
      directory: "src/components",
      filter: "Cha",
    });
    expect(parseComposerFileMentionInput("@src//components/")).toEqual({
      active: true,
      query: "src/components/",
      directory: "src/components",
      filter: "",
    });
    expect(parseComposerFileMentionInput("look @src")).toEqual({
      active: false,
      query: "",
      directory: "",
      filter: "",
    });
  });

  it("builds workspace list paths and child mention paths without guessing", () => {
    expect(joinWorkspaceMentionDirectory("/repo", "")).toBe("/repo");
    expect(joinWorkspaceMentionDirectory("/repo/", "src/components")).toBe(
      "/repo/src/components",
    );
    expect(joinWorkspaceMentionDirectory("/", "workspace/src")).toBe(
      "/workspace/src",
    );
    expect(buildComposerMentionChildPath("src", "App.tsx")).toBe(
      "src/App.tsx",
    );
    expect(parentComposerMentionDirectory("src/components/Chat")).toBe(
      "src/components",
    );
  });

  it("sorts folders first and filters within the current directory", () => {
    const rows = sortComposerMentionRows([
      { path: "src/App.tsx", is_file: true, size: 12 },
      { path: "src/components", is_file: false, size: 0 },
      { path: "src/components/ChatComposer.tsx", is_file: true, size: 34 },
    ]);

    expect(rows.map((row) => row.path)).toEqual([
      "src/components",
      "src/App.tsx",
      "src/components/ChatComposer.tsx",
    ]);
    expect(filterComposerMentionRows(rows, "chat")).toEqual([
      { path: "src/components/ChatComposer.tsx", is_file: true, size: 34 },
    ]);
  });

  it("injects selected paths as exact model-facing context and strips it for UI", () => {
    const paths = ["src/components/Chat/ChatComposer.tsx", "src/App.tsx"];
    const body = "请检查这些文件";
    const merged = mergeComposerPathsAndBody(paths, body);

    expect(merged).toContain("<omiga-selected-paths>");
    expect(merged).toContain("- src/components/Chat/ChatComposer.tsx");
    expect(merged).toContain("do not infer or guess alternatives");
    expect(stripLeadingPathPrefixFromMerged(merged, paths)).toBe(body);
    expect(pathsStillMatchMergedContent(paths, merged)).toBe(true);
  });

  it("recovers paths from stored injection blocks when metadata is missing", () => {
    const merged = [
      "<omiga-selected-paths>",
      "The user selected these workspace-relative paths with the @ picker. Use these exact path strings when reading or editing files; do not infer or guess alternatives:",
      "- data/QSDB_qsgroups.txt",
      "- data/QSDB.fasta",
      "</omiga-selected-paths>",
      "",
      "提取文件中与 QS 核心相关的分组、基因",
    ].join("\n");

    expect(splitLeadingPathPrefixFromMerged(merged)).toEqual({
      paths: ["data/QSDB_qsgroups.txt", "data/QSDB.fasta"],
      body: "提取文件中与 QS 核心相关的分组、基因",
      hasPathPrefix: true,
    });
    expect(stripLeadingPathPrefixFromMerged(merged, [])).toBe(
      "提取文件中与 QS 核心相关的分组、基因",
    );
  });

  it("keeps legacy @path prefixes readable for old stored messages", () => {
    const paths = ["src/App.tsx"];
    const legacy = `${formatComposerPathPreview(paths)}\n\nBody`;

    expect(stripLeadingPathPrefixFromMerged(legacy, paths)).toBe("Body");
    expect(pathsStillMatchMergedContent(paths, legacy)).toBe(true);
    expect(buildComposerPathInjection([])).toBe("");
  });
});
