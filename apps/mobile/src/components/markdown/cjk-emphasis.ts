// MD4C applies CommonMark flanking rules, which assume words are separated by
// spaces. In CJK prose a closing `**` after punctuation and before a letter
// (`月。**选`) cannot close, and an opening `**` after a letter and before
// punctuation (`种**“固`) cannot open. Moving such a run across the adjacent
// punctuation makes it flanking and only changes whether the mark is bold.
// Edits reorder characters and never change the source length.

// Explicit ranges: native engines reject the property escapes Metro emits for `u`.
const CJK_TEXT =
  /[\u1100-\u11FF\u2E80-\u2FFF\u3000-\u9FFF\uA960-\uA97F\uAC00-\uD7FF\uF900-\uFAFF\uFE30-\uFE4F\uFF00-\uFFEF]|[\uD840-\uD888][\uDC00-\uDFFF]/;
const CJK_RANGES = [
  [0x1100, 0x11ff],
  [0x2e80, 0x2fff],
  [0x3000, 0x9fff],
  [0xa960, 0xa97f],
  [0xac00, 0xd7ff],
  [0xf900, 0xfaff],
  [0xfe30, 0xfe4f],
  [0xff00, 0xffef],
  [0x20000, 0x323af],
] as const;
// MD4C's Unicode P and S categories, limited to blocks that appear beside CJK
// prose. Missing a mark only leaves the run as MD4C already parses it.
const PUNCTUATION_RANGES = [
  [0xa1, 0xa9],
  [0xab, 0xac],
  [0xae, 0xb1],
  [0xb4, 0xb4],
  [0xb6, 0xb8],
  [0xbb, 0xbb],
  [0xbf, 0xbf],
  [0xd7, 0xd7],
  [0xf7, 0xf7],
  [0x2010, 0x2027],
  [0x2030, 0x205e],
  [0x20a0, 0x20c0],
  [0x2190, 0x2429],
  [0x2440, 0x244a],
  [0x249c, 0x24e9],
  [0x2500, 0x2775],
  [0x2794, 0x2b73],
  [0x2b76, 0x2bff],
  [0x2e00, 0x2e5d],
  [0x3001, 0x3004],
  [0x3008, 0x3020],
  [0x3030, 0x3030],
  [0x303d, 0x303f],
  [0x30fb, 0x30fb],
  [0xfe10, 0xfe19],
  [0xfe30, 0xfe52],
  [0xfe54, 0xfe66],
  [0xfe68, 0xfe6b],
  [0xff01, 0xff0f],
  [0xff1a, 0xff20],
  [0xff3b, 0xff40],
  [0xff5b, 0xff65],
  [0xffe0, 0xffe6],
  [0xffe8, 0xffee],
  [0x1f10d, 0x1fbff],
] as const;
const UNICODE_SPACES = new Set([0xa0, 0x1680, 0x202f, 0x205f, 0x3000]);
// ASCII marks a run may cross. Others start Markdown syntax or entities.
const CROSSABLE_ASCII = new Set(".,:?'\"%()");

const SPACE = 0;
const PUNCTUATION = 1;
const OTHER = 2;

interface Edit {
  start: number;
  end: number;
  text: string;
}
interface Opener {
  length: number;
  edits: Edit[];
}
// Link text resolves its emphasis apart from the surrounding text.
interface Frame {
  openers: Opener[];
  edits: Edit[];
  strayCloser: boolean;
}
type Unit = [number, number][];

function inRanges(code: number, ranges: readonly (readonly [number, number])[]): boolean {
  return ranges.some(([low, high]) => code >= low && code <= high);
}

function level(code: number | undefined): number {
  if (code === undefined) return SPACE;
  if (code <= 0x7f) {
    if (code === 0x20 || (code >= 0x09 && code <= 0x0d)) return SPACE;
    const punctuation =
      (code >= 0x21 && code <= 0x2f) ||
      (code >= 0x3a && code <= 0x40) ||
      (code >= 0x5b && code <= 0x60) ||
      (code >= 0x7b && code <= 0x7e);
    return punctuation ? PUNCTUATION : OTHER;
  }
  if (UNICODE_SPACES.has(code) || (code >= 0x2000 && code <= 0x200a)) return SPACE;
  return inRanges(code, PUNCTUATION_RANGES) ? PUNCTUATION : OTHER;
}

function isCjk(code: number | undefined): boolean {
  return code !== undefined && level(code) === OTHER && inRanges(code, CJK_RANGES);
}

function crossable(code: number | undefined): boolean {
  if (code === undefined || level(code) !== PUNCTUATION) return false;
  return code > 0x7f || CROSSABLE_ASCII.has(String.fromCharCode(code));
}

function codeAfter(source: string, index: number, end: number): number | undefined {
  return index < end ? source.codePointAt(index) : undefined;
}

function codeBefore(source: string, index: number, start: number): number | undefined {
  if (index <= start) return undefined;
  const low = source.charCodeAt(index - 1);
  if (low >= 0xdc00 && low <= 0xdfff && index - 2 >= start) {
    const high = source.charCodeAt(index - 2);
    if (high >= 0xd800 && high <= 0xdbff) return source.codePointAt(index - 2);
  }
  return low;
}

function width(code: number): number {
  return code > 0xffff ? 2 : 1;
}

const LINE_PREFIX = /^((?:[ \t]{0,3}>[ ]?)*)([ \t]*)/;
const FENCE = /^(`{3,}|~{3,})/;
const THEMATIC_BREAK = /^([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const LIST_MARKER = /^(?:[-+*]|\d{1,9}[.)])(?:[ \t]+(?:\[[ xX]\][ \t]+)?|$)/;
const TABLE_DELIMITER = /^\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$/;

interface Line {
  content: number;
  end: number;
  depth: number;
  indent: number;
  text: string;
}

function splitLines(source: string): Line[] {
  const lines: Line[] = [];
  let start = 0;
  while (start <= source.length) {
    const newline = source.indexOf("\n", start);
    const lineEnd = newline === -1 ? source.length : newline;
    const end = source[lineEnd - 1] === "\r" ? lineEnd - 1 : lineEnd;
    const prefix = LINE_PREFIX.exec(source.slice(start, end))!;
    const indent = [...prefix[2]].reduce((sum, char) => sum + (char === "\t" ? 4 : 1), 0);
    const content = start + prefix[0].length;
    lines.push({
      content,
      end,
      depth: prefix[1].split(">").length - 1,
      indent,
      text: source.slice(content, end),
    });
    if (newline === -1) break;
    start = newline + 1;
  }
  return lines;
}

/** Group prose into inline units; skip code, HTML, definitions and breaks. */
function inlineUnits(source: string): Unit[] {
  const lines = splitLines(source);
  const units: Unit[] = [];
  let paragraph: Unit = [];
  let fence: string | null = null;
  let html = false;
  let table = false;
  let list = false;
  const flush = () => {
    if (paragraph.length > 0) units.push(paragraph);
    paragraph = [];
  };
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    const text = line.text;
    if (fence !== null) {
      const close = FENCE.exec(text);
      if (close && close[1][0] === fence[0] && close[1].length >= fence.length) {
        if (/^[ \t]*$/.test(text.slice(close[1].length))) fence = null;
      }
      continue;
    }
    const blank = /^[ \t]*$/.test(text);
    if (blank) {
      html = table = false;
      flush();
      continue;
    }
    if (html) continue;
    let open = paragraph.length > 0;
    // A deeper quote marker starts a new block; a shallower one is lazy continuation.
    if (open && line.depth > lines[index - 1].depth) {
      flush();
      open = false;
    }
    if (line.indent >= 4 && !open && !table) continue;
    const opening = FENCE.exec(text);
    if (opening && !(opening[1][0] === "`" && text.slice(opening[1].length).includes("`"))) {
      flush();
      table = false;
      fence = opening[1];
      continue;
    }
    if (/^<[A-Za-z/!?]/.test(text)) {
      flush();
      table = false;
      html = true;
      continue;
    }
    if (THEMATIC_BREAK.test(text) || (open && /^(?:=+|-+)[ \t]*$/.test(text))) {
      flush();
      table = false;
      continue;
    }
    if (/^\[[^\]]+\]:/.test(text)) {
      flush();
      continue;
    }
    const heading = /^#{1,6}(?:[ \t]+|$)/.exec(text);
    const marker = LIST_MARKER.exec(text);
    // Outside a list, only a bullet or `1.` with content interrupts a paragraph.
    const item =
      marker && (!open || list || /^(?:[-+*]|1[.)])[ \t]+[^ \t]/.test(text)) ? marker : null;
    if (heading || item) table = false;
    const next = lines[index + 1];
    // MD4C only reads a table header from the first line of a block.
    if (!table && !open && text.includes("|") && next?.text.includes("|")) {
      table = TABLE_DELIMITER.test(next.text);
    }
    if (table) {
      if (TABLE_DELIMITER.test(text)) continue;
      // GFM splits cells at every unescaped pipe, even inside code spans.
      let cell = line.content;
      for (let offset = line.content; offset <= line.end; offset++) {
        if (offset === line.end || (source[offset] === "|" && source[offset - 1] !== "\\")) {
          if (offset > cell) units.push([[cell, offset]]);
          cell = offset + 1;
        }
      }
      continue;
    }
    if (heading) {
      flush();
      list = false;
      units.push([[line.content + heading[0].length, line.end]]);
      continue;
    }
    if (item) {
      flush();
      list = true;
      paragraph = [[line.content + item[0].length, line.end]];
      continue;
    }
    if (!open && line.indent === 0) list = false;
    paragraph.push([line.content, line.end]);
  }
  flush();
  return units;
}

/** Collect delimiter moves for one paragraph, heading or table cell. */
function adaptUnit(source: string, unit: Unit, edits: Edit[]) {
  // Maximal backtick runs by length; each length's cursor only moves forward.
  const ticks = new Map<number, number[]>();
  const cursors = new Map<number, number>();
  for (const [start, end] of unit) {
    for (let index = start; index < end; index++) {
      if (source[index] !== "`") continue;
      let close = index;
      while (close < end && source[close] === "`") close++;
      const runs = ticks.get(close - index) ?? [];
      runs.push(index);
      ticks.set(close - index, runs);
      index = close - 1;
    }
  }
  const codeSpanEnd = (length: number, from: number): number => {
    const runs = ticks.get(length) ?? [];
    let cursor = cursors.get(length) ?? 0;
    while (cursor < runs.length && runs[cursor] < from) cursor++;
    cursors.set(length, cursor);
    return cursor < runs.length ? runs[cursor] + length : -1;
  };
  const frames: Frame[] = [{ openers: [], edits: [], strayCloser: false }];
  let resume = 0;
  for (const [start, end] of unit) {
    let index = Math.max(start, resume);
    // Moves never reach back into a code span, link destination or delimiter.
    let boundary = index;
    let tagEnd = -1;
    while (index < end) {
      const char = source[index];
      const frame = frames[frames.length - 1];
      if (char === "\\") {
        index += level(source.charCodeAt(index + 1)) === PUNCTUATION ? 2 : 1;
        boundary = index;
      } else if (char === "`") {
        let close = index;
        while (close < end && source[close] === "`") close++;
        const after = codeSpanEnd(close - index, close);
        index = after === -1 ? close : after;
        boundary = index;
        if (index > end) {
          resume = index;
          break;
        }
      } else if (char === "<" && /[A-Za-z/!?]/.test(source[index + 1] ?? "")) {
        if (tagEnd < index) tagEnd = source.indexOf(">", index);
        if (tagEnd === -1 || tagEnd >= end) return;
        index = boundary = tagEnd + 1;
      } else if (char === "[" || (char === "!" && source[index + 1] === "[")) {
        frames.push({ openers: [], edits: [], strayCloser: false });
        index = boundary = index + (char === "!" ? 2 : 1);
      } else if (char === "]") {
        index = boundary = index + 1;
        if (frames.length === 1) continue;
        frames.pop();
        const parent = frames[frames.length - 1];
        if (source[index] === "(") {
          let depth = 0;
          for (; index < end; index++) {
            if (source[index] === "\\") index++;
            else if (source[index] === "(") depth++;
            else if (source[index] === ")" && --depth === 0) break;
          }
          if (index >= end) return;
          index = boundary = index + 1;
        } else if (source[index] === "[") {
          const close = source.indexOf("]", index);
          if (close === -1 || close >= end) return;
          index = boundary = close + 1;
        } else if (frame.openers.length > 0 || frame.strayCloser) {
          // Literal brackets let delimiters pair across them; stop guessing.
          return;
        }
        for (const edit of frame.edits) parent.edits.push(edit);
      } else if (
        (char === "h" || char === "w") &&
        /^(?:https?:\/\/|www\.)/.test(source.slice(index, index + 8))
      ) {
        while (index < end && level(source.codePointAt(index)) !== SPACE) index++;
        boundary = index;
      } else if (char === "*") {
        let close = index;
        while (close < end && source[close] === "*") close++;
        const length = close - index;
        const before = codeBefore(source, index, start);
        const after = codeAfter(source, close, end);
        const left = level(before);
        const right = level(after);
        const canOpen = right !== SPACE && right >= left;
        const canClose = left !== SPACE && left >= right;
        const top = frame.openers[frame.openers.length - 1];
        const limit = boundary;
        index = boundary = close;
        if (!canOpen && !canClose) continue;
        // Triple runs split between strong and emphasis; leave them to MD4C.
        if (length > 2) return;
        if (length === 2 && !canClose && top?.length === 2 && isCjk(after)) {
          const edit = moveCloser(source, index - 2, start, limit);
          if (edit) {
            frame.openers.pop();
            frame.edits.push(...top.edits, edit);
            continue;
          }
        }
        // Only pending openers commit their move, once a closer pairs them.
        if (length === 2 && !canOpen && !top && isCjk(before)) {
          const edit = moveOpener(source, index - 2, end);
          if (edit) {
            frame.openers.push({ length, edits: [edit] });
            index = boundary = edit.end;
            continue;
          }
        }
        if (canClose && top) {
          // A mismatched length pairs partially in MD4C; stop guessing.
          if (top.length !== length) return;
          frame.openers.pop();
          frame.edits.push(...top.edits);
          continue;
        }
        if (canClose) frame.strayCloser = true;
        if (canOpen) frame.openers.push({ length, edits: [] });
      } else {
        index++;
      }
    }
  }
  // Unclosed brackets and unpaired openers keep their original spelling.
  for (const edit of frames[0].edits) edits.push(edit);
}

/** `月。**选` becomes `月**。选` when the result can close. */
function moveCloser(source: string, run: number, start: number, limit: number): Edit | null {
  let from = run;
  while (from > limit) {
    const code = codeBefore(source, from, limit);
    if (!crossable(code)) break;
    from -= width(code!);
  }
  const before = codeBefore(source, from, start);
  // Another `*` would lengthen the run and a backslash would escape it.
  if (from === run || level(before) === SPACE || before === 0x2a || before === 0x5c) return null;
  return { start: from, end: run + 2, text: `**${source.slice(from, run)}` };
}

/** `种**“固` becomes `种“**固` when the result can open. */
function moveOpener(source: string, run: number, end: number): Edit | null {
  let close = run + 2;
  while (close < end) {
    const code = source.codePointAt(close);
    if (!crossable(code)) break;
    close += width(code!);
  }
  const after = codeAfter(source, close, end);
  if (close === run + 2 || level(after) === SPACE || after === 0x2a) return null;
  return { start: run, end: close, text: `${source.slice(run + 2, close)}**` };
}

/**
 * Move `**` runs that CJK punctuation leaves non-flanking so MD4C pairs them.
 * Text without `**` or CJK characters is returned unchanged.
 */
export function normalizeCjkEmphasis(source: string): string {
  if (!source.includes("**") || !CJK_TEXT.test(source)) return source;
  const edits: Edit[] = [];
  for (const unit of inlineUnits(source)) adaptUnit(source, unit, edits);
  if (edits.length === 0) return source;
  edits.sort((a, b) => a.start - b.start);
  let result = "";
  let cursor = 0;
  for (const edit of edits) {
    result += source.slice(cursor, edit.start) + edit.text;
    cursor = edit.end;
  }
  return result + source.slice(cursor);
}
