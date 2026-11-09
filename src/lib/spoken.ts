/**
 * 智能剪口播 — text-level analysis of a spoken-word script.
 *
 * Purely textual: it flags filler interjections (嗯/呃…), duplicated or
 * repeated lines (口误/重复) and text-shaped dead air (runs of ellipses,
 * blank-line gaps). Real pauses/breaths live on an audio timeline that plain
 * text does not carry — these are the closest textual approximations.
 *
 * The rules deliberately favour precision over recall: a missed filler is
 * cheap, a false highlight on normal prose is annoying.
 */

export type SpokenKind = "filler" | "repeat" | "pause";

export interface SpokenFinding {
  /** Character offsets into the raw document text. */
  from: number;
  to: number;
  kind: SpokenKind;
  text: string;
  /** 1-based line number of `from`. */
  line: number;
}

/** Kind labels used by the UI. */
export const SPOKEN_KIND_LABEL: Record<SpokenKind, string> = {
  filler: "语气词",
  repeat: "重复",
  pause: "留白",
};

/** Hard cap so a pathological document cannot flood the panel. */
const MAX_FINDINGS = 500;

/** Interjections that are almost certainly fillers whenever they start a token. */
const STRONG_FILLERS = /(?<![一-鿿])[呃嗯]{1,4}/gu;
/** Interjections that also occur inside real words — require word boundaries. */
const WEAK_FILLERS = /(?<![一-鿿])[啊哦噢哎唉诶唔呣]+(?![一-鿿])/gu;
/** Doubled connectives — “然后然后”, “这个这个”… */
const DOUBLED_FILLERS = /(?<![一-鿿])(这个|那个|然后|就是|所以|那么|其实|反正)\1/gu;
/** Demonstratives dangling before punctuation — the spoken “那个……”。 */
const DANGLING_DEMONSTRATIVES = /(?<![一-鿿])(?:那个|这个)(?=[，。！？…—、\s])/gu;

/** Text shapes that stand for dead air / long pauses. */
const PAUSE_PATTERNS: RegExp[] = [
  /…{2,}/gu, // ≥“……” — the standard script pause mark
  /。{3,}/gu, // 。。。
  /\.{6,}/gu, // ......
  /\n(?:[ \t]*\n){2,}/gu, // 2+ consecutive blank lines
];

/** Strip everything that could differ between two spoken takes of one line. */
function normalizeSentence(raw: string): string {
  return raw.replace(/[\s，、；：:,.!?！？"'“”‘’（）()【】\[\]…—·<>《》]/g, "");
}

/** Collect non-overlapping regex matches. */
function collectMatches(text: string, re: RegExp, kind: SpokenKind, out: SpokenFinding[]): void {
  re.lastIndex = 0;
  for (let m = re.exec(text); m !== null; m = re.exec(text)) {
    if (m[0].length === 0) {
      re.lastIndex += 1; // safety against zero-width matches
      continue;
    }
    out.push({ from: m.index, to: m.index + m[0].length, kind, text: m[0], line: 0 });
  }
}

/**
 * Find every “redundant” span worth marking for a spoken-word cut.
 * Returned findings are sorted by position and never overlap.
 */
export function analyzeSpoken(text: string): SpokenFinding[] {
  const candidates: SpokenFinding[] = [];

  collectMatches(text, STRONG_FILLERS, "filler", candidates);
  collectMatches(text, WEAK_FILLERS, "filler", candidates);
  collectMatches(text, DOUBLED_FILLERS, "filler", candidates);
  collectMatches(text, DANGLING_DEMONSTRATIVES, "filler", candidates);
  for (const pattern of PAUSE_PATTERNS) {
    collectMatches(text, pattern, "pause", candidates);
  }

  // --- 口误与重复：consecutive identical sentences merge into one finding,
  // near repeats (same sentence again within a short window) mark the copy.
  const sentenceRe = /[^。！？!?\n]+/g;
  const sentences: { from: number; to: number; norm: string }[] = [];
  for (let m = sentenceRe.exec(text); m !== null; m = sentenceRe.exec(text)) {
    const raw = m[0];
    const norm = normalizeSentence(raw);
    if (norm.length >= 4) {
      sentences.push({ from: m.index, to: m.index + raw.length, norm });
    }
  }
  let i = 0;
  while (i < sentences.length) {
    let j = i;
    while (j + 1 < sentences.length && sentences[j + 1].norm === sentences[i].norm) {
      j += 1;
    }
    if (j > i) {
      // A run of the same sentence: flag the whole run as one repetition.
      candidates.push({
        from: sentences[i].from,
        to: sentences[j].to,
        kind: "repeat",
        text: text.slice(sentences[i].from, sentences[j].to),
        line: 0,
      });
      i = j + 1;
      continue;
    }
    // Near repeat: the same sentence re-appears within ~300 chars.
    const current = sentences[i];
    if (current.norm.length >= 6) {
      for (let k = i + 1; k < sentences.length; k += 1) {
        if (sentences[k].from - current.to > 300) break;
        if (sentences[k].norm === current.norm) {
          candidates.push({
            from: sentences[k].from,
            to: sentences[k].to,
            kind: "repeat",
            text: text.slice(sentences[k].from, sentences[k].to),
            line: 0,
          });
          break; // one flag per repeat is enough
        }
      }
    }
    i += 1;
  }

  // Sort, then drop overlaps (first-wins) so CodeMirror range sets stay valid.
  candidates.sort((a, b) => a.from - b.from || a.to - b.to);
  const findings: SpokenFinding[] = [];
  let lastTo = -1;
  for (const finding of candidates) {
    if (finding.from < lastTo) continue;
    findings.push(finding);
    lastTo = finding.to;
    if (findings.length >= MAX_FINDINGS) break;
  }

  // Line numbers in one pass.
  let line = 1;
  let cursor = 0;
  for (const finding of findings) {
    while (cursor < finding.from) {
      if (text[cursor] === "\n") line += 1;
      cursor += 1;
    }
    finding.line = line;
  }
  return findings;
}
