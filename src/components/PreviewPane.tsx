import { useEffect, useMemo, useRef } from "react";

import type { ChapterDoc, ChapterEntry, FrontMatter, PreviewInfo } from "../types";
import { renderMarkdown, rewriteRelativeUrls } from "../lib/markdown";
import { ChevronIcon, RefreshIcon } from "./Icons";

export type PreviewMode = "render" | "mdbook";

interface PreviewPaneProps {
  mode: PreviewMode;
  chapter: ChapterEntry | null;
  doc: ChapterDoc | null;
  loadingChapter: boolean;
  workspaceTitle: string;
  chapterIndex: number;
  chapterTotal: number;
  previewInfo: PreviewInfo | null;
  previewBuilding: boolean;
  onBuildPreview: () => void;
  onPrev: () => void;
  onNext: () => void;
  /** Resolve a relative `.md` link inside a chapter to an absolute path. */
  resolveLink: (href: string) => string | null;
  onOpenLinkedFile: (path: string) => void;
  /** Absolute workspace source dir, used to serve relative images. */
  sourceRoot: string;
  /** `mdepub://localhost` (or the http equivalent). */
  sourceBase: string;
  /** Chapter paths removed from the book (source-relative). */
  excluded: Set<string>;
}

export function PreviewPane({
  mode,
  chapter,
  doc,
  loadingChapter,
  workspaceTitle,
  chapterIndex,
  chapterTotal,
  previewInfo,
  previewBuilding,
  onBuildPreview,
  onPrev,
  onNext,
  resolveLink,
  onOpenLinkedFile,
  sourceRoot,
  sourceBase,
  excluded,
}: PreviewPaneProps) {
  const articleRef = useRef<HTMLDivElement>(null);
  const isExcluded =
    chapter !== null &&
    excluded.has(
      sourceRoot && chapter.path.startsWith(`${sourceRoot}/`)
        ? chapter.path.slice(sourceRoot.length + 1)
        : chapter.path,
    );

  const html = useMemo(() => {
    if (!doc?.body) return "";
    const rendered = renderMarkdown(doc.body);
    if (!chapter || !sourceRoot) return rendered;
    const chapterDir = chapter.path.slice(0, chapter.path.lastIndexOf("/"));
    return rewriteRelativeUrls(rendered, {
      chapterDir,
      sourceRoot,
      sourceBase,
      resolveChapterLink: resolveLink,
    });
  }, [doc?.body, chapter, sourceRoot, sourceBase, resolveLink]);

  // Scroll to the top whenever the visible chapter changes.
  useEffect(() => {
    articleRef.current?.parentElement?.scrollTo({ top: 0 });
  }, [chapter?.path]);

  const handleArticleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    const target = (event.target as HTMLElement).closest("a");
    if (!target) return;
    const href = target.getAttribute("href");
    if (!href) return;

    if (href.startsWith("#")) {
      event.preventDefault();
      const id = decodeURIComponent(href.slice(1));
      articleRef.current
        ?.querySelector(`[id="${CSS.escape(id)}"]`)
        ?.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }

    // Rewritten workspace assets are served by the app itself, so leave those
    // (and any external link) for the webview to open.
    if (!/\.md$/i.test(href.split("#")[0])) {
      const resolved = resolveLink(href);
      if (!resolved) return;
      event.preventDefault();
      onOpenLinkedFile(resolved);
      return;
    }

    event.preventDefault();
    const resolved = resolveLink(href);
    if (resolved) onOpenLinkedFile(resolved);
  };

  return (
    <section className="preview">
      <div className="preview-bar">
        <div className="preview-crumbs">
          <strong>{workspaceTitle}</strong>
          {chapter && (
            <>
              <span className="sep">/</span>
              {chapter.title}
            </>
          )}
          {chapterTotal > 0 && (
            <>
              <span className="sep">·</span>
              {chapterIndex + 1} / {chapterTotal}
            </>
          )}
          {chapter && isExcluded && (
            <span className="crumb-badge" title="这一章不会出现在导出的电子书里">
              已从书籍中排除
            </span>
          )}
        </div>

        {mode === "render" && (
          <div className="titlebar-group">
            <button
              type="button"
              className="btn btn-ghost btn-icon"
              onClick={onPrev}
              disabled={chapterIndex <= 0}
              title="上一章"
            >
              <span style={{ transform: "rotate(90deg)", display: "inline-flex" }}>
                <ChevronIcon size={12} />
              </span>
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-icon"
              onClick={onNext}
              disabled={chapterIndex < 0 || chapterIndex >= chapterTotal - 1}
              title="下一章"
            >
              <span style={{ transform: "rotate(-90deg)", display: "inline-flex" }}>
                <ChevronIcon size={12} />
              </span>
            </button>
          </div>
        )}

        {mode === "mdbook" && (
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={onBuildPreview}
            disabled={previewBuilding}
          >
            <RefreshIcon size={12} />
            {previewBuilding ? "构建中…" : previewInfo ? "重新构建" : "构建预览"}
          </button>
        )}
      </div>

      <div className="preview-stage">
        {mode === "render" ? (
          <div className="preview-scroll">
            {loadingChapter ? (
              <div className="preview-placeholder">正在读取章节…</div>
            ) : chapter ? (
              <div className="preview-article" ref={articleRef} onClick={handleArticleClick}>
                {doc && <FrontMatterStrip frontMatter={doc.frontMatter} />}
                <div dangerouslySetInnerHTML={{ __html: html }} />
              </div>
            ) : (
              <div className="preview-placeholder">
                <div>
                  <div className="big">在左侧选择一个章节</div>
                  <div>也可以直接拖拽 Markdown 文件到窗口中</div>
                </div>
              </div>
            )}
          </div>
        ) : previewInfo ? (
          <iframe
            className="preview-frame"
            title="mdBook 预览"
            src={previewInfo.indexUrl}
            sandbox="allow-scripts allow-same-origin allow-popups allow-forms"
          />
        ) : (
          <div className="preview-placeholder">
            <div>
              <div className="big">尚未构建 mdBook 预览</div>
              <div style={{ marginBottom: 16 }}>
                这里会显示由 mdBook 真正生成的整书 HTML（带侧边栏、主题与搜索）。
              </div>
              <button
                type="button"
                className="btn btn-primary"
                onClick={onBuildPreview}
                disabled={previewBuilding}
              >
                <RefreshIcon size={13} />
                {previewBuilding ? "构建中…" : "构建整书预览"}
              </button>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

/**
 * The document's YAML/TOML header, rendered as metadata rather than body text.
 *
 * The block is stripped from the Markdown before rendering, so this is the only
 * place a reader would ever see a date or tags.
 */
function FrontMatterStrip({ frontMatter }: { frontMatter: FrontMatter }) {
  if (!frontMatter.format) return null;

  const chips = frontMatter.tags.length
    ? frontMatter.tags
    : frontMatter.fields
        .filter((field) => field.key.toLowerCase() === "tags")
        .flatMap((field) => field.value.replace(/^\[|\]$/g, "").split(","))
        .map((value) => value.trim())
        .filter(Boolean);

  const hasMeta = Boolean(frontMatter.date) || chips.length > 0 || frontMatter.draft;
  if (!hasMeta && !frontMatter.description) return null;

  return (
    <aside className="frontmatter">
      {hasMeta && (
        <div className="frontmatter-meta">
          {frontMatter.date && <span className="frontmatter-date">{frontMatter.date}</span>}
          {frontMatter.draft && <span className="chip chip-warn">草稿</span>}
          {chips.map((tag) => (
            <span className="chip" key={tag}>
              {tag}
            </span>
          ))}
        </div>
      )}
      {frontMatter.description && (
        <p className="frontmatter-description">{frontMatter.description}</p>
      )}
    </aside>
  );
}

export function useChapterIndex(chapters: ChapterEntry[], chapter: ChapterEntry | null) {
  return useMemo(() => {
    if (!chapter) return -1;
    return chapters.findIndex((item) => item.path === chapter.path);
  }, [chapters, chapter]);
}
