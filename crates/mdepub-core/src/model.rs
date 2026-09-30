//! Plain data types shared between the Rust core, the Tauri commands and the web UI.
//!
//! Everything here is `serde`-friendly and uses `camelCase` on the wire so the
//! TypeScript side can consume it without adapters.

use serde::{Deserialize, Serialize};

/// What kind of thing the user opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// A single `.md` file.
    SingleFile,
    /// A folder of Markdown files (synthesised into a book).
    Folder,
    /// A real mdBook project (`book.toml` + `src/SUMMARY.md`).
    MdBook,
}

/// A single key/value pair from a front matter block, kept in file order so the
/// UI can show the header exactly as the author wrote it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontMatterField {
    pub key: String,
    pub value: String,
}

/// A parsed YAML (`---`) or TOML (`+++`) front matter block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FrontMatter {
    /// `"yaml"`, `"toml"` or `""` when the document has no front matter.
    pub format: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub date: Option<String>,
    /// Cover/hero image, relative to the document unless absolute.
    pub image: Option<String>,
    pub tags: Vec<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub draft: bool,
    /// Every top-level key, in file order.
    pub fields: Vec<FrontMatterField>,
}

impl FrontMatter {
    pub fn is_present(&self) -> bool {
        !self.format.is_empty()
    }

    /// Fields worth showing as a strip above the preview: skip the ones already
    /// rendered elsewhere, and skip empty values.
    pub fn highlights(&self) -> Vec<FrontMatterField> {
        self.fields
            .iter()
            .filter(|f| !f.value.trim().is_empty())
            .filter(|f| !["title", "description"].iter().any(|k| f.key.eq_ignore_ascii_case(k)))
            .cloned()
            .collect()
    }
}

/// A chapter as read for the preview pane: the body plus its parsed header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterDoc {
    /// Markdown body with the front matter block removed.
    pub body: String,
    pub front_matter: FrontMatter,
}

/// One entry in the book outline / workspace tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterEntry {
    /// Stable id (relative source path).
    pub id: String,
    /// Display title (from `SUMMARY.md`, the first `#` heading, or the filename).
    pub title: String,
    /// Absolute path on disk.
    pub path: String,
    /// Path relative to the book source directory.
    pub rel: String,
    /// Nesting depth, starting at 1.
    pub level: u32,
    /// Rough word/character count for the status bar.
    pub words: usize,
}

/// A file or folder inside the opened workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_markdown: bool,
    pub in_book: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FsNode>,
}

/// Result of opening a path: the outline plus the file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    /// Absolute path the user opened.
    pub root: String,
    /// For single-file mode this is the file, otherwise the folder.
    pub kind: SourceKind,
    /// Detected title (mdBook `book.toml`, first chapter, or folder name).
    pub title: String,
    /// Absolute source directory that holds the Markdown.
    pub src_dir: String,
    /// Flat, ordered outline used for the preview and for exports.
    pub chapters: Vec<ChapterEntry>,
    /// Tree used by the left sidebar.
    pub tree: Vec<FsNode>,
    /// Non-fatal notes (skipped files, dropped preprocessors, ...).
    pub warnings: Vec<String>,
    /// Detected book metadata, used to pre-fill the export dialog.
    pub meta: BookMeta,
    /// Total characters across all chapters (status bar).
    pub total_words: usize,
}

/// Bibliographic metadata for the exported e-book.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BookMeta {
    pub title: String,
    pub authors: Vec<String>,
    pub language: String,
    pub publisher: String,
    pub description: String,
    pub subjects: Vec<String>,
    pub rights: String,
    /// Absolute path to a cover image, if any.
    pub cover: Option<String>,
    /// ISBN / UUID / other identifier.
    pub identifier: Option<String>,
    /// Series name, EPUB3 `belongs-to-collection`.
    pub series: String,
}

impl Default for BookMeta {
    fn default() -> Self {
        Self {
            title: String::new(),
            authors: Vec::new(),
            language: "zh-CN".to_string(),
            publisher: String::new(),
            description: String::new(),
            subjects: Vec::new(),
            rights: String::new(),
            cover: None,
            identifier: None,
            series: String::new(),
        }
    }
}

impl BookMeta {
    /// Fill in sane defaults from a detected title, keeping anything already set.
    pub fn filled(mut self, title: impl Into<String>) -> Self {
        if self.title.trim().is_empty() {
            self.title = title.into();
        }
        if self.language.trim().is_empty() {
            self.language = "zh-CN".to_string();
        }
        if self.authors.is_empty() || self.authors.iter().all(|a| a.trim().is_empty()) {
            self.authors = Vec::new();
        }
        self
    }

    /// Authors joined for display, `未知作者` when empty.
    pub fn author_line(&self) -> String {
        let joined = self
            .authors
            .iter()
            .map(|a| a.trim())
            .filter(|a| !a.is_empty())
            .collect::<Vec<_>>()
            .join("、");
        if joined.is_empty() {
            "未知作者".to_string()
        } else {
            joined
        }
    }
}

/// Output format selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BookFormat {
    Epub,
    Pdf,
}

/// EPUB-specific knobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EpubOptions {
    /// `"3"` (default) or `"2"`.
    pub version: String,
    /// `"serif" | "sans" | "system"` — selects the bundled stylesheet flavour.
    pub font_family: String,
    /// Base font size in `em`, applied to `body`.
    pub font_size: f32,
    /// Line height multiplier.
    pub line_height: f32,
    /// `"ltr" | "rtl" | "default"`.
    pub page_direction: String,
    /// Generate a title page before the first chapter.
    pub title_page: bool,
    /// Generate a "cover page" referencing the cover image.
    pub cover_page: bool,
    /// Split the book into one XHTML file per top-level entry.
    pub split_by_chapter: bool,
    /// Extra CSS appended after the built-in stylesheet.
    pub extra_css: String,
}

impl Default for EpubOptions {
    fn default() -> Self {
        Self {
            version: "3".into(),
            font_family: "serif".into(),
            font_size: 1.0,
            line_height: 1.6,
            page_direction: "default".into(),
            title_page: true,
            cover_page: true,
            split_by_chapter: true,
            extra_css: String::new(),
        }
    }
}

/// PDF-specific knobs (forwarded to the headless browser print pipeline).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PdfOptions {
    /// `A4 | A5 | B5 | Letter | Legal | A3 | custom`
    pub page_size: String,
    pub custom_width_mm: f32,
    pub custom_height_mm: f32,
    pub margin_mm: f32,
    /// `light | dark | sepia`
    pub theme: String,
    pub header_footer: bool,
    pub print_background: bool,
    /// `0.5 - 2.0`
    pub scale: f32,
    /// Extra CSS appended inside the `@media print` block.
    pub extra_css: String,
    /// Generate a PDF outline (bookmarks) from the heading structure.
    pub outline: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            page_size: "A4".into(),
            custom_width_mm: 148.0,
            custom_height_mm: 210.0,
            margin_mm: 18.0,
            theme: "light".into(),
            header_footer: false,
            print_background: true,
            scale: 1.0,
            extra_css: String::new(),
            outline: true,
        }
    }
}

/// Everything needed for one export run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportRequest {
    /// Absolute path of the file/folder to convert.
    pub source: String,
    pub meta: BookMeta,
    pub epub: EpubOptions,
    pub pdf: PdfOptions,
    pub formats: Vec<BookFormat>,
    /// Insert a visible table-of-contents page at the very front of the book
    /// (right after the cover) in addition to the reader's built-in navigation.
    pub toc_page: bool,
    /// Heading levels the TOC page should list (1 = chapters only).
    pub toc_depth: u32,
    /// Directory the e-books are written into.
    pub output_dir: String,
    /// File stem, without extension.
    pub base_name: String,
}

impl Default for ExportRequest {
    fn default() -> Self {
        Self {
            source: String::new(),
            meta: BookMeta::default(),
            epub: EpubOptions::default(),
            pdf: PdfOptions::default(),
            formats: vec![BookFormat::Epub],
            toc_page: false,
            toc_depth: 3,
            output_dir: String::new(),
            base_name: "book".into(),
        }
    }
}

/// What an export produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub epub_path: Option<String>,
    pub pdf_path: Option<String>,
    pub html_dir: String,
    pub chapter_count: usize,
    pub bytes_epub: u64,
    pub bytes_pdf: u64,
    pub duration_ms: u128,
    pub warnings: Vec<String>,
}

/// Progress event pushed to the UI while building.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Machine-readable stage id.
    pub stage: String,
    /// Human-readable (Chinese) description.
    pub message: String,
    /// 0-100.
    pub percent: u8,
    /// Current step / total steps when applicable.
    pub step: Option<usize>,
    pub total_steps: Option<usize>,
}

impl Progress {
    pub fn new(stage: &str, message: impl Into<String>, percent: u8) -> Self {
        Self {
            stage: stage.to_string(),
            message: message.into(),
            percent,
            step: None,
            total_steps: None,
        }
    }

    pub fn with_steps(mut self, step: usize, total: usize) -> Self {
        self.step = Some(step);
        self.total_steps = Some(total);
        self
    }
}

/// How the generated mdBook HTML should choose its own colour scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Keep whatever the project configured. Used for exports, where the print
    /// stylesheet decides the colours anyway.
    #[default]
    Project,
    /// Force mdBook's built-in light theme with `navy` as the dark counterpart,
    /// so mdBook keys its theme off `prefers-color-scheme`. The desktop app
    /// mirrors its own theme onto the window, which is what makes the in-app
    /// preview follow the light/dark toggle.
    FollowSystem,
}

/// Callback used by long-running operations to report progress.
pub type ProgressFn<'a> = &'a (dyn Fn(Progress) + Send + Sync);

/// No-op reporter.
pub fn silent(_: Progress) {}
