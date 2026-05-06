import { describe, expect, it } from "vitest";
import { inferIntent, permissionPromptLabels } from "./PermissionPromptBar";
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
});
