// Shared helpers for text-like widgets (<input> text/password and <textarea>).

export type WrappedLine = {
  // Index range in the source string (end is exclusive).
  start: number;
  end: number;
  text: string;
};

export function wrapFieldTextWithIndices(
  text: string,
  maxWidth: number,
  measure: (s: string) => number
): WrappedLine[] {
  const s = String(text ?? '');
  const out: WrappedLine[] = [];

  // Wrap each hard line (split by \n) independently, preserving indices.
  let lineStart = 0;
  for (let i = 0; i <= s.length; i++) {
    const isBreak = i === s.length || s[i] === '\n';
    if (!isBreak) continue;

    const paraStart = lineStart;
    const paraEnd = i;
    if (paraStart === paraEnd) {
      // Preserve empty lines.
      out.push({ start: paraStart, end: paraEnd, text: '' });
    } else {
      let segStart = paraStart;
      let lastSpace = -1;

      for (let pos = segStart; pos < paraEnd; pos++) {
        const ch = s[pos];
        if (ch === ' ') lastSpace = pos;

        // Measure [segStart..pos] inclusive.
        const next = s.slice(segStart, pos + 1);
        if (measure(next) <= maxWidth || pos === segStart) continue;

        // Break at the last space if available; include the space at end of the line
        // so indices stay contiguous.
        let breakPos = lastSpace >= segStart ? lastSpace + 1 : pos;
        if (breakPos <= segStart) breakPos = Math.min(paraEnd, segStart + 1);

        out.push({ start: segStart, end: breakPos, text: s.slice(segStart, breakPos) });
        segStart = breakPos;
        pos = segStart - 1;
        lastSpace = -1;
      }

      if (segStart <= paraEnd) {
        out.push({ start: segStart, end: paraEnd, text: s.slice(segStart, paraEnd) });
      }
    }

    // Skip the newline.
    lineStart = i + 1;
  }

  return out;
}

export function clampWrappedLines(lines: WrappedLine[], maxLines: number): WrappedLine[] {
  if (maxLines <= 0) return [];
  if (lines.length <= maxLines) return lines;
  // For editable fields, do not append "..." because it breaks index mapping.
  return lines.slice(0, maxLines);
}

export function getCaretIndexFromPoint(opts: {
  fullText: string;
  lines: WrappedLine[];
  localX: number;
  localY: number;
  lineHeight: number;
  measure: (s: string) => number;
}): number {
  const { fullText, lines, localX, localY, lineHeight, measure } = opts;
  if (lines.length === 0) return 0;

  const x = Math.max(0, localX);
  const y = Math.max(0, localY);
  const lh = Math.max(1, lineHeight);
  const lineIdx = Math.max(0, Math.min(lines.length - 1, Math.floor(y / lh)));
  const line = lines[lineIdx];

  // Choose the closest caret position in this line.
  let best = line.start;
  let bestDist = Number.POSITIVE_INFINITY;
  for (let i = line.start; i <= line.end; i++) {
    const w = measure(fullText.slice(line.start, i));
    const d = Math.abs(w - x);
    if (d < bestDist) {
      bestDist = d;
      best = i;
    }
  }
  return best;
}
