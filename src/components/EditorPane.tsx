import { useEffect, useMemo, useRef, useState } from "react";
import { EditorState, Prec } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
  crosshairCursor,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import {
  bracketMatching,
  foldGutter,
  foldKeymap,
  HighlightStyle,
  indentOnInput,
  indentUnit,
  syntaxHighlighting,
} from "@codemirror/language";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { tags } from "@lezer/highlight";

import { countWords, splitFrontMatter } from "../lib/markdown";
import { RenderedBody } from "./RenderedBody";
import { SaveIcon } from "./Icons";

export interface EditorPaneProps {
  /** Absolute file path being edited (used for asset URL rewriting). */
  path: string;
  /** File contents as last loaded / saved — the dirty baseline. */
  initialRaw: string;
  saving: boolean;
  workspaceTitle: string;
  chapterTitle: string;
  chapterIndex: number;
  chapterTotal: number;
  sourceRoot: string;
  sourceBase: string;
  resolveLink: (href: string) => string | null;
  onOpenLinkedFile: (path: string) => void;
  /** Dirty-flag transitions only (not on every keystroke). */
  onDirtyChange: (dirty: boolean) => void;
  /** Fires on every document change so the parent can save the latest text. */
  onTextChange: (text: string) => void;
  onSaveRequest: () => void;
}

/**
 * Syntax colours for the Markdown buffer, expressed in the app's CSS variables
 * so the editor follows the light/dark switch without a JS-side retheme.
 */
const mdHighlightStyle = HighlightStyle.define([
  { tag: tags.heading1, fontSize: "1.3em", fontWeight: "700", color: "var(--text)" },
  { tag: tags.heading2, fontSize: "1.18em", fontWeight: "700", color: "var(--text)" },
  { tag: [tags.heading3, tags.heading4], fontSize: "1.06em", fontWeight: "700", color: "var(--text)" },
  { tag: [tags.heading5, tags.heading6], fontWeight: "700", color: "var(--text)" },
  { tag: tags.strong, fontWeight: "700", color: "var(--text)" },
  { tag: tags.emphasis, fontStyle: "italic", color: "var(--text)" },
  { tag: tags.strikethrough, textDecoration: "line-through", color: "var(--text-tertiary)" },
  { tag: tags.link, color: "var(--accent)", textDecoration: "underline" },
  { tag: tags.url, color: "var(--text-tertiary)" },
  { tag: tags.monospace, color: "var(--warning)" },
  { tag: tags.quote, color: "var(--text-secondary)", fontStyle: "italic" },
  { tag: tags.contentSeparator, color: "var(--accent)", fontWeight: "700" },
  { tag: [tags.processingInstruction, tags.meta], color: "var(--text-tertiary)" },
]);

const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    backgroundColor: "var(--bg-input)",
    color: "var(--text)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily:
      'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
    fontSize: "13px",
    lineHeight: "1.75",
    padding: "12px 0 45vh",
  },
  ".cm-content": { caretColor: "var(--accent)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "var(--accent-soft)",
  },
  ".cm-gutters": {
    backgroundColor: "var(--bg-chrome)",
    color: "var(--text-tertiary)",
    border: "none",
  },
  ".cm-activeLine": { backgroundColor: "var(--bg-hover)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--bg-hover)" },
  ".cm-selectionMatch": { backgroundColor: "var(--accent-soft)" },
  ".cm-searchMatch": {
    backgroundColor: "color-mix(in srgb, var(--warning) 30%, transparent)",
    outline: "1px solid var(--warning)",
  },
  ".cm-searchMatch.cm-searchMatch-selected": {
    backgroundColor: "color-mix(in srgb, var(--warning) 55%, transparent)",
  },
  ".cm-panels": {
    backgroundColor: "var(--bg-panel)",
    color: "var(--text)",
    borderBottom: "1px solid var(--border)",
  },
  ".cm-textfield": {
    backgroundColor: "var(--bg-input)",
    color: "var(--text)",
    border: "1px solid var(--border-strong)",
    borderRadius: 6,
  },
  ".cm-button": {
    backgroundColor: "var(--bg-elevated)",
    color: "var(--text)",
    border: "1px solid var(--border-strong)",
    borderRadius: 6,
  },
  ".cm-foldPlaceholder": {
    backgroundColor: "var(--bg-active)",
    color: "var(--text-tertiary)",
    border: "none",
    borderRadius: 4,
  },
  ".cm-tooltip": {
    backgroundColor: "var(--bg-elevated)",
    color: "var(--text)",
    border: "1px solid var(--border-strong)",
  },
});

/**
 * Markdown editing: CodeMirror buffer on the left, a live-rendered preview
 * (front matter hidden) on the right. The buffer is uncontrolled — the parent
 * receives text through `onTextChange` and dirty transitions through
 * `onDirtyChange`, so typing never re-renders the whole app.
 */
export function EditorPane({
  path,
  initialRaw,
  saving,
  workspaceTitle,
  chapterTitle,
  chapterIndex,
  chapterTotal,
  sourceRoot,
  sourceBase,
  resolveLink,
  onOpenLinkedFile,
  onDirtyChange,
  onTextChange,
  onSaveRequest,
}: EditorPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const baselineRef = useRef(initialRaw);
  const dirtyRef = useRef(false);
  const debounceRef = useRef<number | undefined>(undefined);
  const [dirty, setDirtyLocal] = useState(false);
  const [previewBody, setPreviewBody] = useState(() => splitFrontMatter(initialRaw).body);

  // Keep the latest callbacks reachable from the (mount-once) editor setup
  // without recreating the CodeMirror state.
  const callbacks = useRef({ onDirtyChange, onTextChange, onSaveRequest });
  callbacks.current = { onDirtyChange, onTextChange, onSaveRequest };

  const reportDirty = (next: boolean) => {
    if (dirtyRef.current === next) return;
    dirtyRef.current = next;
    setDirtyLocal(next);
    callbacks.current.onDirtyChange(next);
  };

  useEffect(() => {
    const schedulePreview = () => {
      window.clearTimeout(debounceRef.current);
      debounceRef.current = window.setTimeout(() => {
        const text = viewRef.current?.state.doc.toString() ?? "";
        setPreviewBody(splitFrontMatter(text).body);
      }, 250);
    };

    const view = new EditorView({
      parent: hostRef.current!,
      state: EditorState.create({
        doc: initialRaw,
        extensions: [
          lineNumbers(),
          highlightActiveLineGutter(),
          highlightSpecialChars(),
          history(),
          foldGutter(),
          drawSelection(),
          dropCursor(),
          EditorState.allowMultipleSelections.of(true),
          indentOnInput(),
          indentUnit.of("  "),
          syntaxHighlighting(mdHighlightStyle, { fallback: true }),
          bracketMatching(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          rectangularSelection(),
          crosshairCursor(),
          EditorView.lineWrapping,
          markdown({ base: markdownLanguage }),
          search({ top: true }),
          Prec.highest(
            keymap.of([
              {
                key: "Mod-s",
                run: () => {
                  callbacks.current.onSaveRequest();
                  return true;
                },
              },
            ]),
          ),
          keymap.of([...foldKeymap, ...historyKeymap, ...searchKeymap, ...defaultKeymap, indentWithTab]),
          editorTheme,
          EditorView.updateListener.of((update) => {
            if (!update.docChanged) return;
            const text = update.state.doc.toString();
            callbacks.current.onTextChange(text);
            reportDirty(text !== baselineRef.current);
            schedulePreview();
          }),
        ],
      }),
    });
    viewRef.current = view;
    view.focus();
    return () => {
      window.clearTimeout(debounceRef.current);
      view.destroy();
      viewRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // After a save the parent hands back the persisted text — move the dirty
  // baseline so the buffer reads clean again.
  useEffect(() => {
    baselineRef.current = initialRaw;
    reportDirty((viewRef.current?.state.doc.toString() ?? initialRaw) !== initialRaw);
  }, [initialRaw]);

  const words = useMemo(() => countWords(previewBody), [previewBody]);

  return (
    <section className="preview">
      <div className="preview-bar">
        <div className="preview-crumbs">
          <strong>{workspaceTitle}</strong>
          <span className="sep">/</span>
          {chapterTitle}
          {chapterTotal > 0 && (
            <>
              <span className="sep">·</span>
              {chapterIndex + 1} / {chapterTotal}
            </>
          )}
          {dirty && (
            <span className="crumb-badge" title="有未保存到磁盘的修改">
              未保存
            </span>
          )}
        </div>

        <div className="titlebar-group">
          <span className="editor-words" title="实时预览正文字数（不含 front matter）">
            {words.toLocaleString("zh-CN")} 字
          </span>
          <button
            type="button"
            className="btn btn-primary btn-sm"
            onClick={onSaveRequest}
            disabled={!dirty || saving}
            title="保存到文件（⌘S）"
          >
            <SaveIcon size={12} />
            {saving ? "保存中…" : "保存"}
          </button>
        </div>
      </div>

      <div className="preview-stage">
        <div className="editor-split">
          <div className="editor-pane">
            <div className="editor-host" ref={hostRef} />
          </div>
          <div className="editor-preview">
            <RenderedBody
              body={previewBody}
              chapterPath={path}
              sourceRoot={sourceRoot}
              sourceBase={sourceBase}
              resolveLink={resolveLink}
              onOpenLinkedFile={onOpenLinkedFile}
            />
          </div>
        </div>
      </div>
    </section>
  );
}
