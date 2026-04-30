import { describe, expect, it } from "vitest";
import {
  findCustomEditorForFile,
  getIconThemeContributions,
  globToRegExp,
  isExtensionInstalled,
  languageForFile,
  resolveIconForFileNode,
  resolveIconTheme,
  type InstalledVscodeExtension,
} from "./vscodeExtensions";

function extension(packageJson: InstalledVscodeExtension["packageJson"]): InstalledVscodeExtension {
  return {
    id: "acme.demo",
    name: "demo",
    displayName: "Demo Extension",
    publisher: "acme",
    version: "1.0.0",
    description: "",
    path: "/extensions/acme.demo",
    enabled: true,
    packageJson,
  };
}

describe("VS Code extension contribution helpers", () => {
  it("matches VS Code-style custom editor filename patterns", () => {
    const ext = extension({
      contributes: {
        customEditors: [
          {
            viewType: "drawio.editor",
            displayName: "Draw.io Preview",
            selector: [{ filenamePattern: "*.drawio" }],
            priority: "default",
          },
        ],
      },
    });

    expect(findCustomEditorForFile("diagram.drawio", "/repo/diagram.drawio", [ext]))
      .toMatchObject({ viewType: "drawio.editor", extensionId: "acme.demo" });
    expect(findCustomEditorForFile("diagram.txt", "/repo/diagram.txt", [ext])).toBeNull();
    expect(globToRegExp("**/*.drawio").test("/repo/a/b/diagram.drawio")).toBe(true);
  });

  it("uses contributed languages for filenames and compound extensions", () => {
    const ext = extension({
      contributes: {
        languages: [
          { id: "astro", extensions: [".astro"] },
          { id: "typescript", extensions: [".d.ts"] },
          { id: "dotenv", filenames: [".env.local"] },
        ],
      },
    });

    expect(languageForFile("Card.astro", [ext])).toBe("astro");
    expect(languageForFile("index.d.ts", [ext])).toBe("typescript");
    expect(languageForFile(".env.local", [ext])).toBe("dotenv");
  });

  it("detects installed extension IDs case-insensitively", () => {
    const ext = extension({});

    expect(isExtensionInstalled([ext], "acme.demo")).toBe(true);
    expect(isExtensionInstalled([ext], "ACME.DEMO")).toBe(true);
    expect(isExtensionInstalled([ext], "missing.demo")).toBe(false);
  });

  it("resolves icon themes with file names, compound extensions, and folders", () => {
    const ext = extension({
      contributes: {
        iconThemes: [{ id: "acme-icons", label: "Acme Icons", path: "./themes/icons.json" }],
      },
    });

    expect(getIconThemeContributions([ext])).toEqual([
      expect.objectContaining({
        id: "acme-icons",
        path: "./themes/icons.json",
        extensionId: "acme.demo",
      }),
    ]);

    const theme = resolveIconTheme([ext], "acme-icons", {
      iconDefinitions: {
        "_file": { iconPath: "./file.svg" },
        "_folder": { iconPath: "./folder.svg" },
        "_package": { iconPath: "./package.svg" },
        "_dts": { iconPath: "./dts.svg" },
      },
      file: "_file",
      folder: "_folder",
      fileNames: { "package.json": "_package" },
      fileExtensions: { "d.ts": "_dts" },
    });

    expect(resolveIconForFileNode(theme, { name: "package.json", isDirectory: false }))
      .toMatchObject({ iconPath: "/extensions/acme.demo/themes/package.svg" });
    expect(resolveIconForFileNode(theme, { name: "index.d.ts", isDirectory: false }))
      .toMatchObject({ iconPath: "/extensions/acme.demo/themes/dts.svg" });
    expect(resolveIconForFileNode(theme, { name: "src", isDirectory: true }))
      .toMatchObject({ iconPath: "/extensions/acme.demo/themes/folder.svg" });
  });

  it("confines icon theme documents and assets to the extension directory", () => {
    const unsafeThemePath = extension({
      contributes: {
        iconThemes: [{ id: "escape-theme", label: "Escape", path: "../outside.json" }],
      },
    });

    expect(resolveIconTheme([unsafeThemePath], "escape-theme", {})).toBeNull();

    const ext = extension({
      contributes: {
        iconThemes: [{ id: "safe-theme", label: "Safe", path: "./themes/icons.json" }],
      },
    });
    const baseTheme = resolveIconTheme([ext], "safe-theme", {
      iconDefinitions: {
        "_file": { iconPath: "../../outside.svg" },
        "_absolute": { iconPath: "/etc/passwd" },
        "_safe": { iconPath: "./safe.svg" },
      },
      fileNames: {
        "outside.txt": "_file",
        "absolute.txt": "_absolute",
        "safe.txt": "_safe",
      },
    });

    expect(resolveIconForFileNode(baseTheme, { name: "outside.txt", isDirectory: false }))
      .toBeNull();
    expect(resolveIconForFileNode(baseTheme, { name: "absolute.txt", isDirectory: false }))
      .toBeNull();
    expect(resolveIconForFileNode(baseTheme, { name: "safe.txt", isDirectory: false }))
      .toMatchObject({ iconPath: "/extensions/acme.demo/themes/safe.svg" });
  });
});
