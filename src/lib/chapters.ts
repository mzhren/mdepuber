/**
 * Chapter ordering for the draggable outline.
 *
 * The outline is a flat list with a `level` per row. Reordering must keep the
 * tree consistent: a chapter only moves among its siblings, and moving a
 * chapter carries its nested children along — otherwise rows would silently
 * re-attach to a different parent.
 */

import type { ChapterEntry } from "../types";

export type DropPosition = "before" | "after";

/** Index just past the end of the subtree rooted at `index`. */
function subtreeEnd(chapters: ChapterEntry[], index: number): number {
  const level = chapters[index].level;
  let end = index + 1;
  while (end < chapters.length && chapters[end].level > level) end += 1;
  return end;
}

/** Path of the nearest preceding chapter with a smaller level ("" = top). */
function parentPath(chapters: ChapterEntry[], index: number): string {
  for (let i = index - 1; i >= 0; i -= 1) {
    if (chapters[i].level < chapters[index].level) return chapters[i].path;
  }
  return "";
}

function indexOfPath(chapters: ChapterEntry[], path: string): number {
  return chapters.findIndex((chapter) => chapter.path === path);
}

/**
 * Whether `dragPath` may be dropped before/after `targetPath`.
 *
 * Only sibling-to-sibling drops are valid: same level, same parent. Returns
 * the position clamped for subtree targets (dropping *after* a chapter with
 * children lands past the whole subtree, never between it and its children).
 */
export function validDropPosition(
  chapters: ChapterEntry[],
  dragPath: string,
  targetPath: string,
  afterMidpoint: boolean,
): DropPosition | null {
  if (dragPath === targetPath) return null;
  const from = indexOfPath(chapters, dragPath);
  const to = indexOfPath(chapters, targetPath);
  if (from < 0 || to < 0) return null;
  if (chapters[from].level !== chapters[to].level) return null;
  if (parentPath(chapters, from) !== parentPath(chapters, to)) return null;
  return afterMidpoint ? "after" : "before";
}

/**
 * Apply a valid drop: returns the new full order — source-relative paths, as
 * sent to the backend — with the dragged chapter (and its nested children)
 * moved as a block. `null` for invalid drops.
 */
export function reorderChapters(
  chapters: ChapterEntry[],
  relOf: (path: string) => string,
  dragPath: string,
  targetPath: string,
  position: DropPosition,
): string[] | null {
  if (validDropPosition(chapters, dragPath, targetPath, position === "after") === null) {
    return null;
  }
  const from = indexOfPath(chapters, dragPath);
  const blockEnd = subtreeEnd(chapters, from);
  const block = chapters.slice(from, blockEnd);
  const rest = [...chapters.slice(0, from), ...chapters.slice(blockEnd)];

  let insertAt = indexOfPath(rest, targetPath);
  if (insertAt < 0) return null;
  if (position === "after") {
    // Never land between a sibling and its own children.
    insertAt = subtreeEnd(rest, insertAt);
  }
  const next = [...rest.slice(0, insertAt), ...block, ...rest.slice(insertAt)];
  return next.map((chapter) => relOf(chapter.path));
}

/**
 * Sort the outline by a previous drag result (source-relative paths).
 *
 * The order list came from sibling-only moves, so it is tree-consistent; this
 * re-sorts each sibling group by the global position, preserving levels and
 * everything the user never touched.
 */
export function orderChapters(
  chapters: ChapterEntry[],
  relOf: (path: string) => string,
  order: string[] | null,
): ChapterEntry[] {
  if (!order || order.length === 0) return chapters;
  const rank = new Map(order.map((rel, index) => [rel, index]));
  if (chapters.every((chapter, index) => rank.get(relOf(chapter.path)) === index)) return chapters;

  const result: ChapterEntry[] = [];
  const build = (start: number, level: number): void => {
    const group: number[] = [];
    let i = start;
    while (i < chapters.length && chapters[i].level === level) {
      group.push(i);
      i = subtreeEnd(chapters, i);
    }
    group.sort(
      (a, b) =>
        (rank.get(relOf(chapters[a].path)) ?? Number.MAX_SAFE_INTEGER) -
        (rank.get(relOf(chapters[b].path)) ?? Number.MAX_SAFE_INTEGER),
    );
    for (const index of group) {
      result.push(chapters[index]);
      if (subtreeEnd(chapters, index) > index + 1) {
        build(index + 1, level + 1);
      }
    }
  };
  if (chapters.length > 0) build(0, chapters[0].level);
  return result;
}
