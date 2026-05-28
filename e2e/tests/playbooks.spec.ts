/**
 * Critical path: the Playbooks management panel is reachable from Settings.
 *
 * Flow: open Settings → click the "Playbooks" nav item → the dedicated panel
 * renders (heading + Refresh + empty state). Chain composition (and Save as
 * Playbook) lives in the operator chain editor under the Plugins tab.
 * Tauri IPC is mocked via fixtures.
 */
import { test, expect } from "../fixtures";

test.describe("Playbooks panel", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("#omiga-session-panel")).toBeVisible({
      timeout: 15_000,
    });
    await page.locator('[data-testid="user-menu-trigger"]').click();
    await page.locator('[data-testid="menu-item-settings"]').click();
    await expect(
      page.locator('[role="dialog"][aria-labelledby="omiga-settings-title"]'),
    ).toBeVisible({ timeout: 5_000 });
  });

  test("opens the Playbooks panel from settings navigation", async ({ page }) => {
    await page
      .getByRole("navigation", { name: "Settings sections" })
      .getByText("Playbooks", { exact: true })
      .click();

    // Panel-specific content (avoids matching the sidebar nav label).
    await expect(
      page.getByText("Review distilled playbooks and replay them against the current project."),
    ).toBeVisible({ timeout: 5_000 });
    await expect(page.getByRole("button", { name: "Refresh" })).toBeVisible();
    // Default mock returns no playbooks → empty state.
    await expect(page.getByText("No playbooks yet")).toBeVisible();
  });
});
