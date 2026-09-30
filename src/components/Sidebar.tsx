import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import type { BookMeta, Workspace } from "../types";
import { FileTree, toRelative } from "./FileTree";
import { BookIcon, CloseIcon, FolderOpenIcon } from "./Icons";
import { Section } from "./Section";
import { shortenPath } from "../lib/api";

interface SidebarProps {
  workspace: Workspace | null;
  meta: BookMeta;
  activeChapterPath: string | null;
  onMetaChange: (patch: Partial<BookMeta>) => void;
  onSelectFile: (path: string, isMarkdown: boolean) => void;
  /** Chapter paths removed from the book (source-relative). */
  excluded: Set<string>;
  onToggleExclude: (rel: string) => void;
  onRestoreAll: () => void;
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
  onMetaChange,
  onSelectFile,
  excluded,
  onToggleExclude,
  onRestoreAll,
  tocPage,
  tocDepth,
  onTocChange,
}: SidebarProps) {
  const [authorDraft, setAuthorDraft] = useState("");

  const addAuthor = () => {
    const value = authorDraft.trim();
    if (!value) return;
    onMetaChange({ authors: [...meta.authors, value] });
    setAuthorDraft("");
  };

  const removeAuthor = (index: number) => {
    onMetaChange({ authors: meta.authors.filter((_, i) => i !== index) });
  };

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
                  ? `${workspace.chapters.length - excluded.size} / ${workspace.chapters.length} 章`
                  : `${workspace.chapters.length} 章`
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
              <div className="tree">
                {workspace.chapters.map((chapter) => {
                  const rel = toRelative(workspace.srcDir, chapter.path);
                  const isExcluded = excluded.has(rel);
                  return (
                    <button
                      key={chapter.id}
                      type="button"
                      className={`tree-row tree-row-plain${
                        chapter.path === activeChapterPath ? " is-active" : ""
                      }${isExcluded ? " is-excluded" : ""}`}
                      style={{ paddingLeft: 8 + (chapter.level - 1) * 12 }}
                      onClick={() => onSelectFile(chapter.path, true)}
                      title={isExcluded ? `${chapter.path}（已排除）` : chapter.path}
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
