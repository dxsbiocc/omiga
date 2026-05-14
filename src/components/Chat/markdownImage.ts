export type MarkdownImageReference =
  | {
      kind: "blocked-inline";
      rawSrc: string;
    }
  | {
      kind: "remote";
      rawSrc: string;
      src: string;
    }
  | {
      kind: "local";
      rawSrc: string;
      localPath: string;
      suffix: string;
    };

export const MARKDOWN_LOCAL_IMAGE_EXT_RE =
  /\.(?:png|jpe?g|gif|webp|svg|bmp|ico|tiff?|avif)(?:[?#].*)?$/i;

export function hasUrlScheme(value: string): boolean {
  return /^[a-z][a-z0-9+.-]*:/i.test(value);
}

export function isAbsoluteLocalPath(value: string): boolean {
  return value.startsWith("/") || /^[a-zA-Z]:[\\/]/.test(value);
}

export function stripFileQueryAndHash(value: string): {
  path: string;
  suffix: string;
} {
  const index = value.search(/[?#]/);
  if (index === -1) return { path: value, suffix: "" };
  return { path: value.slice(0, index), suffix: value.slice(index) };
}

function workspaceRelativeLocalPath(raw: string, workspacePath: string) {
  const base = workspacePath.replace(/[\\/]+$/, "");
  const relative = raw.replace(/^\.?[\\/]+/, "");
  return stripFileQueryAndHash(`${base}/${relative}`);
}

export function resolveMarkdownImageReference(
  src: string,
  workspacePath: string,
): MarkdownImageReference {
  const raw = src.trim();
  if (!raw || /^data:image\/[^;]+;base64,/i.test(raw)) {
    return { kind: "blocked-inline", rawSrc: raw };
  }

  if (/^(?:https?|blob|asset|tauri):/i.test(raw)) {
    return { kind: "remote", rawSrc: raw, src: raw };
  }

  if (/^file:\/\//i.test(raw)) {
    try {
      const url = new URL(raw);
      return {
        kind: "local",
        rawSrc: raw,
        localPath: decodeURIComponent(url.pathname),
        suffix: `${url.search}${url.hash}`,
      };
    } catch {
      return { kind: "remote", rawSrc: raw, src: raw };
    }
  }

  if (isAbsoluteLocalPath(raw)) {
    const { path, suffix } = stripFileQueryAndHash(raw);
    return { kind: "local", rawSrc: raw, localPath: path, suffix };
  }

  if (!hasUrlScheme(raw) && workspacePath && MARKDOWN_LOCAL_IMAGE_EXT_RE.test(raw)) {
    const { path, suffix } = workspaceRelativeLocalPath(raw, workspacePath);
    return { kind: "local", rawSrc: raw, localPath: path, suffix };
  }

  return { kind: "remote", rawSrc: raw, src: raw };
}
