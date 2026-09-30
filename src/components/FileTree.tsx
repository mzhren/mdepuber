import { useMemo, useState } from "react";

import type { FsNode } from "../types";
import { CloseIcon, FileIcon, FolderIcon, ImageIcon, RestoreIcon } from "./Icons";

interface FileTreeProps {
  nodes: FsNode[];
  activePath: string | null;
  /** Absolute workspace source directory, used to derive the exclusion key. */
  srcDir: string;
  /** Chapter paths (source-relative) the user removed from the book. */
  excluded: Set<string>;
  onSelect: (path: string, isMarkdown: boolean) => void;
  onToggleExclude: (rel: string) => void;
}

/** Chapter identity used everywhere: path relative to the book source dir. */
export function toRelative(srcDir: string, absolute: string): string {
  const prefix = srcDir.endsWith("/") ? srcDir : `${srcDir}/`;
  return absolute.startsWith(prefix) ? absolute.slice(prefix.length) : absolute;
}

function glyph(node: FsNode) {
  if (node.isDir) return <FolderIcon size={13} />;
  const ext = node.name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "svg", "webp", "avif"].includes(ext)) {
    return <ImageIcon size={13} />;
  }
  return <FileIcon size={13} />;
}

interface RowProps extends Omit<FileTreeProps, "nodes"> {
  node: FsNode;
  depth: number;
}

function Row({
  node,
  depth,
  activePath,
  srcDir,
  excluded,
  onSelect,
  onToggleExclude,
}: RowProps) {
  const [open, setOpen] = useState(depth < 2);

  if (node.isDir) {
    const children = node.children ?? [];
    return (
      <div>
        <button
          type="button"
          className="tree-row tree-row-plain"
          style={{ paddingLeft: 8 + depth * 12 }}
          onClick={() => setOpen((value) => !value)}
          title={node.path}
        >
          <span className="tree-twisty">{open ? "▾" : "▸"}</span>
          <span className="tree-glyph">{glyph(node)}</span>
          <span className="tree-label">{node.name}</span>
          <span className="tree-badge">{children.length}</span>
        </button>
        {open &&
          children.map((child) => (
            <Row
              key={child.path}
              node={child}
              depth={depth + 1}
              activePath={activePath}
              srcDir={srcDir}
              excluded={excluded}
              onSelect={onSelect}
              onToggleExclude={onToggleExclude}
            />
          ))}
      </div>
    );
  }

  const rel = toRelative(srcDir, node.path);
  // Only chapters can be excluded — removing an image would just break links.
  const canExclude = node.isMarkdown && node.inBook;
  const isExcluded = canExclude && excluded.has(rel);
  const isActive = node.path === activePath;

  return (
    <div
      className={`tree-row${isActive ? " is-active" : ""}${isExcluded ? " is-excluded" : ""}${
        node.inBook ? "" : " is-dim"
      }`}
      style={{ paddingLeft: 8 + depth * 12 + 12 }}
    >
      <button
        type="button"
        className="tree-row-main"
        onClick={() => onSelect(node.path, node.isMarkdown)}
        title={isExcluded ? `${node.path}（已从书籍中排除）` : node.path}
      >
        <span className="tree-glyph">{glyph(node)}</span>
        <span className="tree-label">{node.name}</span>
        {node.inBook && !isExcluded && <span className="tree-badge">书</span>}
        {isExcluded && <span className="tree-badge tree-badge-warn">已排除</span>}
      </button>
      {canExclude && (
        <button
          type="button"
          className="tree-row-action"
          onClick={() => onToggleExclude(rel)}
          title={isExcluded ? "恢复到书籍中" : "从书籍中排除（不会删除磁盘文件）"}
          aria-label={isExcluded ? "恢复到书籍中" : "从书籍中排除"}
        >
          {isExcluded ? <RestoreIcon size={12} /> : <CloseIcon size={12} />}
        </button>
      )}
    </div>
  );
}

export function FileTree({
  nodes,
  activePath,
  srcDir,
  excluded,
  onSelect,
  onToggleExclude,
}: FileTreeProps) {
  const rendered = useMemo(
    () =>
      nodes.map((node) => (
        <Row
          key={node.path}
          node={node}
          depth={0}
          activePath={activePath}
          srcDir={srcDir}
          excluded={excluded}
          onSelect={onSelect}
          onToggleExclude={onToggleExclude}
        />
      )),
    [nodes, activePath, srcDir, excluded, onSelect, onToggleExclude],
  );

  if (!nodes.length) {
    return (
      <div className="field-hint" style={{ padding: "4px 8px" }}>
        目录为空
      </div>
    );
  }

  return <div className="tree">{rendered}</div>;
}
