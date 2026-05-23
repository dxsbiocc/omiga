import { test, expect } from "@playwright/test";
import { mountTauriMock } from "../setup/tauri-mock";

const taskId = "task-test-1";
const operatorAlias = "test_op";

const operatorCatalog = {
  registryPath: "/tmp/operator-registry.json",
  diagnostics: [],
  operators: [
    {
      id: operatorAlias,
      version: "0.1.0",
      name: operatorAlias,
      description: "Simple test operator",
      sourcePlugin: "test-plugin",
      manifestPath: "/plugins/test-plugin/operators/test-op.yaml",
      interface: {},
      execution: { argv: ["echo", "ok"] },
      runtime: null,
      resources: {},
      smokeTests: [
        {
          id: "smoke",
          name: "Smoke test",
          arguments: {
            inputs: {},
            params: {},
            resources: {},
          },
        },
      ],
      enabledAliases: [operatorAlias],
      exposed: true,
      unavailableReason: null,
    },
  ],
};

test("background-run shows running chip and clears on cancel", async ({ page }) => {
  await mountTauriMock(page, {
    commandMocks: {
      list_omiga_plugin_marketplaces: [],
      list_plugins: [],
      list_operators: operatorCatalog,
      list_operator_runs: [],
      list_active_operator_tasks: [],
      run_operator_async: {
        response: { taskId },
        events: [
          {
            event: `operator-task-${taskId}`,
            payload: { type: "started", taskId, alias: operatorAlias },
          },
        ],
      },
      cancel_operator_task: {
        response: null,
        events: [
          {
            event: `operator-task-${taskId}`,
            payload: { type: "cancelled", taskId },
          },
        ],
      },
    },
  });

  await page.goto("/");
  await expect(page.locator("#omiga-session-panel")).toBeVisible({
    timeout: 15_000,
  });

  await page.locator('[data-testid="user-menu-trigger"]').click();
  await page.locator('[data-testid="menu-item-settings"]').click();

  const settingsDialog = page.locator(
    '[role="dialog"][aria-labelledby="omiga-settings-title"]',
  );
  await expect(settingsDialog).toBeVisible({ timeout: 5_000 });

  await settingsDialog
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("button", { name: "Plugins" })
    .click();

  await expect(
    settingsDialog.getByRole("heading", { name: "Plugins" }).first(),
  ).toBeVisible();

  await settingsDialog
    .getByRole("button", { name: /Agent tools/i })
    .first()
    .click();

  const operatorList = settingsDialog.getByRole("region", {
    name: "Plugin tool list",
  });
  await expect(
    operatorList.getByText(operatorAlias, { exact: true }).first(),
  ).toBeVisible();

  await operatorList
    .getByRole("button", { name: "Background", exact: true })
    .click();

  const runningChip = operatorList.getByText(/Running/i);
  await expect(runningChip).toBeVisible();

  await expect(
    page.getByRole("button", { name: /Open async operator tasks/i }),
  ).toBeVisible();
  await expect(
    page.locator(".MuiBadge-badge").filter({ hasText: "1" }),
  ).toBeVisible();

  await operatorList
    .getByRole("button", { name: "Cancel", exact: true })
    .click();

  await expect(runningChip).toBeHidden();
});
