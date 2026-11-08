/** Markdown rendering for the in-app preview pane. */

import MarkdownIt from "markdown-it";
import hljs from "highlight.js/lib/common";

const md = new MarkdownIt({
  html: true,
  linkify: true,
  typographer: false,
  breaks: false,
  highlight(code, language) {
    if (language && hljs.getLanguage(language)) {
      try {
        return hljs.highlight(code, { language, ignoreIllegals: true }).value;
      } catch {
        /* fall through to auto-detection */
      }
    }
    try {
      return hljs.highlightAuto(code).value;
    } catch {
      return "";
    }
  },
});

/**
 * GitHub-style admonitions (`> [!NOTE]`) are not part of markdown-it's core.
 * mdBook renders them via pulldown-cmark's GFM extensions, so the preview has
 * to match — otherwise the raw `[!NOTE]` marker leaks into the paragraph.
 */
const ALERT_LABELS: Record<string, string> = {
  NOTE: "注意",
  TIP: "提示",
  IMPORTANT: "重要",
  WARNING: "警告",
  CAUTION: "小心",
};

md.core.ruler.push("mdepub_alerts", (state) => {
  const tokens = state.tokens;
  for (let i = 0; i < tokens.length; i += 1) {
    if (tokens[i].type !== "blockquote_open") continue;

    const paragraphOpen = tokens[i + 1];
    const inline = tokens[i + 2];
    if (!paragraphOpen || paragraphOpen.type !== "paragraph_open") continue;
    if (!inline || inline.type !== "inline") continue;

    const match = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*/.exec(inline.content);
    if (!match) continue;

    const kind = match[1];
    const label = ALERT_LABELS[kind];
    const className = `markdown-alert markdown-alert-${kind.toLowerCase()}`;
    tokens[i].attrSet("class", className);

    // Drop the marker itself from the inline token's children.
    let remaining = match[0].length;
    const children = inline.children ?? [];
    let cursor = 0;
    while (remaining > 0 && cursor < children.length) {
      const child = children[cursor];
      if (child.type !== "text") break;
      const take = Math.min(remaining, child.content.length);
      child.content = child.content.slice(take);
      remaining -= take;
      if (child.content.length === 0) children.splice(cursor, 1);
      else cursor += 1;
    }
    // A soft break left dangling in front of the text looks like an empty line.
    while (children.length && (children[0].type === "softbreak" || children[0].type === "hardbreak")) {
      children.shift();
    }

    const titleOpen = new state.Token("paragraph_open", "p", 1);
    titleOpen.attrSet("class", "markdown-alert-title");
    const titleInline = new state.Token("inline", "", 0);
    const titleText = new state.Token("text", "", 0);
    titleText.content = label;
    titleInline.children = [titleText];
    titleInline.content = label;
    const titleClose = new state.Token("paragraph_close", "p", -1);
    tokens.splice(i + 1, 0, titleOpen, titleInline, titleClose);
  }
  return true;
});

// Tables and task lists are part of CommonMark+GFM; markdown-it has them on by
// default, but the footnote-style anchor ids need a nudge for stability.
const defaultHeadingOpen =
  md.renderer.rules.heading_open ??
  ((tokens, idx, options, _env, self) => self.renderToken(tokens, idx, options));

md.renderer.rules.heading_open = (tokens, idx, options, env, self) => {
  const inline = tokens[idx + 1];
  const text = inline?.content ?? "";
  const slug = slugify(text);
  const existing = tokens[idx].attrGet("id");
  if (!existing && slug) {
    tokens[idx].attrSet("id", uniqueSlug(slug, env as RenderEnv));
  }
  return defaultHeadingOpen(tokens, idx, options, env, self);
};

interface RenderEnv {
  slugs?: Map<string, number>;
}

function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .trim()
    .replace(/\s+/g, "-");
}

function uniqueSlug(slug: string, env: RenderEnv): string {
  if (!env || typeof env !== "object") return slug;
  env.slugs ??= new Map();
  const seen = env.slugs.get(slug) ?? 0;
  env.slugs.set(slug, seen + 1);
  return seen === 0 ? slug : `${slug}-${seen}`;
}

export function renderMarkdown(source: string): string {
  const env: RenderEnv = { slugs: new Map() };
  return md.render(source, env);
}

const ABSOLUTE_URL = /^[a-z][a-z0-9+.-]*:/i;

function isExternal(url: string): boolean {
  return ABSOLUTE_URL.test(url) || url.startsWith("//") || url.startsWith("#");
}

/** Join a relative URL onto an absolute POSIX directory, resolving `..`. */
export function resolveRelative(baseDir: string, relative: string): string | null {
  const parts = baseDir.split("/");
  for (const segment of relative.split("/")) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      if (parts.length === 0) return null;
      parts.pop();
    } else {
      parts.push(segment);
    }
  }
  return parts.join("/");
}

export interface RewriteTargets {
  /** Absolute directory of the chapter being rendered. */
  chapterDir: string;
  /** Absolute workspace source directory — the only place we will serve from. */
  sourceRoot: string;
  /** URL prefix of the `mdepub://` protocol, e.g. `mdepub://localhost`. */
  sourceBase: string;
  /** Resolves a relative `.md` link to a chapter path, if it is one. */
  resolveChapterLink?: (url: string) => string | null;
}

/**
 * Point relative `src`/`href` at the workspace asset route.
 *
 * markdown-it emits the raw relative paths from the document, which the webview
 * would resolve against the app origin and fail to load — that is why images in
 * a chapter preview used to show up as a broken placeholder.
 */
export function rewriteRelativeUrls(html: string, targets: RewriteTargets): string {
  const { chapterDir, sourceRoot, sourceBase } = targets;
  const root = sourceRoot.replace(/\/+$/, "");
  if (!root) return html;

  const doc = new DOMParser().parseFromString(html, "text/html");

  const assetUrl = (raw: string): string | null => {
    if (!raw || isExternal(raw)) return null;
    const clean = decodeURIComponent(raw.split("#")[0].split("?")[0]);
    if (!clean || clean.startsWith("/")) return null;
    const absolute = resolveRelative(chapterDir, clean);
    if (!absolute || !absolute.startsWith(`${root}/`)) return null;
    return `${sourceBase}/@source${encodeURI(absolute)}`;
  };

  doc.querySelectorAll("img[src]").forEach((el) => {
    const rewritten = assetUrl(el.getAttribute("src") ?? "");
    if (rewritten) el.setAttribute("src", rewritten);
  });

  doc.querySelectorAll("source[srcset]").forEach((el) => {
    const srcset = el.getAttribute("srcset") ?? "";
    const next = srcset
      .split(",")
      .map((entry) => {
        const [url, ...rest] = entry.trim().split(/\s+/);
        const rewritten = assetUrl(url);
        return [rewritten ?? url, ...rest].join(" ");
      })
      .join(", ");
    el.setAttribute("srcset", next);
  });

  // Non-Markdown links (attachments, images) are served too; `.md` links stay
  // with the app so clicking one can switch chapters.
  doc.querySelectorAll("a[href]").forEach((el) => {
    const href = el.getAttribute("href") ?? "";
    if (!href || isExternal(href)) return;
    if (targets.resolveChapterLink?.(href)) return;
    const rewritten = assetUrl(href);
    if (rewritten) el.setAttribute("href", rewritten);
  });

  // Video/audio sources and object data follow the same rule.
  doc.querySelectorAll("video[src], audio[src], source[src]").forEach((el) => {
    const rewritten = assetUrl(el.getAttribute("src") ?? "");
    if (rewritten) el.setAttribute("src", rewritten);
  });

  return doc.body.innerHTML;
}

/**
 * Split a leading YAML (`---`) / TOML (`+++`) front matter block off the raw
 * file text, mirroring `mdepub-core::frontmatter::locate` — the delimiter must
 * be the very first line and a matching closer must exist, otherwise the whole
 * document is body (e.g. one that merely opens with a horizontal rule).
 */
export function splitFrontMatter(raw: string): { format: "" | "yaml" | "toml"; body: string } {
  const lines = raw.split(/^/m);
  const trimmedEnd = (line: string) => line.replace(/[ \t\r\n]+$/, "");
  const opener = trimmedEnd(lines[0] ?? "");
  const format = opener === "---" ? ("yaml" as const) : opener === "+++" ? ("toml" as const) : ("");
  if (!format) return { format: "", body: raw };

  const closers = format === "yaml" ? ["---", "..."] : ["+++"];
  for (let i = 1; i < lines.length; i += 1) {
    if (closers.includes(trimmedEnd(lines[i]))) {
      return { format, body: lines.slice(i + 1).join("") };
    }
  }
  return { format: "", body: raw };
}

/** Very small word count that matches the Rust implementation closely enough. */
export function countWords(text: string): number {
  let count = 0;
  let inWord = false;
  for (const char of text) {
    if (/\s/.test(char)) {
      inWord = false;
      continue;
    }
    if (/[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]/.test(char)) {
      count += 1;
      inWord = false;
    } else if (/[\p{L}\p{N}]/u.test(char)) {
      if (!inWord) {
        count += 1;
        inWord = true;
      }
    }
  }
  return count;
}

export { md };
