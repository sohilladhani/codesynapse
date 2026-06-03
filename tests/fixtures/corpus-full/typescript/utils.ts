export function slugify(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

export function truncate(text: string, maxLen = 100): string {
  return text.length > maxLen ? text.slice(0, maxLen) + "..." : text;
}

export function paginate<T>(items: T[], page: number, perPage = 20): T[] {
  const start = (page - 1) * perPage;
  return items.slice(start, start + perPage);
}

export function chunk<T>(items: T[], size: number): T[][] {
  const result: T[][] = [];
  for (let i = 0; i < items.length; i += size) {
    result.push(items.slice(i, i + size));
  }
  return result;
}

export function deepMerge<T extends Record<string, unknown>>(base: T, override: Partial<T>): T {
  const result = { ...base };
  for (const [k, v] of Object.entries(override)) {
    if (v && typeof v === "object" && !Array.isArray(v) && k in result && typeof result[k] === "object") {
      (result as Record<string, unknown>)[k] = deepMerge(result[k] as Record<string, unknown>, v as Record<string, unknown>);
    } else {
      (result as Record<string, unknown>)[k] = v;
    }
  }
  return result;
}

export function parseJsonSafe<T = unknown>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}
