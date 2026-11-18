const DEFAULT_ACCENT = "#3b82f6";

/** Normalize a user/loaded color string to lowercase `#rrggbb`, or `fallback` if invalid. */
export function normalizeHexColor(input: string | null | undefined, fallback = DEFAULT_ACCENT): string {
  if (typeof input !== "string") return fallback;
  const raw = input.trim().replace(/^#/, "");
  const hex =
    raw.length === 3
      ? raw
          .split("")
          .map((c) => c + c)
          .join("")
      : raw;
  if (!/^[0-9a-fA-F]{6}$/.test(hex)) return fallback;
  return `#${hex.toLowerCase()}`;
}

/** Derive a soft/translucent accent for hover/selection fills from a hex color. */
export function accentSoftFromHex(hex: string, alpha = 0.18): string {
  const n = normalizeHexColor(hex);
  const r = parseInt(n.slice(1, 3), 16);
  const g = parseInt(n.slice(3, 5), 16);
  const b = parseInt(n.slice(5, 7), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

export { DEFAULT_ACCENT };
