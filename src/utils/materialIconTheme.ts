/**
 * `react-material-icon-theme` 的 getFileIcon 只在传入 `fileExtension` 时按后缀匹配；
 * 仅传 `fileName` 只会匹配少数精确文件名（如 package.json），其余会落到默认 file 图标。
 */
export function materialIconFileExtension(fileName: string): string | undefined {
  const base = fileName.split(/[/\\]/).pop() ?? fileName;
  if (!base || base === "." || base === "..") return undefined;
  const dot = base.lastIndexOf(".");
  if (dot < 0) return undefined;
  if (dot === base.length - 1) return undefined;
  return base.slice(dot + 1).toLowerCase();
}
