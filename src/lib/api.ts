/** Thin typed wrappers around the Tauri command bridge. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  BookMeta,
  ChapterDoc,
  ChapterEntry,
  CommandError,
  EnvironmentInfo,
  ExportReport,
  ExportRequest,
  PreviewInfo,
  Progress,
  Workspace,
} from "../types";

const PROGRESS_EVENT = "mdepub://progress";

function normaliseError(error: unknown): Error {
  if (typeof error === "string") return new Error(error);
  if (error && typeof error === "object" && "message" in error) {
    return new Error(String((error as CommandError).message));
  }
  return new Error(String(error));
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normaliseError(error);
  }
}

export const api = {
  openWorkspace: (path: string) => call<Workspace>("open_workspace", { path }),

  /** Body (front matter stripped) plus the parsed header. */
  readChapter: (path: string) => call<ChapterDoc>("read_chapter", { path }),

  defaultOutputDir: (source: string) => call<string>("default_output_dir", { source }),

  exportBook: (req: ExportRequest) => call<ExportReport>("export_book", { req }),

  buildPreview: (source: string, meta: BookMeta) =>
    call<PreviewInfo>("build_preview", { source, meta }),

  previewBase: () => call<string>("preview_base"),

  openPath: (path: string, reveal = false) => call<void>("open_path", { path, reveal }),

  environmentInfo: () => call<EnvironmentInfo>("environment_info"),

  /** Path handed to the app on the command line, if any. */
  initialPath: () => call<string | null>("initial_path"),
};

export function onProgress(handler: (progress: Progress) => void): Promise<UnlistenFn> {
  return listen<Progress>(PROGRESS_EVENT, (event) => handler(event.payload));
}

/** Chapters flattened in reading order, ignoring nesting (used for prev/next). */
export function flatChapters(workspace: Workspace | null): ChapterEntry[] {
  return workspace?.chapters ?? [];
}

export function formatBytes(bytes: number): string {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const value = bytes / 1024 ** index;
  return `${value.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(1)} s`;
}

/** Keep only the tail of a long path so it fits in a narrow sidebar. */
export function shortenPath(path: string, max = 46): string {
  if (path.length <= max) return path;
  return `…${path.slice(path.length - max + 1)}`;
}
