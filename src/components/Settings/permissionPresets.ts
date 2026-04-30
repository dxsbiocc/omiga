/** Claude-style `permissions.deny` rule strings; merged with `~/.claude` / `.claude`. */

export type PermissionPreset = {
  /** Written to JSON when checked */
  rule: string;
  label: string;
  /** Match existing file lines case-insensitively */
  aliases: string[];
};

export const PERMISSION_PRESETS: PermissionPreset[] = [
  { rule: "Bash", label: "终端命令 (Bash)", aliases: ["bash", "Bash"] },
  { rule: "Read", label: "读取文件", aliases: ["Read", "file_read", "FileRead"] },
  { rule: "Write", label: "写入文件", aliases: ["Write", "file_write", "FileWrite"] },
  { rule: "Edit", label: "编辑文件", aliases: ["Edit", "file_edit", "FileEdit"] },
  {
    rule: "Ripgrep",
    label: "Ripgrep 搜索",
    aliases: ["ripgrep", "Ripgrep", "grep", "Grep"],
  },
  { rule: "Glob", label: "Glob 文件匹配", aliases: ["glob", "Glob"] },
  { rule: "Fetch", label: "网页抓取", aliases: ["fetch", "Fetch"] },
  { rule: "Query", label: "数据库查询", aliases: ["query", "Query"] },
  { rule: "Search", label: "网络搜索", aliases: ["search", "Search"] },
  { rule: "Agent", label: "子代理 (Agent)", aliases: ["Agent", "agent"] },
  { rule: "skill", label: "Skill 工具", aliases: ["skill", "Skill", "SkillTool"] },
  { rule: "TodoWrite", label: "Todo 列表", aliases: ["todo_write", "TodoWrite"] },
  { rule: "NotebookEdit", label: "Notebook 编辑", aliases: ["notebook_edit", "NotebookEdit"] },
  {
    rule: "AskUserQuestion",
    label: "询问用户",
    aliases: ["ask_user_question", "AskUserQuestion"],
  },
  {
    rule: "ListMcpResourcesTool",
    label: "列出 MCP 资源",
    aliases: ["list_mcp_resources", "ListMcpResourcesTool", "ListMcpResources"],
  },
  {
    rule: "ReadMcpResourceTool",
    label: "读取 MCP 资源",
    aliases: ["read_mcp_resource", "ReadMcpResourceTool", "ReadMcpResource"],
  },
  { rule: "TaskStop", label: "停止任务", aliases: ["task_stop", "TaskStop", "KillShell"] },
];

export function denyMatchesPreset(denyLine: string, aliases: string[]): boolean {
  const t = denyLine.trim();
  if (!t) return false;
  const lower = t.toLowerCase();
  return aliases.some((a) => a.toLowerCase() === lower);
}

export function isPresetRule(denyLine: string): boolean {
  return PERMISSION_PRESETS.some((p) => denyMatchesPreset(denyLine, p.aliases));
}

export function buildDenyList(
  presetChecked: Record<string, boolean>,
  customBlock: string,
): string[] {
  const out: string[] = [];
  for (const p of PERMISSION_PRESETS) {
    if (presetChecked[p.rule]) out.push(p.rule);
  }
  for (const line of customBlock.split(/\r?\n/)) {
    const t = line.trim();
    if (t) out.push(t);
  }
  return [...new Set(out)];
}

export function parseDenyIntoState(deny: string[]): {
  presetChecked: Record<string, boolean>;
  customBlock: string;
} {
  const presetChecked: Record<string, boolean> = {};
  for (const p of PERMISSION_PRESETS) {
    presetChecked[p.rule] = deny.some((d) => denyMatchesPreset(d, p.aliases));
  }
  const customLines = deny.filter((d) => !isPresetRule(d));
  return {
    presetChecked,
    customBlock: customLines.join("\n"),
  };
}
