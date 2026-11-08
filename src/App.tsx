import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { confirm, open } from "@tauri-apps/plugin-dialog";
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
import { countWords, splitFrontMatter } from "./lib/markdown";
import { orderChapters } from "./lib/chapters";
import { toRelative } from "./components/FileTree";
import { Sidebar } from "./components/Sidebar";
import { PreviewPane, useChapterIndex, type PreviewMode } from "./components/PreviewPane";
import { EditorPane } from "./components/EditorPane";
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

type WorkspaceViewMode = PreviewMode | "edit";

/** Refresh a chapter's title/words in the workspace after a successful save. */
function patchWorkspaceStats(ws: Workspace, path: string, doc: ChapterDoc | null): Workspace {
  if (!doc) return ws;
  let touched = false;
  const chapters = ws.chapters.map((entry) => {
    if (entry.path !== path) return entry;
    touched = true;
    return {
      ...entry,
      title: doc.frontMatter.title?.trim() || entry.title,
      words: countWords(doc.body),
    };
  });
  if (!touched) return ws;
  return {
    ...ws,
    chapters,
    totalWords: chapters.reduce((sum, entry) => sum + entry.words, 0),
  };
}

export default function App() {
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [chapter, setChapter] = useState<ChapterEntry | null>(null);
  const [chapterDoc, setChapterDoc] = useState<ChapterDoc | null>(null);
  const [loadingChapter, setLoadingChapter] = useState(false);

  const [meta, setMeta] = useState<BookMeta>(defaultBookMeta);
  const [epub, setEpub] = useState<EpubOptions>(defaultEpubOptions);
  const [pdf, setPdf] = useState<PdfOptions>(defaultPdfOptions);
  const [formats, setFormats] = useState<BookFormat[]>(["epub"]);
  /** Chapter paths removed from the book (source-relative). Files stay on disk. */
  const [excluded, setExcluded] = useState<Set<string>>(() => new Set());
  /** Chapter order dragged in the outline (source-relative). Null = natural. */
  const [chapterOrder, setChapterOrder] = useState<string[] | null>(null);
  const [tocPage, setTocPage] = useState(false);
  const [tocDepth, setTocDepth] = useState(3);
  const [outputDir, setOutputDir] = useState("");
  const [baseName, setBaseName] = useState("");

  const [mode, setMode] = useState<WorkspaceViewMode>("render");
  const [previewInfo, setPreviewInfo] = useState<PreviewInfo | null>(null);
  const [previewBuilding, setPreviewBuilding] = useState(false);

  /* ------------------------------------------------------------- edit buffer */
  /** The chapter currently open in the editor, with its dirty baseline. */
  const [edit, setEdit] = useState<{ path: string; raw: string } | null>(null);
  const [editDirty, setEditDirty] = useState(false);
  const [editSaving, setEditSaving] = useState(false);
  /** Bumped on every buffer (re)load so the CodeMirror view remounts. */
  const [editVersion, setEditVersion] = useState(0);
  // Refs mirror the buffer for code that must not wait for React state.
  const editPathRef = useRef<string | null>(null);
  const editDirtyRef = useRef(false);
  const editTextRef = useRef("");
  const editSavingRef = useRef(false);
  const editorSeqRef = useRef(0);
  const modeRef = useRef<WorkspaceViewMode>(mode);
  const chapterPathRef = useRef<string | null>(null);

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
  /** Which side the workspace sidebar sits on (the panel button flips it). */
  const [sidebarSide, setSidebarSide] = useState<"left" | "right">(() =>
    localStorage.getItem("mdepuber.sidebarSide") === "right" ? "right" : "left",
  );
  const [theme, setTheme] = useState<Theme>(() => resolveInitialTheme());

  const contentCache = useRef(new Map<string, ChapterDoc>());
  const [sourceBase, setSourceBase] = useState("mdepub://localhost");

  // Keep a synchronous mirror of the mode for callbacks registered once.
  modeRef.current = mode;
  chapterPathRef.current = chapter?.path ?? null;

  const setOk = useCallback((message: string) => setStatus({ kind: "ok", message }), []);
  const setIdle = useCallback((message: string) => setStatus({ kind: "idle", message }), []);
  const setErr = useCallback((message: string) => setStatus({ kind: "error", message }), []);

  /* ------------------------------------------------------------- edit buffer */

  const discardEditBuffer = useCallback(() => {
    editPathRef.current = null;
    editDirtyRef.current = false;
    editTextRef.current = "";
    setEdit(null);
    setEditDirty(false);
  }, []);

  const confirmDiscard = useCallback(async () => {
    try {
      return await confirm("当前章节有未保存的修改，丢弃修改并继续吗？", {
        title: "未保存的修改",
        kind: "warning",
        okLabel: "丢弃修改",
        cancelLabel: "取消",
      });
    } catch {
      return false;
    }
  }, []);

  /** Load the raw file into the editor buffer (front matter included). */
  const openEditorBuffer = useCallback(
    async (path: string) => {
      const seq = (editorSeqRef.current += 1);
      try {
        const raw = await api.readChapterRaw(path);
        // A newer open (rapid chapter switching) supersedes this one.
        if (seq !== editorSeqRef.current) return;
        editPathRef.current = path;
        editDirtyRef.current = false;
        editTextRef.current = raw;
        setEdit({ path, raw });
        setEditDirty(false);
        setEditVersion((version) => version + 1);
      } catch (error) {
        if (seq !== editorSeqRef.current) return;
        setErr(`无法打开编辑器：${(error as Error).message}`);
      }
    },
    [setErr],
  );

  const handleEditDirty = useCallback((dirty: boolean) => {
    editDirtyRef.current = dirty;
    setEditDirty(dirty);
  }, []);

  /** Mirror the buffer's text into a ref the save path can read synchronously. */
  const handleEditText = useCallback((text: string) => {
    editTextRef.current = text;
  }, []);

  const saveEdit = useCallback(async () => {
    const buffer = edit;
    if (!buffer || !editDirtyRef.current || editSavingRef.current) return;
    // Snapshot at call time: keystrokes typed while the write is in flight
    // stay dirty instead of being silently marked as saved.
    const textToSave = editTextRef.current;
    editSavingRef.current = true;
    setEditSaving(true);
    try {
      await api.writeChapter(buffer.path, textToSave);
      // Refresh the preview cache + sidebar stats from what actually landed on
      // disk, and retire any mdBook preview built from the old contents.
      const doc = await api.readChapter(buffer.path).catch(() => null);
      if (doc) {
        contentCache.current.set(buffer.path, doc);
        // The reading preview renders `chapterDoc`, not the cache — refresh it
        // too, otherwise switching back to 章节预览 shows the pre-save text.
        if (chapterPathRef.current === buffer.path) setChapterDoc(doc);
      }
      setEdit((prev) =>
        prev && prev.path === buffer.path ? { ...prev, raw: textToSave } : prev,
      );
      editDirtyRef.current = false;
      setEditDirty(false);
      setWorkspace((ws) => (ws ? patchWorkspaceStats(ws, buffer.path, doc) : ws));
      setPreviewInfo(null);
      setOk(`已保存「${buffer.path.split("/").pop()}」`);
    } catch (error) {
      setErr(`保存失败：${(error as Error).message}`);
    } finally {
      editSavingRef.current = false;
      setEditSaving(false);
    }
  }, [edit, setOk, setErr]);

  // ⌘S / Ctrl+S saves in edit mode, wherever the focus currently sits.
  useEffect(() => {
    if (mode !== "edit") return;
    const handler = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void saveEdit();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [mode, saveEdit]);

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

  const toggleSidebarSide = useCallback(() => {
    setSidebarSide((side) => {
      const next = side === "left" ? "right" : "left";
      try {
        localStorage.setItem("mdepuber.sidebarSide", next);
      } catch {
        /* private mode etc. — the flip still applies for this session */
      }
      return next;
    });
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
      // Moving to a different chapter with unsaved edits needs an explicit
      // "discard" from the user first.
      if (editPathRef.current && editPathRef.current !== target.path && editDirtyRef.current) {
        const proceed = await confirmDiscard();
        if (!proceed) return;
        discardEditBuffer();
      }
      setChapter(target);
      // In edit mode the buffer follows the chapter (the doc below keeps the
      // preview cache warm for when the user switches back to reading). Never
      // reload the buffer for the chapter already on screen — that would wipe
      // unsaved edits without a prompt.
      if (modeRef.current === "edit" && editPathRef.current !== target.path) {
        void openEditorBuffer(target.path);
      }
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
    [confirmDiscard, discardEditBuffer, openEditorBuffer],
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
      discardEditBuffer();
      modeRef.current = "render";
      setMode("render");
      setBaseName(ws.title);
      setFormats(["epub"]);
      setExcluded(new Set());
      setChapterOrder(null);

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
    [loadChapter, discardEditBuffer, setOk],
  );

  const openPath = useCallback(
    async (path: string) => {
      // Opening another book must not silently drop unsaved edits.
      if (editDirtyRef.current) {
        const proceed = await confirmDiscard();
        if (!proceed) return;
        discardEditBuffer();
      }
      setIdle("正在读取…");
      try {
        const ws = await api.openWorkspace(path);
        await adoptWorkspace(ws);
      } catch (error) {
        setErr((error as Error).message);
      }
    },
    [adoptWorkspace, confirmDiscard, discardEditBuffer, setIdle, setErr],
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

  const relOf = useCallback(
    (path: string) => (workspace ? toRelative(workspace.srcDir, path) : path),
    [workspace],
  );

  /** Outline in display order — the natural order until the user drags. */
  const chapters = useMemo(
    () => (workspace ? orderChapters(workspace.chapters, relOf, chapterOrder) : []),
    [workspace, relOf, chapterOrder],
  );

  const handleReorder = useCallback((order: string[]) => {
    setChapterOrder(order);
    // The mdBook preview is a built artefact; it has to be rebuilt to follow.
    setPreviewInfo(null);
  }, []);
  const chapterIndex = useChapterIndex(chapters, chapter);

  const gotoOffset = useCallback(
    (offset: number) => {
      const next = chapters[chapterIndex + offset];
      if (next) void loadChapter(next);
    },
    [chapters, chapterIndex, loadChapter],
  );

  const toggleExclude = useCallback(
    (rel: string) => {
      setExcluded((previous) => {
        const next = new Set(previous);
        if (next.has(rel)) {
          next.delete(rel);
          return next;
        }
        // An empty book cannot be built, so keep the last chapter.
        if (workspace && next.size + 1 >= workspace.chapters.length) {
          setErr("至少要保留一个章节，无法把全部章节排除。");
          return previous;
        }
        next.add(rel);
        return next;
      });
      // The mdBook preview is a built artefact; it has to be rebuilt to reflect
      // the new chapter list.
      setPreviewInfo(null);
    },
    [workspace, setErr],
  );

  const restoreAllExcluded = useCallback(() => {
    setExcluded(new Set());
    setPreviewInfo(null);
  }, []);

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
        // Same unsaved-edit guard as chapter navigation.
        if (editPathRef.current && editPathRef.current !== path && editDirtyRef.current) {
          const proceed = await confirmDiscard();
          if (!proceed) return;
          discardEditBuffer();
        }
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
          if (modeRef.current === "edit" && editPathRef.current !== path) {
            void openEditorBuffer(path);
          }
        } catch (error) {
          setErr((error as Error).message);
        }
      })();
    },
    [chapters, loadChapter, confirmDiscard, discardEditBuffer, openEditorBuffer, setIdle, setErr],
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
      const info = await api.buildPreview(workspace.root, meta, [...excluded], chapterOrder ?? []);
      setPreviewInfo(info);
      setMode("mdbook");
      setOk(`mdBook 预览已构建：${info.chapterCount} 个章节`);
    } catch (error) {
      setErr((error as Error).message);
    } finally {
      setPreviewBuilding(false);
      setProgress(null);
    }
  }, [workspace, meta, excluded, chapterOrder, setOk, setErr]);

  /* -------------------------------------------------------------------- mode */

  const switchMode = useCallback(
    (next: WorkspaceViewMode) => {
      if (next === mode) {
        if (next === "mdbook" && !previewInfo && workspace) void buildPreview();
        return;
      }
      if (mode === "edit" && edit && editDirtyRef.current && next !== "edit") {
        // The reading preview should show what's in the buffer, not the last
        // saved snapshot.
        const parsed = splitFrontMatter(editTextRef.current);
        const doc: ChapterDoc = {
          body: parsed.body,
          frontMatter: { ...emptyFrontMatter(), format: parsed.format },
        };
        setChapterDoc(doc);
        contentCache.current.set(edit.path, doc);
      }
      if (next === "edit") {
        if (!chapter) return;
        // Re-entering the editor on the same chapter keeps its buffer (and any
        // unsaved edits); a different chapter loads a fresh buffer.
        if (!edit || edit.path !== chapter.path) void openEditorBuffer(chapter.path);
      }
      if (next === "mdbook" && !previewInfo && workspace) void buildPreview();
      setMode(next);
    },
    [mode, edit, chapter, previewInfo, workspace, buildPreview, openEditorBuffer],
  );

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
        excluded: [...excluded],
        order: chapterOrder ?? [],
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
  }, [workspace, meta, epub, pdf, formats, excluded, chapterOrder, tocPage, tocDepth, outputDir, baseName, setOk, setErr]);

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
            onClick={toggleSidebarSide}
            title={sidebarSide === "left" ? "将工作区移到右侧" : "将工作区移到左侧"}
            aria-label={sidebarSide === "left" ? "将工作区移到右侧" : "将工作区移到左侧"}
          >
            <span className={sidebarSide === "right" ? "icon-flip" : undefined}>
              <PanelIcon size={14} />
            </span>
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
              onClick={() => switchMode("render")}
              disabled={!workspace}
            >
              章节预览
            </button>
            <button
              type="button"
              className={mode === "edit" ? "is-active" : ""}
              onClick={() => switchMode("edit")}
              disabled={!workspace || !chapter}
              title="编辑当前章节的 Markdown 源文件"
            >
              编辑{mode !== "edit" && editDirty ? " •" : ""}
            </button>
            <button
              type="button"
              className={mode === "mdbook" ? "is-active" : ""}
              onClick={() => switchMode("mdbook")}
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

      <div className={`body${sidebarSide === "right" ? " sidebar-right" : ""}`}>
        <Sidebar
          workspace={workspace}
          meta={meta}
          activeChapterPath={chapter?.path ?? null}
          chapters={chapters}
          onMetaChange={(patch) => setMeta((prev) => ({ ...prev, ...patch }))}
          onSelectFile={handleSelectFile}
          excluded={excluded}
          onToggleExclude={toggleExclude}
          onRestoreAll={restoreAllExcluded}
          onReorder={handleReorder}
          tocPage={tocPage}
          tocDepth={tocDepth}
          onTocChange={(patch) => {
            if (patch.tocPage !== undefined) setTocPage(patch.tocPage);
            if (patch.tocDepth !== undefined) setTocDepth(patch.tocDepth);
          }}
        />

        {workspace ? (
          mode === "edit" ? (
            edit ? (
              <EditorPane
                key={`${edit.path}:${editVersion}`}
                path={edit.path}
                initialRaw={edit.raw}
                saving={editSaving}
                workspaceTitle={workspace.title}
                chapterTitle={chapter?.title ?? ""}
                chapterIndex={chapterIndex}
                chapterTotal={chapters.length}
                sourceRoot={workspace.srcDir}
                sourceBase={sourceBase}
                resolveLink={resolveLink}
                onOpenLinkedFile={(path) => handleSelectFile(path, true)}
                onDirtyChange={handleEditDirty}
                onTextChange={handleEditText}
                onSaveRequest={() => void saveEdit()}
              />
            ) : (
              <section className="preview">
                <div className="preview-stage">
                  <div className="preview-placeholder">正在读取文件…</div>
                </div>
              </section>
            )
          ) : (
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
              excluded={excluded}
            />
          )
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
