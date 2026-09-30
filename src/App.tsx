import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type {
  BookFormat,
  BookMeta,
  ChapterDoc,
  ChapterEntry,
  EnvironmentInfo,
  EpubOptions,
  ExportReport,
  PdfOptions,
  PreviewInfo,
  Progress,
  Workspace,
} from "./types";
import { defaultBookMeta, defaultEpubOptions, defaultPdfOptions, emptyFrontMatter } from "./types";
import { api, onProgress } from "./lib/api";
import { countWords } from "./lib/markdown";
import { Sidebar } from "./components/Sidebar";
import { PreviewPane, useChapterIndex, type PreviewMode } from "./components/PreviewPane";
import { ExportDialog } from "./components/ExportDialog";
import { StatusBar, type StatusKind } from "./components/StatusBar";
import { EmptyState } from "./components/EmptyState";
import { ExportIcon, MoonIcon, PanelIcon, SunIcon } from "./components/Icons";
import {
  THEME_LABEL,
  applyTheme,
  otherTheme,
  persistTheme,
  resolveInitialTheme,
  type Theme,
} from "./lib/theme";

export default function App() {
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [chapter, setChapter] = useState<ChapterEntry | null>(null);
  const [chapterDoc, setChapterDoc] = useState<ChapterDoc | null>(null);
  const [loadingChapter, setLoadingChapter] = useState(false);

  const [meta, setMeta] = useState<BookMeta>(defaultBookMeta);
  const [epub, setEpub] = useState<EpubOptions>(defaultEpubOptions);
  const [pdf, setPdf] = useState<PdfOptions>(defaultPdfOptions);
  const [formats, setFormats] = useState<BookFormat[]>(["epub"]);
  const [tocPage, setTocPage] = useState(false);
  const [tocDepth, setTocDepth] = useState(3);
  const [outputDir, setOutputDir] = useState("");
  const [baseName, setBaseName] = useState("");

  const [mode, setMode] = useState<PreviewMode>("render");
  const [previewInfo, setPreviewInfo] = useState<PreviewInfo | null>(null);
  const [previewBuilding, setPreviewBuilding] = useState(false);

  const [dialogOpen, setDialogOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [report, setReport] = useState<ExportReport | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);

  const [status, setStatus] = useState<{ kind: StatusKind; message: string }>({
    kind: "idle",
    message: "就绪",
  });
  const [environment, setEnvironment] = useState<EnvironmentInfo | null>(null);
  const [dragActive, setDragActive] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [theme, setTheme] = useState<Theme>(() => resolveInitialTheme());

  const contentCache = useRef(new Map<string, ChapterDoc>());
  const [sourceBase, setSourceBase] = useState("mdepub://localhost");

  const setOk = useCallback((message: string) => setStatus({ kind: "ok", message }), []);
  const setIdle = useCallback((message: string) => setStatus({ kind: "idle", message }), []);
  const setErr = useCallback((message: string) => setStatus({ kind: "error", message }), []);

  /* ----------------------------------------------------------------- theme */

  useEffect(() => {
    applyTheme(theme);
    persistTheme(theme);
    // Mirroring the theme onto the native window also flips
    // `prefers-color-scheme` inside the webview, which is what makes the
    // embedded mdBook preview follow along (mdBook keys its default theme off
    // the system preference).
    getCurrentWindow()
      .setTheme(theme)
      .catch((error) => {
        // Not fatal — the app chrome still switches. Only the native window
        // appearance (and with it `prefers-color-scheme` for the embedded
        // mdBook preview) would stay behind.
        console.warn("[mdepuber] 无法设置窗口主题", error);
      });
  }, [theme]);

  const toggleTheme = useCallback(() => {
    setTheme((current) => otherTheme(current));
  }, []);

  /* ------------------------------------------------------------ bootstrap */

  useEffect(() => {
    api.environmentInfo().then(setEnvironment).catch(() => undefined);
    api.previewBase().then(setSourceBase).catch(() => undefined);
    const unlisten = onProgress((next) => setProgress(next));
    return () => {
      unlisten.then((fn) => fn()).catch(() => undefined);
    };
  }, []);

  /* --------------------------------------------------------- open workspace */

  const loadChapter = useCallback(
    async (target: ChapterEntry) => {
      setChapter(target);
      const cached = contentCache.current.get(target.path);
      if (cached !== undefined) {
        setChapterDoc(cached);
        return;
      }
      setLoadingChapter(true);
      try {
        const doc = await api.readChapter(target.path);
        contentCache.current.set(target.path, doc);
        setChapterDoc(doc);
      } catch (error) {
        setChapterDoc({
          body: `> 无法读取该文件：${(error as Error).message}`,
          frontMatter: emptyFrontMatter(),
        });
      } finally {
        setLoadingChapter(false);
      }
    },
    [],
  );

  const adoptWorkspace = useCallback(
    async (ws: Workspace) => {
      contentCache.current.clear();
      setWorkspace(ws);
      setMeta({ ...ws.meta });
      setPreviewInfo(null);
      setPreviewBuilding(false);
      setReport(null);
      setExportError(null);
      setMode("render");
      setBaseName(ws.title);
      setFormats(["epub"]);

      const dir = await api.defaultOutputDir(ws.root).catch(() => "");
      setOutputDir(dir);

      if (ws.chapters.length > 0) {
        await loadChapter(ws.chapters[0]);
      } else {
        setChapter(null);
        setChapterDoc(null);
      }

      if (ws.warnings.length > 0) {
        setStatus({ kind: "idle", message: ws.warnings[0] });
      } else {
        setOk(
          `已打开「${ws.title}」：${ws.chapters.length} 个章节，共 ${ws.totalWords.toLocaleString("zh-CN")} 字`,
        );
      }
    },
    [loadChapter, setOk],
  );

  const openPath = useCallback(
    async (path: string) => {
      setIdle("正在读取…");
      try {
        const ws = await api.openWorkspace(path);
        await adoptWorkspace(ws);
      } catch (error) {
        setErr((error as Error).message);
      }
    },
    [adoptWorkspace, setIdle, setErr],
  );

  // Open a path passed on the command line ("Open With", drag onto the Dock
  // icon, or `mdepuber book.md`).
  const initialPathHandled = useRef(false);
  useEffect(() => {
    if (initialPathHandled.current) return;
    initialPathHandled.current = true;
    api
      .initialPath()
      .then((path) => {
        if (path) void openPath(path);
      })
      .catch(() => undefined);
  }, [openPath]);

  const openFile = useCallback(async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "Markdown", extensions: ["md", "markdown", "mdown", "mkd"] },
        { name: "全部文件", extensions: ["*"] },
      ],
    });
    if (typeof selected === "string") await openPath(selected);
  }, [openPath]);

  const openFolder = useCallback(async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") await openPath(selected);
  }, [openPath]);

  /* --------------------------------------------------------- drag and drop */

  useEffect(() => {
    let dispose: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over") {
          setDragActive(true);
        } else if (event.payload.type === "drop") {
          setDragActive(false);
          const paths = event.payload.paths ?? [];
          if (paths.length > 0) void openPath(paths[0]);
        } else {
          setDragActive(false);
        }
      })
      .then((fn) => {
        dispose = fn;
      })
      .catch(() => undefined);
    return () => dispose?.();
  }, [openPath]);

  /* ------------------------------------------------------------- navigation */

  const chapters = useMemo(() => workspace?.chapters ?? [], [workspace]);
  const chapterIndex = useChapterIndex(chapters, chapter);

  const gotoOffset = useCallback(
    (offset: number) => {
      const next = chapters[chapterIndex + offset];
      if (next) void loadChapter(next);
    },
    [chapters, chapterIndex, loadChapter],
  );

  const handleSelectFile = useCallback(
    (path: string, isMarkdown: boolean) => {
      const match = chapters.find((item) => item.path === path);
      if (match) {
        void loadChapter(match);
        return;
      }
      if (!isMarkdown) {
        setIdle(`已选择非 Markdown 文件：${path.split("/").pop()}`);
        return;
      }
      void (async () => {
        try {
          const doc = await api.readChapter(path);
          contentCache.current.set(path, doc);
          const name = path.split("/").pop() ?? "未命名";
          setChapter({
            id: path,
            title: doc.frontMatter.title?.trim() || name.replace(/\.(md|markdown)$/i, ""),
            path,
            rel: name,
            level: 1,
            words: countWords(doc.body),
          });
          setChapterDoc(doc);
        } catch (error) {
          setErr((error as Error).message);
        }
      })();
    },
    [chapters, loadChapter, setIdle, setErr],
  );

  /** Resolve a relative link inside a chapter to an absolute path. */
  const resolveLink = useCallback(
    (href: string): string | null => {
      if (!chapter) return null;
      const clean = href.split("#")[0];
      if (!clean) return null;
      const decoded = decodeURIComponent(clean);
      const sourceDir = chapter.path.slice(0, chapter.path.lastIndexOf("/"));
      const parts = sourceDir.split("/");
      for (const segment of decoded.split("/")) {
        if (segment === "" || segment === ".") continue;
        if (segment === "..") parts.pop();
        else parts.push(segment);
      }
      const resolved = parts.join("/");
      if (chapters.some((item) => item.path === resolved)) return resolved;
      if (/\.md$/i.test(resolved) && chapters.some((item) => item.path === resolved)) {
        return resolved;
      }
      return null;
    },
    [chapter, chapters],
  );

  /* ---------------------------------------------------------------- preview */

  const buildPreview = useCallback(async () => {
    if (!workspace) return;
    setPreviewBuilding(true);
    setStatus({ kind: "busy", message: "正在用 mdBook 构建整书预览…" });
    setProgress(null);
    try {
      const info = await api.buildPreview(workspace.root, meta);
      setPreviewInfo(info);
      setMode("mdbook");
      setOk(`mdBook 预览已构建：${info.chapterCount} 个章节`);
    } catch (error) {
      setErr((error as Error).message);
    } finally {
      setPreviewBuilding(false);
      setProgress(null);
    }
  }, [workspace, meta, setOk, setErr]);

  /* ----------------------------------------------------------------- export */

  const runExport = useCallback(async () => {
    if (!workspace) return;
    setBusy(true);
    setExportError(null);
    setReport(null);
    setStatus({ kind: "busy", message: "正在导出…" });

    try {
      const result = await api.exportBook({
        source: workspace.root,
        meta,
        epub,
        pdf,
        formats,
        tocPage,
        tocDepth,
        outputDir,
        baseName: baseName || meta.title || workspace.title,
      });
      setReport(result);
      setOk(
        `导出完成，用时 ${(result.durationMs / 1000).toFixed(1)} 秒` +
          (result.epubPath ? " · EPUB" : "") +
          (result.pdfPath ? " · PDF" : ""),
      );
    } catch (error) {
      setExportError((error as Error).message);
      setErr((error as Error).message);
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }, [workspace, meta, epub, pdf, formats, tocPage, tocDepth, outputDir, baseName, setOk, setErr]);

  const openExportDialog = useCallback(() => {
    setReport(null);
    setExportError(null);
    setDialogOpen(true);
  }, []);

  /* ------------------------------------------------------------------ render */

  const statusProps = useMemo(
    () => ({
      kind: status.kind,
      message: status.message,
      chapters: workspace?.chapters.length ?? 0,
      words: workspace?.totalWords ?? 0,
      environment,
      progress,
    }),
    [status, workspace, environment, progress],
  );

  return (
    <div className="app">
      <header className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <span className="brand-mark">M</span>
          <span>mdepuber</span>
          <span className="brand-sub">Markdown → EPUB / PDF</span>
        </div>

        <div className="titlebar-spacer" data-tauri-drag-region />

        <div className="titlebar-group">
          <button
            type="button"
            className="btn btn-ghost btn-icon"
            onClick={() => setSidebarOpen((value) => !value)}
            title={sidebarOpen ? "收起工作区" : "展开工作区"}
          >
            <PanelIcon size={14} />
          </button>

          <button
            type="button"
            className="btn btn-ghost btn-icon"
            onClick={toggleTheme}
            title={`切换到${THEME_LABEL[otherTheme(theme)]}模式`}
            aria-label={`切换到${THEME_LABEL[otherTheme(theme)]}模式`}
          >
            {theme === "dark" ? <SunIcon size={14} /> : <MoonIcon size={14} />}
          </button>

          <div className="segmented">
            <button
              type="button"
              className={mode === "render" ? "is-active" : ""}
              onClick={() => setMode("render")}
              disabled={!workspace}
            >
              章节预览
            </button>
            <button
              type="button"
              className={mode === "mdbook" ? "is-active" : ""}
              onClick={() => {
                setMode("mdbook");
                if (!previewInfo && workspace) void buildPreview();
              }}
              disabled={!workspace}
            >
              mdBook 预览
            </button>
          </div>

          <button
            type="button"
            className="btn btn-primary"
            onClick={openExportDialog}
            disabled={!workspace || busy}
          >
            <ExportIcon size={14} />
            导出
          </button>
        </div>
      </header>

      <div className={`body${sidebarOpen ? "" : " is-collapsed"}`}>
        {sidebarOpen && (
          <Sidebar
            workspace={workspace}
            meta={meta}
            activeChapterPath={chapter?.path ?? null}
            onMetaChange={(patch) => setMeta((prev) => ({ ...prev, ...patch }))}
            onSelectFile={handleSelectFile}
            tocPage={tocPage}
            tocDepth={tocDepth}
            onTocChange={(patch) => {
              if (patch.tocPage !== undefined) setTocPage(patch.tocPage);
              if (patch.tocDepth !== undefined) setTocDepth(patch.tocDepth);
            }}
          />
        )}

        {workspace ? (
          <PreviewPane
            mode={mode}
            chapter={chapter}
            doc={chapterDoc}
            loadingChapter={loadingChapter}
            workspaceTitle={workspace.title}
            chapterIndex={chapterIndex}
            chapterTotal={chapters.length}
            previewInfo={previewInfo}
            previewBuilding={previewBuilding}
            onBuildPreview={buildPreview}
            onPrev={() => gotoOffset(-1)}
            onNext={() => gotoOffset(1)}
            resolveLink={resolveLink}
            onOpenLinkedFile={(path) => handleSelectFile(path, true)}
            sourceRoot={workspace.srcDir}
            sourceBase={sourceBase}
          />
        ) : (
          <EmptyState
            onOpenFile={openFile}
            onOpenFolder={openFolder}
            dragActive={dragActive}
          />
        )}
      </div>

      <StatusBar {...statusProps} />

      <ExportDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        meta={meta}
        sourceName={workspace?.title ?? ""}
        epub={epub}
        pdf={pdf}
        onEpubChange={(patch) => setEpub((prev) => ({ ...prev, ...patch }))}
        onPdfChange={(patch) => setPdf((prev) => ({ ...prev, ...patch }))}
        formats={formats}
        onFormatsChange={setFormats}
        tocPage={tocPage}
        tocDepth={tocDepth}
        onTocChange={(patch) => {
          if (patch.tocPage !== undefined) setTocPage(patch.tocPage);
          if (patch.tocDepth !== undefined) setTocDepth(patch.tocDepth);
        }}
        outputDir={outputDir}
        onOutputDirChange={setOutputDir}
        baseName={baseName}
        onBaseNameChange={setBaseName}
        busy={busy}
        progress={progress}
        error={exportError}
        report={report}
        onExport={runExport}
        onReset={() => {
          setReport(null);
          setExportError(null);
        }}
      />
    </div>
  );
}
