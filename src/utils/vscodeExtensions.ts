import { convertFileSrc } from "@tauri-apps/api/core";

export interface InstalledVscodeExtension {
  id: string;
  name: string;
  displayName: string;
  publisher: string;
  version: string;
  description: string;
  path: string;
  enabled: boolean;
  packageJson: VscodePackageJson;
}

export interface RecommendedVscodeExtension {
  id: string;
  name: string;
  publisher: string;
  displayName: string;
  description: string;
  repositoryUrl: string;
  marketplaceUrl: string;
  downloadUrl: string;
}

export interface VscodePackageJson {
  name?: string;
  displayName?: string;
  publisher?: string;
  version?: string;
  description?: string;
  activationEvents?: unknown;
  contributes?: VscodeContributes;
  [key: string]: unknown;
}

export interface VscodeContributes {
  languages?: VscodeLanguageContribution[];
  iconThemes?: VscodeIconThemeContributionManifest[];
  customEditors?: VscodeCustomEditorContributionManifest[];
  notebooks?: VscodeNotebookContributionManifest[];
  [key: string]: unknown;
}

export interface VscodeLanguageContribution {
  id?: string;
  aliases?: string[];
  extensions?: string[];
  filenames?: string[];
  filenamePatterns?: string[];
  [key: string]: unknown;
}

export interface VscodeIconThemeContributionManifest {
  id?: string;
  label?: string;
  path?: string;
  [key: string]: unknown;
}

export interface VscodeCustomEditorContributionManifest {
  viewType?: string;
  displayName?: string;
  selector?: Array<{ filenamePattern?: string; [key: string]: unknown }>;
  priority?: string;
  [key: string]: unknown;
}

export interface VscodeNotebookContributionManifest {
  type?: string;
  displayName?: string;
  selector?: Array<{ filenamePattern?: string; [key: string]: unknown }>;
  [key: string]: unknown;
}

export interface IconThemeContribution {
  id: string;
  label: string;
  path: string;
  extensionId: string;
  extensionName: string;
  extensionPath: string;
}

export interface CustomEditorContribution {
  viewType: string;
  displayName: string;
  selector: Array<{ filenamePattern: string }>;
  priority: string;
  extensionId: string;
  extensionName: string;
}

export interface NotebookContribution {
  type: string;
  displayName: string;
  selector: Array<{ filenamePattern: string }>;
  extensionId: string;
  extensionName: string;
}

export interface VscodeIconThemeDocument {
  iconDefinitions?: Record<string, { iconPath?: string; fontCharacter?: string }>;
  file?: string;
  folder?: string;
  folderExpanded?: string;
  rootFolder?: string;
  rootFolderExpanded?: string;
  fileExtensions?: Record<string, string>;
  fileNames?: Record<string, string>;
  languageIds?: Record<string, string>;
  folderNames?: Record<string, string>;
  folderNamesExpanded?: Record<string, string>;
  hidesExplorerArrows?: boolean;
  [key: string]: unknown;
}

export interface ResolvedIconTheme extends IconThemeContribution {
  document: VscodeIconThemeDocument;
  themeDir: string;
}

export interface ResolvedIcon {
  iconPath: string;
  definitionId: string;
}

export const DEFAULT_ICON_THEME_ID = "omiga.material-default";

function asArray<T = unknown>(value: unknown): T[] {
  return Array.isArray(value) ? (value as T[]) : [];
}

function basename(path: string): string {
  return path.split(/[/\\]/u).filter(Boolean).pop() ?? path;
}

function dirname(path: string): string {
  const normalized = path.replace(/\\/gu, "/");
  const i = normalized.lastIndexOf("/");
  return i <= 0 ? "" : normalized.slice(0, i);
}

function stripLeadingDot(ext: string): string {
  return ext.trim().replace(/^\.+/u, "").toLowerCase();
}

function extensionSuffixes(fileName: string): string[] {
  const base = basename(fileName).toLowerCase();
  const parts = base.split(".");
  if (parts.length < 2) return [];
  const suffixes: string[] = [];
  for (let i = 0; i < parts.length - 1; i += 1) {
    const suffix = parts.slice(i + 1).join(".");
    if (suffix) suffixes.push(suffix);
  }
  // Longest suffix first: `d.ts` must beat `ts`.
  return Array.from(new Set(suffixes)).sort((a, b) => b.length - a.length);
}

function getExtensionName(ext: InstalledVscodeExtension): string {
  return ext.displayName || ext.packageJson.displayName || ext.name || ext.id;
}

export function isExtensionInstalled(
  extensions: InstalledVscodeExtension[],
  extensionId: string,
): boolean {
  const normalizedId = extensionId.trim().toLowerCase();
  return extensions.some((extension) => extension.id.toLowerCase() === normalizedId);
}

function normalizeSelector(
  selector: VscodeCustomEditorContributionManifest["selector"],
): Array<{ filenamePattern: string }> {
  return asArray<{ filenamePattern?: string }>(selector)
    .map((entry) => ({ filenamePattern: String(entry?.filenamePattern ?? "").trim() }))
    .filter((entry) => entry.filenamePattern.length > 0);
}

function escapeRegExp(s: string): string {
  return s.replace(/[|\\{}()[\]^$+?.]/gu, "\\$&");
}

export function globToRegExp(pattern: string): RegExp {
  let out = "^";
  for (let i = 0; i < pattern.length; i += 1) {
    const ch = pattern[i];
    const next = pattern[i + 1];
    if (ch === "*" && next === "*") {
      out += ".*";
      i += 1;
    } else if (ch === "*") {
      out += "[^/]*";
    } else if (ch === "?") {
      out += "[^/]";
    } else if (ch === "/") {
      out += "\\/";
    } else {
      out += escapeRegExp(ch);
    }
  }
  out += "$";
  return new RegExp(out, "iu");
}

export function matchesFilenamePattern(pattern: string, fileName: string, filePath = fileName): boolean {
  const normalizedPattern = pattern.trim().replace(/\\/gu, "/");
  if (!normalizedPattern) return false;
  const normalizedPath = filePath.replace(/\\/gu, "/");
  const base = basename(normalizedPath);

  if (!normalizedPattern.includes("/") && globToRegExp(normalizedPattern).test(base)) {
    return true;
  }
  return globToRegExp(normalizedPattern).test(normalizedPath);
}

export function joinFsPath(base: string, ...parts: string[]): string {
  const firstAbsolute = parts.find((part) => /^([a-zA-Z]:)?[\\/]/u.test(part));
  if (firstAbsolute) {
    return normalizeFsPath(firstAbsolute);
  }
  return normalizeFsPath([base, ...parts].filter(Boolean).join("/"));
}

function normalizeFsPath(path: string): string {
  const driveMatch = path.match(/^([a-zA-Z]:)(.*)$/u);
  const drive = driveMatch?.[1] ?? "";
  const rest = (driveMatch?.[2] ?? path).replace(/\\/gu, "/");
  const isAbsolute = rest.startsWith("/");
  const segments: string[] = [];
  for (const part of rest.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      segments.pop();
    } else {
      segments.push(part);
    }
  }
  return `${drive}${isAbsolute ? "/" : ""}${segments.join("/")}`;
}

function isAbsoluteOrDrivePath(path: string): boolean {
  return /^([a-zA-Z]:)?[\\/]/u.test(path) || /^[a-zA-Z]:/u.test(path);
}

function isSameOrDescendant(root: string, candidate: string): boolean {
  const normalizedRoot = normalizeFsPath(root);
  const normalizedCandidate = normalizeFsPath(candidate);
  const rootForCompare = normalizedRoot.toLowerCase();
  const candidateForCompare = normalizedCandidate.toLowerCase();
  const rootPrefix = rootForCompare.endsWith("/")
    ? rootForCompare
    : `${rootForCompare}/`;
  return (
    candidateForCompare === rootForCompare ||
    candidateForCompare.startsWith(rootPrefix)
  );
}

function resolveContainedExtensionPath(
  extensionPath: string,
  baseDir: string,
  rawPath: string,
): string | null {
  const rel = rawPath.trim();
  if (!rel || isAbsoluteOrDrivePath(rel)) return null;
  const resolved = joinFsPath(baseDir, rel);
  return isSameOrDescendant(extensionPath, resolved) ? resolved : null;
}

export function filePathToAssetSrc(filePath: string): string {
  try {
    return convertFileSrc(filePath);
  } catch {
    return filePath;
  }
}

export function getIconThemeContributions(
  extensions: InstalledVscodeExtension[],
): IconThemeContribution[] {
  return extensions.flatMap((extension) =>
    asArray<VscodeIconThemeContributionManifest>(
      extension.packageJson.contributes?.iconThemes,
    )
      .map((theme) => ({
        id: String(theme.id ?? "").trim(),
        label: String(theme.label ?? theme.id ?? "").trim(),
        path: String(theme.path ?? "").trim(),
        extensionId: extension.id,
        extensionName: getExtensionName(extension),
        extensionPath: extension.path,
      }))
      .filter((theme) => theme.id && theme.path),
  );
}

export function getCustomEditorContributions(
  extensions: InstalledVscodeExtension[],
): CustomEditorContribution[] {
  return extensions.flatMap((extension) =>
    asArray<VscodeCustomEditorContributionManifest>(
      extension.packageJson.contributes?.customEditors,
    )
      .map((editor) => ({
        viewType: String(editor.viewType ?? "").trim(),
        displayName: String(editor.displayName ?? editor.viewType ?? "").trim(),
        selector: normalizeSelector(editor.selector),
        priority: String(editor.priority ?? "default").trim() || "default",
        extensionId: extension.id,
        extensionName: getExtensionName(extension),
      }))
      .filter((editor) => editor.viewType && editor.selector.length > 0),
  );
}

export function getNotebookContributions(
  extensions: InstalledVscodeExtension[],
): NotebookContribution[] {
  return extensions.flatMap((extension) =>
    asArray<VscodeNotebookContributionManifest>(
      extension.packageJson.contributes?.notebooks,
    )
      .map((notebook) => ({
        type: String(notebook.type ?? "").trim(),
        displayName: String(notebook.displayName ?? notebook.type ?? "").trim(),
        selector: normalizeSelector(notebook.selector),
        extensionId: extension.id,
        extensionName: getExtensionName(extension),
      }))
      .filter((notebook) => notebook.type && notebook.selector.length > 0),
  );
}

export function languageForFile(
  fileName: string,
  extensions: InstalledVscodeExtension[],
): string | undefined {
  const base = basename(fileName);
  const baseLower = base.toLowerCase();
  const suffixes = extensionSuffixes(base);

  for (const extension of extensions) {
    for (const lang of asArray<VscodeLanguageContribution>(
      extension.packageJson.contributes?.languages,
    )) {
      const id = String(lang.id ?? "").trim();
      if (!id) continue;

      const filenames = asArray<string>(lang.filenames).map((n) => n.toLowerCase());
      if (filenames.includes(baseLower)) return id;

      const patterns = asArray<string>(lang.filenamePatterns);
      if (patterns.some((pattern) => matchesFilenamePattern(pattern, base))) {
        return id;
      }

      const langExts = asArray<string>(lang.extensions).map(stripLeadingDot);
      if (suffixes.some((suffix) => langExts.includes(suffix))) {
        return id;
      }
    }
  }

  return undefined;
}

export function findCustomEditorForFile(
  fileName: string,
  filePath: string,
  extensions: InstalledVscodeExtension[],
): CustomEditorContribution | null {
  const matches = getCustomEditorContributions(extensions).filter((editor) =>
    editor.selector.some((entry) =>
      matchesFilenamePattern(entry.filenamePattern, fileName, filePath || fileName),
    ),
  );
  matches.sort((a, b) => {
    const ap = a.priority === "default" ? 0 : 1;
    const bp = b.priority === "default" ? 0 : 1;
    return ap - bp || a.displayName.localeCompare(b.displayName);
  });
  return matches[0] ?? null;
}

export function resolveIconTheme(
  extensions: InstalledVscodeExtension[],
  themeId: string,
  document: VscodeIconThemeDocument,
): ResolvedIconTheme | null {
  const contribution = getIconThemeContributions(extensions).find(
    (theme) => theme.id === themeId,
  );
  if (!contribution) return null;

  const themeFile = resolveContainedExtensionPath(
    contribution.extensionPath,
    contribution.extensionPath,
    contribution.path,
  );
  if (!themeFile) return null;
  return {
    ...contribution,
    document,
    themeDir: dirname(themeFile),
  };
}

function lookupCaseInsensitive(map: Record<string, string> | undefined, key: string): string | undefined {
  if (!map) return undefined;
  const direct = map[key] ?? map[key.toLowerCase()] ?? map[key.toUpperCase()];
  if (direct) return direct;
  const lower = key.toLowerCase();
  return Object.entries(map).find(([entryKey]) => entryKey.toLowerCase() === lower)?.[1];
}

function iconPathForDefinition(
  theme: ResolvedIconTheme,
  definitionId: string | undefined,
): ResolvedIcon | null {
  if (!definitionId) return null;
  const def = theme.document.iconDefinitions?.[definitionId];
  if (!def?.iconPath) return null;
  const iconPath = resolveContainedExtensionPath(
    theme.extensionPath,
    theme.themeDir,
    def.iconPath,
  );
  if (!iconPath) return null;
  return {
    definitionId,
    iconPath,
  };
}

export function resolveIconForFileNode(
  theme: ResolvedIconTheme | null,
  node: { name: string; isDirectory: boolean },
  extensions: InstalledVscodeExtension[] = [],
  opts: { isOpen?: boolean; isRoot?: boolean } = {},
): ResolvedIcon | null {
  if (!theme) return null;
  const doc = theme.document;
  const base = basename(node.name);
  const baseLower = base.toLowerCase();

  if (node.isDirectory) {
    const folderNames = opts.isOpen ? doc.folderNamesExpanded : doc.folderNames;
    const namedFolder = lookupCaseInsensitive(folderNames, baseLower);
    const fallback =
      opts.isRoot && opts.isOpen
        ? doc.rootFolderExpanded
        : opts.isRoot
          ? doc.rootFolder
          : opts.isOpen
            ? doc.folderExpanded
            : doc.folder;
    return iconPathForDefinition(theme, namedFolder ?? fallback);
  }

  const fileNameIcon = lookupCaseInsensitive(doc.fileNames, baseLower);
  if (fileNameIcon) return iconPathForDefinition(theme, fileNameIcon);

  for (const suffix of extensionSuffixes(base)) {
    const extIcon = lookupCaseInsensitive(doc.fileExtensions, suffix);
    if (extIcon) return iconPathForDefinition(theme, extIcon);
  }

  const languageId = languageForFile(base, extensions);
  if (languageId) {
    const languageIcon = lookupCaseInsensitive(doc.languageIds, languageId);
    if (languageIcon) return iconPathForDefinition(theme, languageIcon);
  }

  return iconPathForDefinition(theme, doc.file);
}

export function contributionSummary(extension: InstalledVscodeExtension): {
  languages: number;
  iconThemes: number;
  customEditors: number;
  notebooks: number;
} {
  return {
    languages: asArray(extension.packageJson.contributes?.languages).length,
    iconThemes: asArray(extension.packageJson.contributes?.iconThemes).length,
    customEditors: asArray(extension.packageJson.contributes?.customEditors).length,
    notebooks: asArray(extension.packageJson.contributes?.notebooks).length,
  };
}
