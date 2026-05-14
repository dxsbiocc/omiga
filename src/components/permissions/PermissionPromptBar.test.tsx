import { describe, expect, it } from "vitest";
import {
  PERMISSION_CONNECTOR_PREVIEW_MAX_HEIGHT,
  PERMISSION_PROMPT_ACTION_BUTTON_FONT_SIZE,
  PERMISSION_PROMPT_ACTION_BUTTON_HEIGHT,
  PERMISSION_PROMPT_HEADER_ALIGN_ITEMS,
  PERMISSION_PROMPT_HEADER_MIN_HEIGHT,
  PERMISSION_PROMPT_ROOT_OVERFLOW_Y,
  PERMISSION_RUN_CONTENT_MAX_HEIGHT,
  inferIntent,
  permissionCommandSafetyKind,
  permissionInstallChoiceQuestions,
  permissionPromptDisplayTitle,
  permissionPromptLabels,
  permissionSessionApprovalCopy,
} from "./PermissionPromptBar";
import { inferConnectorPermissionIntent } from "../../utils/connectorPermissionIntent";

describe("PermissionPromptBar connector intent", () => {
  it("normalizes direct Slack write arguments for user-facing approval copy", () => {
    const intent = inferConnectorPermissionIntent("connector", {
      connector: "slack",
      operation: "send_message",
      channel: "C123",
      threadTs: "1712345678.123456",
      text: "Ship it",
      confirmWrite: true,
    });

    expect(intent).toMatchObject({
      connectorId: "slack",
      connectorLabel: "Slack",
      operation: "post_message",
      operationLabel: "发送消息",
      isWrite: true,
      target: "C123 thread 1712345678.123456",
      payloadPreview: "Ship it",
    });
  });

  it("normalizes nested execute_tool Connector arguments", () => {
    const intent = inferConnectorPermissionIntent("execute_tool", {
      tool: "Connector",
      arguments: {
        connector: "Linear",
        operation: "update_issue_status",
        id: "ENG-1",
        status: "Done",
        confirm_write: true,
      },
    });

    expect(intent).toMatchObject({
      connectorId: "linear",
      connectorLabel: "Linear",
      operation: "update_issue_status",
      operationLabel: "更新 Issue 状态",
      isWrite: true,
      target: "ENG-1",
      payloadPreview: "Done",
    });
  });

  it("uses external-service write labels instead of generic high-risk run copy", () => {
    const intent = inferIntent("connector", {
      connector: "slack",
      operation: "post_message",
      channel: "C123",
      thread_ts: "1712345678.123456",
      text: "Ship it",
      confirm_write: true,
    });
    const labels = permissionPromptLabels(intent.connector, true, false);

    expect(intent.title).toBe("外部服务写入确认");
    expect(intent.detail).toBe("Slack · 发送消息 · C123 thread 1712345678.123456");
    expect(labels.approveLabel).toBe("允许写入");
    expect(labels.allowOnceLabel).toBe("仅允许这一次写入");
    expect(labels.sessionLabel).toBe("本次会话内允许同一连接器操作");
    expect(labels.connectorWarning).toContain("修改 Slack 中的数据");
    expect(labels.connectorWarning).toContain("批准或拒绝都会写入连接器审计记录");
    expect(labels.approveLabel).not.toBe("运行（高风险）");
  });

  it("uses Bash descriptions as the concrete request operation", () => {
    const intent = inferIntent("Bash", {
      description: "Per-group FCN3 expression statistics",
      command: "python3 - <<'PYEOF'\nprint('stats')\nPYEOF",
    });

    expect(intent.title).toBe("执行命令");
    expect(intent.operation).toBe("运行：Per-group FCN3 expression statistics");
    expect(intent.contentLabel).toBe("运行内容");
    expect(intent.detail).toContain("python3");
  });

  it("shows only the concrete Bash title in the prompt header", () => {
    const intent = inferIntent("Bash", {
      description: "检查 pymol 是否安装",
      command: "which pymol",
    });

    expect(permissionPromptDisplayTitle(intent)).toBe("检查 pymol 是否安装");
  });

  it("falls back to raw Bash arguments when command text is missing", () => {
    const intent = inferIntent("Bash", {
      description: "生成最终可视化 JSON",
      script: "python3 -c 'print(1)'",
    });

    expect(permissionPromptDisplayTitle(intent)).toBe("生成最终可视化 JSON");
    expect(intent.detail).toContain('"script"');
    expect(intent.detail).toContain("python3 -c");
  });

  it("warns when a Bash request contains no displayable command arguments", () => {
    const intent = inferIntent("Bash", {});

    expect(permissionPromptDisplayTitle(intent)).toBe("Shell 命令");
    expect(intent.detail).toContain("未收到命令内容");
  });

  it("makes session approval scope explicit for shell commands", () => {
    const copy = permissionSessionApprovalCopy("Bash", undefined);

    expect(copy.buttonLabel).toBe("本会话允许同类命令");
    expect(copy.title).toContain("仅同类命令");
    expect(copy.title).toContain("危险命令仍会重新确认");
  });

  it("keeps connector session approval scoped to the same operation", () => {
    const intent = inferIntent("connector", {
      connector: "slack",
      operation: "post_message",
      channel: "C123",
      text: "Ship it",
      confirm_write: true,
    });
    const copy = permissionSessionApprovalCopy("connector", intent.connector);

    expect(copy.buttonLabel).toBe("本会话允许同类操作");
    expect(copy.title).toContain("同一连接器的同一操作");
    expect(copy.title).toContain("其它连接器或操作仍会确认");
  });

  it("detects software install commands for choice-based approval", () => {
    expect(
      permissionCommandSafetyKind("bash", { command: "npm install left-pad" }),
    ).toBe("install");
    expect(
      permissionCommandSafetyKind("bash", { command: "python3 -m pip install pandas" }),
    ).toBe("install");
    expect(
      permissionCommandSafetyKind("bash", { command: "npm test" }),
    ).toBeNull();
  });

  it("detects destructive deletion commands as single-use approvals", () => {
    expect(
      permissionCommandSafetyKind("bash", { command: "rm -rf /tmp/demo" }),
    ).toBe("destructive");
    expect(
      permissionCommandSafetyKind("bash", { command: "git reset --hard HEAD" }),
    ).toBe("destructive");
  });

  it("asks install location through choices instead of yes/no", () => {
    const [question] = permissionInstallChoiceQuestions();
    const labels = question.options.map((opt) => opt.label);

    expect(question.question).toContain("请选择安装位置");
    expect(labels).toContain("安装到当前项目/虚拟环境（推荐）");
    expect(labels).toContain("按当前命令安装（仅本次）");
    expect(labels).toContain("自定义安装位置");
    expect(labels).toContain("不安装");
    expect(labels).not.toContain("是");
    expect(labels).not.toContain("否");
  });

  it("normalizes email connector send requests", () => {
    const intent = inferConnectorPermissionIntent("connector", {
      connector: "qq-mail",
      operation: "send_email",
      to: "user@example.com",
      subject: "周报",
      confirm_write: true,
    });

    expect(intent).toMatchObject({
      connectorId: "qq_mail",
      connectorLabel: "QQ 邮箱",
      operation: "send_message",
      operationLabel: "发送邮件",
      isWrite: true,
      target: "user@example.com",
      payloadPreview: "周报",
    });
  });

  it("keeps the permission prompt itself non-scrollable and constrains only detail previews", () => {
    expect(PERMISSION_PROMPT_ROOT_OVERFLOW_Y).toBe("visible");
    expect(PERMISSION_RUN_CONTENT_MAX_HEIGHT).toBe(
      "clamp(96px, 22vh, 220px)",
    );
    expect(PERMISSION_CONNECTOR_PREVIEW_MAX_HEIGHT).toBe(
      "clamp(96px, 18vh, 180px)",
    );
    expect(PERMISSION_PROMPT_ACTION_BUTTON_HEIGHT).toBe(32);
    expect(PERMISSION_PROMPT_ACTION_BUTTON_FONT_SIZE).toBe("0.8rem");
    expect(PERMISSION_PROMPT_HEADER_ALIGN_ITEMS).toBe("center");
    expect(PERMISSION_PROMPT_HEADER_MIN_HEIGHT).toBe(28);
  });
});
