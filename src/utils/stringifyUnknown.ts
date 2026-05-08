export function stringifyUnknown(value: unknown): string {
  if (typeof value === "string") return value;
  if (value == null) return "";
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (typeof value === "bigint") return `${value}n`;
  if (value instanceof Error) {
    return value.stack || value.message || String(value);
  }

  const seen = new WeakSet<object>();
  const replacer = (_key: string, v: unknown): unknown => {
    if (typeof v === "bigint") return `${v}n`;
    if (typeof v === "function") return `[Function ${v.name || "anonymous"}]`;
    if (typeof v === "symbol") return v.toString();
    if (v && typeof v === "object") {
      if (seen.has(v as object)) return "[Circular]";
      seen.add(v as object);
    }
    return v;
  };

  try {
    const serialized = JSON.stringify(value, replacer, 2);
    if (typeof serialized === "string") return serialized;
  } catch {
    // ignore and fall back below
  }

  const plain = Object.prototype.toString.call(value);
  return plain === "[object Object]" ? "{...}" : String(value);
}

