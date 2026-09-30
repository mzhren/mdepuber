import { FolderOpenIcon, FileIcon } from "./Icons";

interface EmptyStateProps {
  onOpenFile: () => void;
  onOpenFolder: () => void;
  dragActive: boolean;
}

export function EmptyState({ onOpenFile, onOpenFolder, dragActive }: EmptyStateProps) {
  return (
    <div className="empty">
      <div className="empty-card">
        <div className="empty-mark">M</div>
        <h1 className="empty-title">把 Markdown 变成一本真正的电子书</h1>
        <p className="empty-text">
          以 <strong>mdBook</strong> 为渲染内核：读取 <code>SUMMARY.md</code>
          、运行预处理器、生成 HTML，再打包成 EPUB 或打印成 PDF。
          <br />
          打开一个 <code>.md</code> 文件，或者一个装满 Markdown 的文件夹即可开始。
        </p>

        <div className="empty-actions">
          <button type="button" className="btn btn-primary" onClick={onOpenFolder}>
            <FolderOpenIcon size={14} />
            打开文件夹
          </button>
          <button type="button" className="btn btn-ghost" onClick={onOpenFile}>
            <FileIcon size={14} />
            打开 Markdown 文件
          </button>
        </div>

        <div className={`drop-hint${dragActive ? " is-over" : ""}`}>
          {dragActive ? "松开鼠标即可打开" : "也可以把文件或文件夹直接拖进这个窗口"}
        </div>

        <div className="feature-grid">
          <div className="feature">
            <h4>左侧工作区</h4>
            <p>章节大纲、文件树与书籍元数据，改动即时生效。</p>
          </div>
          <div className="feature">
            <h4>右侧双预览</h4>
            <p>即时渲染预览，或切到 mdBook 整书预览查看最终效果。</p>
          </div>
          <div className="feature">
            <h4>一键导出</h4>
            <p>EPUB 3 与 PDF 同时输出，支持封面、目录与书签。</p>
          </div>
        </div>
      </div>
    </div>
  );
}
