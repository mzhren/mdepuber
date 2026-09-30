import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import type {
  BookFormat,
  BookMeta,
  EpubOptions,
  ExportReport,
  PdfOptions,
  Progress,
} from "../types";
import { pdfPageSizes } from "../types";
import { api, formatBytes, formatDuration } from "../lib/api";
import { CheckIcon, CloseIcon, ExportIcon, InfoIcon, WarnIcon } from "./Icons";

interface ExportDialogProps {
  open: boolean;
  onClose: () => void;
  meta: BookMeta;
  sourceName: string;
  epub: EpubOptions;
  pdf: PdfOptions;
  onEpubChange: (patch: Partial<EpubOptions>) => void;
  onPdfChange: (patch: Partial<PdfOptions>) => void;
  formats: BookFormat[];
  onFormatsChange: (formats: BookFormat[]) => void;
  tocPage: boolean;
  tocDepth: number;
  onTocChange: (patch: { tocPage?: boolean; tocDepth?: number }) => void;
  outputDir: string;
  onOutputDirChange: (dir: string) => void;
  baseName: string;
  onBaseNameChange: (name: string) => void;
  busy: boolean;
  progress: Progress | null;
  error: string | null;
  report: ExportReport | null;
  onExport: () => void;
  onReset: () => void;
}

type Tab = "general" | "epub" | "pdf";

export function ExportDialog(props: ExportDialogProps) {
  const {
    open: isOpen,
    onClose,
    meta,
    sourceName,
    epub,
    pdf,
    onEpubChange,
    onPdfChange,
    formats,
    onFormatsChange,
    tocPage,
    tocDepth,
    onTocChange,
    outputDir,
    onOutputDirChange,
    baseName,
    onBaseNameChange,
    busy,
    progress,
    error,
    report,
    onExport,
    onReset,
  } = props;

  const [tab, setTab] = useState<Tab>("general");

  // Reset back to the options view whenever the dialog is reopened.
  useEffect(() => {
    if (isOpen && !busy) {
      setTab("general");
    }
    if (!isOpen) {
      onReset();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    if (isOpen) window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [isOpen, busy, onClose]);

  if (!isOpen) return null;

  const toggleFormat = (format: BookFormat) => {
    const has = formats.includes(format);
    const next = has ? formats.filter((f) => f !== format) : [...formats, format];
    if (next.length === 0) return;
    onFormatsChange(next);
  };

  const pickDir = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") onOutputDirChange(selected);
  };

  const wantsEpub = formats.includes("epub");
  const wantsPdf = formats.includes("pdf");

  return (
    <div
      className="overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div className="modal" role="dialog" aria-modal="true">
        <div className="modal-head">
          <ExportIcon size={15} />
          <h2>导出电子书</h2>
          <span className="sub">{sourceName}</span>
          <div style={{ flex: 1 }} />
          <button
            type="button"
            className="btn btn-ghost btn-icon"
            onClick={onClose}
            disabled={busy}
            title="关闭"
          >
            <CloseIcon size={13} />
          </button>
        </div>

        <div className="modal-body">
          {report ? (
            <ResultView report={report} busy={busy} />
          ) : (
            <>
              {error && (
                <div className="notice is-error">
                  <WarnIcon size={14} />
                  <div>{error}</div>
                </div>
              )}

              <div className="tabs">
                <button
                  type="button"
                  className={tab === "general" ? "is-active" : ""}
                  onClick={() => setTab("general")}
                >
                  格式与路径
                </button>
                <button
                  type="button"
                  className={tab === "epub" ? "is-active" : ""}
                  onClick={() => setTab("epub")}
                  disabled={!wantsEpub}
                >
                  EPUB 选项
                </button>
                <button
                  type="button"
                  className={tab === "pdf" ? "is-active" : ""}
                  onClick={() => setTab("pdf")}
                  disabled={!wantsPdf}
                >
                  PDF 选项
                </button>
              </div>

              {tab === "general" && (
                <>
                  <div className="format-toggle">
                    <button
                      type="button"
                      className={`format-card${wantsEpub ? " is-on" : ""}`}
                      onClick={() => toggleFormat("epub")}
                    >
                      <div className="name">
                        EPUB
                        {wantsEpub && (
                          <span className="check">
                            <CheckIcon size={13} />
                          </span>
                        )}
                      </div>
                      <div className="desc">
                        标准电子书格式，适合手机、平板与 Kindle（转换后）阅读。支持封面、目录与流式排版。
                      </div>
                    </button>
                    <button
                      type="button"
                      className={`format-card${wantsPdf ? " is-on" : ""}`}
                      onClick={() => toggleFormat("pdf")}
                    >
                      <div className="name">
                        PDF
                        {wantsPdf && (
                          <span className="check">
                            <CheckIcon size={13} />
                          </span>
                        )}
                      </div>
                      <div className="desc">
                        固定版式，适合打印与分享。使用本机 Chrome / Edge 内核渲染 mdBook 的打印视图。
                      </div>
                    </button>
                  </div>

                  <div className="field">
                    <label className="field-label">输出目录</label>
                    <div className="path-picker">
                      <input
                        value={outputDir}
                        onChange={(event) => onOutputDirChange(event.target.value)}
                        spellCheck={false}
                      />
                      <button type="button" className="btn btn-ghost btn-sm" onClick={pickDir}>
                        浏览
                      </button>
                    </div>
                  </div>

                  <div className="field">
                    <label className="field-label">文件名</label>
                    <div className="path-picker">
                      <input
                        value={baseName}
                        onChange={(event) => onBaseNameChange(event.target.value)}
                        spellCheck={false}
                      />
                      <span className="field-hint" style={{ margin: 0, whiteSpace: "nowrap" }}>
                        .epub / .pdf
                      </span>
                    </div>
                  </div>

                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={tocPage}
                      onChange={(event) => onTocChange({ tocPage: event.target.checked })}
                    />
                    生成目录页（放在封面之后）
                  </label>

                  {tocPage && (
                    <div className="field" style={{ maxWidth: 260 }}>
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

                  <div className="notice">
                    <InfoIcon size={14} />
                    <div>
                      将使用的元数据：<strong>{meta.title || sourceName}</strong> ·{" "}
                      {meta.authors.length ? meta.authors.join("、") : "未署名作者"} ·{" "}
                      {meta.language}
                    </div>
                  </div>
                </>
              )}

              {tab === "epub" && (
                <>
                  <div className="grid-2">
                    <div className="field">
                      <label className="field-label">EPUB 版本</label>
                      <select
                        value={epub.version}
                        onChange={(event) => onEpubChange({ version: event.target.value })}
                      >
                        <option value="3">EPUB 3（推荐）</option>
                        <option value="2">EPUB 2（兼容老旧设备）</option>
                      </select>
                    </div>
                    <div className="field">
                      <label className="field-label">正文字体</label>
                      <select
                        value={epub.fontFamily}
                        onChange={(event) => onEpubChange({ fontFamily: event.target.value })}
                      >
                        <option value="serif">衬线（宋体 / Songti）</option>
                        <option value="sans">无衬线（黑体 / PingFang）</option>
                        <option value="system">跟随阅读器默认</option>
                      </select>
                    </div>
                  </div>

                  <div className="grid-2">
                    <div className="field">
                      <label className="field-label">字号（em）</label>
                      <input
                        type="number"
                        min={0.6}
                        max={2}
                        step={0.05}
                        value={epub.fontSize}
                        onChange={(event) =>
                          onEpubChange({ fontSize: Number(event.target.value) || 1 })
                        }
                      />
                    </div>
                    <div className="field">
                      <label className="field-label">行高</label>
                      <input
                        type="number"
                        min={1}
                        max={2.5}
                        step={0.05}
                        value={epub.lineHeight}
                        onChange={(event) =>
                          onEpubChange({ lineHeight: Number(event.target.value) || 1.6 })
                        }
                      />
                    </div>
                  </div>

                  <div className="field">
                    <label className="field-label">翻页方向</label>
                    <select
                      value={epub.pageDirection}
                      onChange={(event) => onEpubChange({ pageDirection: event.target.value })}
                    >
                      <option value="default">按语言自动判断</option>
                      <option value="ltr">从左到右</option>
                      <option value="rtl">从右到左（竖排/阿拉伯语）</option>
                    </select>
                  </div>

                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={epub.titlePage}
                      onChange={(event) => onEpubChange({ titlePage: event.target.checked })}
                    />
                    生成书名页
                  </label>
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={epub.coverPage}
                      onChange={(event) => onEpubChange({ coverPage: event.target.checked })}
                    />
                    生成封面页（需要先在左侧选择封面图片）
                  </label>

                  <div className="field" style={{ marginTop: 8 }}>
                    <label className="field-label">附加 CSS</label>
                    <textarea
                      rows={4}
                      spellCheck={false}
                      placeholder="例如：body { letter-spacing: 0.02em; }"
                      value={epub.extraCss}
                      onChange={(event) => onEpubChange({ extraCss: event.target.value })}
                    />
                  </div>
                </>
              )}

              {tab === "pdf" && (
                <>
                  <div className="grid-2">
                    <div className="field">
                      <label className="field-label">页面尺寸</label>
                      <select
                        value={pdf.pageSize}
                        onChange={(event) => onPdfChange({ pageSize: event.target.value })}
                      >
                        {pdfPageSizes.map((size) => (
                          <option key={size} value={size}>
                            {size === "custom" ? "自定义" : size}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div className="field">
                      <label className="field-label">页边距（mm）</label>
                      <input
                        type="number"
                        min={0}
                        max={60}
                        step={1}
                        value={pdf.marginMm}
                        onChange={(event) =>
                          onPdfChange({ marginMm: Number(event.target.value) || 0 })
                        }
                      />
                    </div>
                  </div>

                  {pdf.pageSize === "custom" && (
                    <div className="grid-2">
                      <div className="field">
                        <label className="field-label">宽度（mm）</label>
                        <input
                          type="number"
                          min={50}
                          max={600}
                          value={pdf.customWidthMm}
                          onChange={(event) =>
                            onPdfChange({ customWidthMm: Number(event.target.value) || 148 })
                          }
                        />
                      </div>
                      <div className="field">
                        <label className="field-label">高度（mm）</label>
                        <input
                          type="number"
                          min={50}
                          max={600}
                          value={pdf.customHeightMm}
                          onChange={(event) =>
                            onPdfChange({ customHeightMm: Number(event.target.value) || 210 })
                          }
                        />
                      </div>
                    </div>
                  )}

                  <div className="grid-2">
                    <div className="field">
                      <label className="field-label">配色</label>
                      <select
                        value={pdf.theme}
                        onChange={(event) => onPdfChange({ theme: event.target.value })}
                      >
                        <option value="light">浅色（适合打印）</option>
                        <option value="sepia">护眼米色</option>
                        <option value="dark">深色（适合屏幕）</option>
                      </select>
                    </div>
                    <div className="field">
                      <label className="field-label">缩放</label>
                      <input
                        type="number"
                        min={0.5}
                        max={2}
                        step={0.05}
                        value={pdf.scale}
                        onChange={(event) =>
                          onPdfChange({ scale: Number(event.target.value) || 1 })
                        }
                      />
                    </div>
                  </div>

                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={pdf.outline}
                      onChange={(event) => onPdfChange({ outline: event.target.checked })}
                    />
                    生成 PDF 书签（由标题层级自动生成）
                  </label>
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={pdf.headerFooter}
                      onChange={(event) => onPdfChange({ headerFooter: event.target.checked })}
                    />
                    显示页眉页脚（含日期与页码）
                  </label>
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={pdf.printBackground}
                      onChange={(event) => onPdfChange({ printBackground: event.target.checked })}
                    />
                    打印背景色与提示块底色
                  </label>

                  <div className="field" style={{ marginTop: 8 }}>
                    <label className="field-label">附加 CSS（仅打印时生效）</label>
                    <textarea
                      rows={4}
                      spellCheck={false}
                      placeholder="例如：@page { margin: 25mm; }"
                      value={pdf.extraCss}
                      onChange={(event) => onPdfChange({ extraCss: event.target.value })}
                    />
                  </div>
                </>
              )}
            </>
          )}
        </div>

        <div className="modal-foot">
          {busy && progress ? (
            <div style={{ flex: 1 }}>
              <div className="progress-track">
                <span style={{ width: `${progress.percent}%` }} />
              </div>
              <div className="progress-label">
                <span>{progress.message}</span>
                <span>{progress.percent}%</span>
              </div>
            </div>
          ) : (
            <>
              <span className="field-hint" style={{ margin: 0 }}>
                {report
                  ? `完成，用时 ${formatDuration(report.durationMs)}`
                  : `将导出 ${formats.map((f) => f.toUpperCase()).join(" + ")}`}
              </span>
              <div className="spacer" />
              {report ? (
                <>
                  <button type="button" className="btn btn-ghost" onClick={onReset}>
                    继续导出
                  </button>
                  <button type="button" className="btn btn-primary" onClick={onClose}>
                    完成
                  </button>
                </>
              ) : (
                <>
                  <button type="button" className="btn btn-ghost" onClick={onClose}>
                    取消
                  </button>
                  <button type="button" className="btn btn-primary" onClick={onExport}>
                    <ExportIcon size={13} />
                    开始导出
                  </button>
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function ResultView({ report, busy }: { report: ExportReport; busy: boolean }) {
  const rows: { label: string; path: string; size: number }[] = [];
  if (report.epubPath) {
    rows.push({ label: "EPUB", path: report.epubPath, size: report.bytesEpub });
  }
  if (report.pdfPath) {
    rows.push({ label: "PDF", path: report.pdfPath, size: report.bytesPdf });
  }

  return (
    <>
      <div className="notice is-ok">
        <CheckIcon size={14} />
        <div>
          导出成功：{report.chapterCount} 个章节
          {busy ? "（仍在收尾…）" : ""}
        </div>
      </div>

      <div className="result-list">
        {rows.map((row) => (
          <div className="result-row" key={row.path}>
            <div className="source-icon" style={{ width: 30, height: 30 }}>
              {row.label}
            </div>
            <div className="meta">
              <div className="name">
                {row.label} · {formatBytes(row.size)}
              </div>
              <div className="path" title={row.path}>
                {row.path}
              </div>
            </div>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => api.openPath(row.path, false)}
            >
              打开
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => api.openPath(row.path, true)}
            >
              显示位置
            </button>
          </div>
        ))}
      </div>

      {report.warnings.length > 0 && (
        <div className="notice is-warn" style={{ marginTop: 12 }}>
          <WarnIcon size={14} />
          <div>
            提示
            <ul>
              {report.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          </div>
        </div>
      )}

      <div className="field-hint" style={{ marginTop: 12 }}>
        HTML 预览目录：<code>{report.htmlDir}</code>
      </div>
    </>
  );
}
