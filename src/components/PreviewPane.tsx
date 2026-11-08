import { useMemo } from "react";

import type { ChapterDoc, ChapterEntry, FrontMatter, PreviewInfo } from "../types";
import { ChevronIcon, RefreshIcon } from "./Icons";
import { RenderedBody } from "./RenderedBody";

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
  const isExcluded =
    chapter !== null &&
    excluded.has(
      sourceRoot && chapter.path.startsWith(`${sourceRoot}/`)
        ? chapter.path.slice(sourceRoot.length + 1)
        : chapter.path,
    );

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
              <RenderedBody
                body={doc?.body ?? ""}
                chapterPath={chapter.path}
                sourceRoot={sourceRoot}
                sourceBase={sourceBase}
                resolveLink={resolveLink}
                onOpenLinkedFile={onOpenLinkedFile}
                header={doc && <FrontMatterStrip frontMatter={doc.frontMatter} />}
              />
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
