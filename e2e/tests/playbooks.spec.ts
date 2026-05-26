/**
 * Critical path: the Playbooks panel is reachable and the chain composer mounts.
 *
 * Flow: open Settings → click the "Playbooks" nav item → the dedicated panel
 * renders (heading + Compose Chain entry + empty state) → opening the composer
 * surfaces the operator chain editor. Proves the end-to-end mount that makes
 * crystallize/replay reachable (Tauri IPC is mocked via fixtures).
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
    await expect(page.getByRole("button", { name: "Compose Chain" })).toBeVisible();
    // Default mock returns no playbooks → empty state.
    await expect(page.getByText("No playbooks yet")).toBeVisible();
  });

  test("Compose Chain opens the operator chain editor", async ({ page }) => {
    await page
      .getByRole("navigation", { name: "Settings sections" })
      .getByText("Playbooks", { exact: true })
      .click();

    await expect(page.getByRole("button", { name: "Compose Chain" })).toBeVisible({
      timeout: 5_000,
    });
    await page.getByRole("button", { name: "Compose Chain" }).click();

    // The chain editor dialog mounts on top of settings (a second dialog appears).
    const dialogs = page.locator('[role="dialog"]');
    await expect(async () => {
      expect(await dialogs.count()).toBeGreaterThan(1);
    }).toPass({ timeout: 5_000 });
  });
});
