import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import type { BookMeta, ChapterEntry, Workspace } from "../types";
import {
  reorderChapters,
  validDropPosition,
  type DropPosition,
} from "../lib/chapters";
import { FileTree, toRelative } from "./FileTree";
import { BookIcon, CloseIcon, FolderOpenIcon } from "./Icons";
import { Section } from "./Section";
import { shortenPath } from "../lib/api";

interface SidebarProps {
  workspace: Workspace | null;
  meta: BookMeta;
  activeChapterPath: string | null;
  /** Chapters in display order (the outline as arranged by dragging). */
  chapters: ChapterEntry[];
  onMetaChange: (patch: Partial<BookMeta>) => void;
  onSelectFile: (path: string, isMarkdown: boolean) => void;
  /** Chapter paths removed from the book (source-relative). */
  excluded: Set<string>;
  onToggleExclude: (rel: string) => void;
  onRestoreAll: () => void;
  /** New chapter paths order after a drag in the outline. */
  onReorder: (order: string[]) => void;
  /** Insert a contents page at the front of the exported book. */
  tocPage: boolean;
  tocDepth: number;
  onTocChange: (patch: { tocPage?: boolean; tocDepth?: number }) => void;
}

const languages = ["zh-CN", "zh-TW", "en", "ja", "ko", "fr", "de", "es", "ru"];

function kindLabel(kind: Workspace["kind"]): string {
  switch (kind) {
    case "singleFile":
      return "单个 Markdown 文件";
    case "mdBook":
      return "mdBook 项目";
    default:
      return "Markdown 文件夹";
  }
}

export function Sidebar({
  workspace,
  meta,
  activeChapterPath,
  chapters,
  onMetaChange,
  onSelectFile,
  excluded,
  onToggleExclude,
  onRestoreAll,
  onReorder,
  tocPage,
  tocDepth,
  onTocChange,
}: SidebarProps) {
  const [authorDraft, setAuthorDraft] = useState("");
  // Outline drag state (kept local; the resulting order goes to App).
  //
  // Dragging is implemented with pointer events rather than HTML5 drag &
  // drop: Tauri's own native drag-drop handling (used for "drop a file onto
  // the window to open it") swallows HTML5 dragstart entirely on macOS, so
  // rows would never drag. Pointer events are independent of that machinery.
  const [dragPath, setDragPath] = useState<string | null>(null);
  const [dropHint, setDropHint] = useState<{ path: string; position: DropPosition } | null>(null);
  const dragRef = useRef<{ path: string; startX: number; startY: number; active: boolean } | null>(
    null,
  );
  const treeRef = useRef<HTMLDivElement>(null);
  // Latest values for the window-level pointer listeners, which are attached
  // once and must not go stale.
  const latest = useRef({ chapters, workspace, onReorder });
  latest.current = { chapters, workspace, onReorder };

  const addAuthor = () => {
    const value = authorDraft.trim();
    if (!value) return;
    onMetaChange({ authors: [...meta.authors, value] });
    setAuthorDraft("");
  };

  const removeAuthor = (index: number) => {
    onMetaChange({ authors: meta.authors.filter((_, i) => i !== index) });
  };

  const clearDrag = () => {
    dragRef.current = null;
    setDragPath(null);
    setDropHint(null);
  };

  const handlePointerDown = (event: React.PointerEvent, chapter: ChapterEntry) => {
    if (event.button !== 0 || chapters.length <= 1) return;
    dragRef.current = {
      path: chapter.path,
      startX: event.clientX,
      startY: event.clientY,
      active: false,
    };
  };

  useEffect(() => {
    /** Row + insertion side under `clientY`, or null when not droppable. */
    const hintAt = (dragPath: string, clientY: number) => {
      const rows = Array.from(
        treeRef.current?.querySelectorAll<HTMLElement>(".tree-row-plain") ?? [],
      );
      for (const row of rows) {
        const rect = row.getBoundingClientRect();
        if (clientY < rect.top || clientY > rect.bottom) continue;
        const target = row.dataset.path;
        if (!target) return null;
        const after = clientY > rect.top + rect.height / 2;
        return {
          target,
          position: validDropPosition(latest.current.chapters, dragPath, target, after),
        };
      }
      return null;
    };

    const onPointerMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      if (!drag.active) {
        // A small threshold keeps plain clicks and trackpad jitter from
        // turning into accidental drags.
        if (
          Math.abs(event.clientY - drag.startY) < 4 &&
          Math.abs(event.clientX - drag.startX) < 4
        ) {
          return;
        }
        drag.active = true;
        setDragPath(drag.path);
      }
      const hint = hintAt(drag.path, event.clientY);
      setDropHint(hint?.position ? { path: hint.target, position: hint.position } : null);
    };

    const onPointerUp = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag?.active) {
        dragRef.current = null;
        return;
      }
      const { workspace: ws, chapters: list, onReorder: reorder } = latest.current;
      const hint = hintAt(drag.path, event.clientY);
      if (ws && hint?.position) {
        const order = reorderChapters(
          list,
          (path) => toRelative(ws.srcDir, path),
          drag.path,
          hint.target,
          hint.position,
        );
        if (order) reorder(order);
      }
      clearDrag();
    };

    const onPointerCancel = () => clearDrag();

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerCancel);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerCancel);
    };
  }, []);

  const pickCover = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "svg"] }],
    });
    if (typeof selected === "string") {
      onMetaChange({ cover: selected });
    }
  };

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        {workspace ? (
          <div className="source-card">
            <div className="source-icon">
              <BookIcon size={13} />
            </div>
            <div className="source-text">
              <div className="source-title" title={workspace.title}>
                {workspace.title}
              </div>
              <div className="source-path" title={workspace.root}>
                {shortenPath(workspace.root, 40)}
              </div>
            </div>
          </div>
        ) : (
          <div className="source-card">
            <div className="source-icon">
              <FolderOpenIcon size={13} />
            </div>
            <div className="source-text">
              <div className="source-title">未打开工作区</div>
              <div className="source-path">打开文件或文件夹开始</div>
            </div>
          </div>
        )}
      </div>

      <div className="sidebar-scroll">
        {workspace && (
          <>
            <Section
              title="章节大纲"
              count={
                excluded.size > 0
                  ? `${chapters.length - excluded.size} / ${chapters.length} 章`
                  : `${chapters.length} 章`
              }
              defaultOpen
            >
              {excluded.size > 0 && (
                <div className="excluded-bar">
                  <span>已排除 {excluded.size} 章</span>
                  <button type="button" className="btn btn-ghost btn-sm" onClick={onRestoreAll}>
                    全部恢复
                  </button>
                </div>
              )}
              {chapters.length > 1 && (
                <div className="field-hint" style={{ marginBottom: 6 }}>
                  上下拖动章节即可调整顺序（导出与预览会跟随，不改动文件）。
                </div>
              )}
              <div className="tree" ref={treeRef}>
                {chapters.map((chapter) => {
                  const rel = toRelative(workspace.srcDir, chapter.path);
                  const isExcluded = excluded.has(rel);
                  const isDragging = dragPath === chapter.path;
                  const dropHere =
                    dropHint?.path === chapter.path ? dropHint.position : null;
                  return (
                    <button
                      key={chapter.id}
                      type="button"
                      data-path={chapter.path}
                      onPointerDown={(event) => handlePointerDown(event, chapter)}
                      className={`tree-row tree-row-plain${
                        chapter.path === activeChapterPath ? " is-active" : ""
                      }${isExcluded ? " is-excluded" : ""}${
                        isDragging ? " is-dragging" : ""
                      }${dropHere === "before" ? " is-drop-above" : ""}${
                        dropHere === "after" ? " is-drop-below" : ""
                      }`}
                      style={{ paddingLeft: 8 + (chapter.level - 1) * 12 }}
                      onClick={() => onSelectFile(chapter.path, true)}
                      title={
                        isExcluded
                          ? `${chapter.path}（已排除）`
                          : chapters.length > 1
                            ? `${chapter.path}（拖动可调整顺序）`
                            : chapter.path
                      }
                    >
                      <span className="tree-label">{chapter.title}</span>
                      {isExcluded && <span className="tree-badge tree-badge-warn">已排除</span>}
                    </button>
                  );
                })}
              </div>
            </Section>

            <Section title="工作区文件" count={kindLabel(workspace.kind)} defaultOpen={false}>
              <FileTree
                nodes={workspace.tree}
                activePath={activeChapterPath}
                srcDir={workspace.srcDir}
                excluded={excluded}
                onSelect={onSelectFile}
                onToggleExclude={onToggleExclude}
              />
            </Section>

            <Section title="书籍信息" defaultOpen>
              <div className="field">
                <label className="field-label">书名</label>
                <input
                  value={meta.title}
                  placeholder={workspace.title}
                  onChange={(event) => onMetaChange({ title: event.target.value })}
                />
              </div>

              <div className="field">
                <label className="field-label">作者</label>
                <div className="path-picker">
                  <input
                    value={authorDraft}
                    placeholder="回车添加作者"
                    onChange={(event) => setAuthorDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        addAuthor();
                      }
                    }}
                  />
                  <button type="button" className="btn btn-ghost btn-sm" onClick={addAuthor}>
                    添加
                  </button>
                </div>
                {meta.authors.length > 0 && (
                  <div className="chip-row">
                    {meta.authors.map((author, index) => (
                      <span className="chip" key={`${author}-${index}`}>
                        {author}
                        <button type="button" onClick={() => removeAuthor(index)} title="移除">
                          <CloseIcon size={10} />
                        </button>
                      </span>
                    ))}
                  </div>
                )}
              </div>

              <div className="field-row">
                <div className="field">
                  <label className="field-label">语言</label>
                  <select
                    value={meta.language}
                    onChange={(event) => onMetaChange({ language: event.target.value })}
                  >
                    {languages.map((code) => (
                      <option key={code} value={code}>
                        {code}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label className="field-label">出版方</label>
                  <input
                    value={meta.publisher}
                    placeholder="可选"
                    onChange={(event) => onMetaChange({ publisher: event.target.value })}
                  />
                </div>
              </div>

              <div className="field">
                <label className="field-label">简介</label>
                <textarea
                  rows={3}
                  value={meta.description}
                  placeholder="会写入 EPUB 的 dc:description"
                  onChange={(event) => onMetaChange({ description: event.target.value })}
                />
              </div>

              <div className="field">
                <label className="field-label">封面图片</label>
                <div className="path-picker">
                  <input
                    value={meta.cover ?? ""}
                    placeholder="未选择"
                    onChange={(event) =>
                      onMetaChange({ cover: event.target.value || null })
                    }
                  />
                  <button type="button" className="btn btn-ghost btn-sm" onClick={pickCover}>
                    选择
                  </button>
                </div>
              </div>

              <div className="field-divider" />

              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={tocPage}
                  onChange={(event) => onTocChange({ tocPage: event.target.checked })}
                />
                生成目录页
              </label>
              <div className="field-hint" style={{ marginTop: 0, marginBottom: 8 }}>
                在封面之后插入一页可见的目录（EPUB 与 PDF 都会生成）。
              </div>

              {tocPage && (
                <div className="field">
                  <label className="field-label">目录层级</label>
                  <select
                    value={tocDepth}
                    onChange={(event) =>
                      onTocChange({ tocDepth: Number(event.target.value) })
                    }
                  >
                    <option value={1}>只列章标题</option>
                    <option value={2}>列到二级标题</option>
                    <option value={3}>列到三级标题</option>
                  </select>
                </div>
              )}
            </Section>
          </>
        )}
      </div>
    </aside>
  );
}
