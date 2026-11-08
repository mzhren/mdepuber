import { useEffect, useMemo, useRef, type ReactNode } from "react";

import { renderMarkdown, rewriteRelativeUrls } from "../lib/markdown";

interface RenderedBodyProps {
  /** Markdown body (front matter already stripped). */
  body: string;
  /** Absolute path of the chapter — anchors URL rewriting and scroll resets. */
  chapterPath: string;
  /** Absolute workspace source dir, used to serve relative images. */
  sourceRoot: string;
  /** `mdepub://localhost` (or the http equivalent). */
  sourceBase: string;
  /** Resolve a relative `.md` link inside a chapter to an absolute path. */
  resolveLink: (href: string) => string | null;
  onOpenLinkedFile: (path: string) => void;
  /** Optional node rendered above the body (e.g. the front matter strip). */
  header?: ReactNode;
}

/**
 * A rendered Markdown article: syntax-highlighted, with workspace-relative
 * image/link URLs rewritten to the `mdepub://` asset route, and in-document
 * links wired to chapter navigation.
 */
export function RenderedBody({
  body,
  chapterPath,
  sourceRoot,
  sourceBase,
  resolveLink,
  onOpenLinkedFile,
  header,
}: RenderedBodyProps) {
  const articleRef = useRef<HTMLDivElement>(null);

  const html = useMemo(() => {
    if (!body) return "";
    const rendered = renderMarkdown(body);
    if (!chapterPath || !sourceRoot) return rendered;
    const chapterDir = chapterPath.slice(0, chapterPath.lastIndexOf("/"));
    return rewriteRelativeUrls(rendered, {
      chapterDir,
      sourceRoot,
      sourceBase,
      resolveChapterLink: resolveLink,
    });
  }, [body, chapterPath, sourceRoot, sourceBase, resolveLink]);

  // Scroll to the top whenever the visible chapter changes (but never while
  // the same chapter is being re-rendered, e.g. from the live editor preview).
  useEffect(() => {
    articleRef.current?.parentElement?.scrollTo({ top: 0 });
  }, [chapterPath]);

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
    <div className="preview-article" ref={articleRef} onClick={handleArticleClick}>
      {header}
      <div dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  );
}
