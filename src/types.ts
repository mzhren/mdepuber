/**
 * Wire types mirroring `mdepub-core::model`.
 *
 * Rust serialises with `#[serde(rename_all = "camelCase")]`, so these line up
 * one-to-one with the structs on the other side.
 */

export type SourceKind = "singleFile" | "folder" | "mdBook";
export type BookFormat = "epub" | "pdf";

export interface FrontMatterField {
  key: string;
  value: string;
}

/** A parsed YAML (`---`) or TOML (`+++`) header block. */
export interface FrontMatter {
  /** `"yaml"`, `"toml"`, or `""` when the document has no header. */
  format: string;
  title: string | null;
  description: string | null;
  date: string | null;
  image: string | null;
  tags: string[];
  authors: string[];
  language: string | null;
  draft: boolean;
  /** Every top-level key, in file order. */
  fields: FrontMatterField[];
}

export const emptyFrontMatter = (): FrontMatter => ({
  format: "",
  title: null,
  description: null,
  date: null,
  image: null,
  tags: [],
  authors: [],
  language: null,
  draft: false,
  fields: [],
});

/** One chapter as delivered to the preview: body + parsed header. */
export interface ChapterDoc {
  body: string;
  frontMatter: FrontMatter;
}

export interface ChapterEntry {
  id: string;
  title: string;
  path: string;
  rel: string;
  level: number;
  words: number;
}

export interface FsNode {
  name: string;
  path: string;
  isDir: boolean;
  isMarkdown: boolean;
  inBook: boolean;
  children?: FsNode[];
}

export interface BookMeta {
  title: string;
  authors: string[];
  language: string;
  publisher: string;
  description: string;
  subjects: string[];
  rights: string;
  cover: string | null;
  identifier: string | null;
  series: string;
}

export interface Workspace {
  root: string;
  kind: SourceKind;
  title: string;
  srcDir: string;
  chapters: ChapterEntry[];
  tree: FsNode[];
  warnings: string[];
  meta: BookMeta;
  totalWords: number;
}

export interface EpubOptions {
  version: string;
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  pageDirection: string;
  titlePage: boolean;
  coverPage: boolean;
  splitByChapter: boolean;
  extraCss: string;
}

export interface PdfOptions {
  pageSize: string;
  customWidthMm: number;
  customHeightMm: number;
  marginMm: number;
  theme: string;
  headerFooter: boolean;
  printBackground: boolean;
  scale: number;
  extraCss: string;
  outline: boolean;
}

export interface ExportRequest {
  source: string;
  meta: BookMeta;
  epub: EpubOptions;
  pdf: PdfOptions;
  formats: BookFormat[];
  /**
   * Chapter paths (relative to the book source dir) to leave out of the book.
   * Files stay on disk; they are dropped at render time.
   */
  excluded: string[];
  /**
   * Chapter paths in the order arranged by dragging the outline. Empty keeps
   * the natural order; applied at render time, files on disk are untouched.
   */
  order: string[];
  /** Insert a visible contents page at the front of the book. */
  tocPage: boolean;
  /** How many heading levels the contents page lists (1-3). */
  tocDepth: number;
  outputDir: string;
  baseName: string;
}

export interface ExportReport {
  epubPath: string | null;
  pdfPath: string | null;
  htmlDir: string;
  chapterCount: number;
  bytesEpub: number;
  bytesPdf: number;
  durationMs: number;
  warnings: string[];
}

export interface Progress {
  stage: string;
  message: string;
  percent: number;
  step: number | null;
  totalSteps: number | null;
}

export interface PreviewInfo {
  baseUrl: string;
  indexUrl: string;
  htmlDir: string;
  chapterCount: number;
  warnings: string[];
}

export interface EnvironmentInfo {
  os: string;
  arch: string;
  appVersion: string;
  pdfEngine: string;
  pdfAvailable: boolean;
}

export interface CommandError {
  code: string;
  message: string;
}

export const defaultBookMeta = (): BookMeta => ({
  title: "",
  authors: [],
  language: "zh-CN",
  publisher: "",
  description: "",
  subjects: [],
  rights: "",
  cover: null,
  identifier: null,
  series: "",
});

export const defaultEpubOptions = (): EpubOptions => ({
  version: "3",
  fontFamily: "serif",
  fontSize: 1,
  lineHeight: 1.6,
  pageDirection: "default",
  titlePage: true,
  coverPage: true,
  splitByChapter: true,
  extraCss: "",
});

export const defaultPdfOptions = (): PdfOptions => ({
  pageSize: "A4",
  customWidthMm: 148,
  customHeightMm: 210,
  marginMm: 18,
  theme: "light",
  headerFooter: false,
  printBackground: true,
  scale: 1,
  extraCss: "",
  outline: true,
});

export const pdfPageSizes = ["A3", "A4", "A5", "B5", "Letter", "Legal", "custom"] as const;
