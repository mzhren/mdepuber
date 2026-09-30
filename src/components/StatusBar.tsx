import type { EnvironmentInfo, Progress } from "../types";

export type StatusKind = "idle" | "busy" | "ok" | "error";

interface StatusBarProps {
  kind: StatusKind;
  message: string;
  chapters: number;
  words: number;
  environment: EnvironmentInfo | null;
  progress: Progress | null;
}

export function StatusBar({
  kind,
  message,
  chapters,
  words,
  environment,
  progress,
}: StatusBarProps) {
  const dotClass =
    kind === "busy"
      ? "status-dot is-busy"
      : kind === "ok"
        ? "status-dot is-ok"
        : kind === "error"
          ? "status-dot is-error"
          : "status-dot";

  return (
    <footer className="statusbar">
      <span className={dotClass} />
      <span
        style={{
          maxWidth: 460,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
        title={message}
      >
        {message}
      </span>

      {kind === "busy" && progress && (
        <>
          <div className="status-progress">
            <span style={{ width: `${progress.percent}%` }} />
          </div>
          <span>{progress.percent}%</span>
        </>
      )}

      <span className="status-spacer" />

      {chapters > 0 && (
        <>
          <span>{chapters} 章</span>
          <span>{words.toLocaleString("zh-CN")} 字</span>
        </>
      )}

      <span title={environment?.pdfEngine || "未检测到 Chromium 内核"}>
        PDF 引擎：{environment ? (environment.pdfAvailable ? "就绪" : "不可用") : "检测中…"}
      </span>
      <span>mdBook 0.5</span>
    </footer>
  );
}
