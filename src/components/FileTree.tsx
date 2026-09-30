import { useMemo, useState } from "react";

import type { FsNode } from "../types";
import { FileIcon, FolderIcon, ImageIcon } from "./Icons";

interface FileTreeProps {
  nodes: FsNode[];
  activePath: string | null;
  onSelect: (path: string, isMarkdown: boolean) => void;
}

function glyph(node: FsNode) {
  if (node.isDir) return <FolderIcon size={13} />;
  const ext = node.name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "svg", "webp", "avif"].includes(ext)) {
    return <ImageIcon size={13} />;
  }
  return <FileIcon size={13} />;
}

interface RowProps {
  node: FsNode;
  depth: number;
  activePath: string | null;
  onSelect: (path: string, isMarkdown: boolean) => void;
}

function Row({ node, depth, activePath, onSelect }: RowProps) {
  const [open, setOpen] = useState(depth < 2);
  const isActive = !node.isDir && node.path === activePath;

  if (node.isDir) {
    const children = node.children ?? [];
    return (
      <div>
        <button
          type="button"
          className="tree-row"
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
              onSelect={onSelect}
            />
          ))}
      </div>
    );
  }

  return (
    <button
      type="button"
      className={`tree-row${isActive ? " is-active" : ""}${node.inBook ? "" : " is-dim"}`}
      style={{ paddingLeft: 8 + depth * 12 + 12 }}
      onClick={() => onSelect(node.path, node.isMarkdown)}
      title={node.path}
    >
      <span className="tree-glyph">{glyph(node)}</span>
      <span className="tree-label">{node.name}</span>
      {node.inBook && <span className="tree-badge">书</span>}
    </button>
  );
}

export function FileTree({ nodes, activePath, onSelect }: FileTreeProps) {
  const rendered = useMemo(
    () =>
      nodes.map((node) => (
        <Row key={node.path} node={node} depth={0} activePath={activePath} onSelect={onSelect} />
      )),
    [nodes, activePath, onSelect],
  );

  if (!nodes.length) {
    return <div className="field-hint" style={{ padding: "4px 8px" }}>目录为空</div>;
  }

  return <div className="tree">{rendered}</div>;
}
