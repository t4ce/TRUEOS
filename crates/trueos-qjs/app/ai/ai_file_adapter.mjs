const FILE_TREE_MAX_CHARS = 12_000;

export function normalizeJsonFileTree(raw) {
  if (typeof raw !== 'string' || !raw.trim()) {
    return '';
  }

  try {
    const parsed = JSON.parse(raw);
    const entries = Array.isArray(parsed && parsed.entries) ? parsed.entries : [];
    const compact = {
      version: Number(parsed && parsed.version || 1) || 1,
      root: String(parsed && parsed.root || '/'),
      max_entries: Number(parsed && parsed.max_entries || entries.length || 0) || 0,
      truncated: !!(parsed && parsed.truncated),
      entries: entries.map((entry) => ({
        path: String(entry && entry.path || ''),
        kind: String(entry && entry.kind || ''),
        depth: Number(entry && entry.depth || 0) || 0,
      })),
    };
    return JSON.stringify(compact).slice(0, FILE_TREE_MAX_CHARS);
  } catch {
    return String(raw).slice(0, FILE_TREE_MAX_CHARS);
  }
}
