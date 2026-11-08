//! `mdepub-core` — Markdown to EPUB / PDF, built on mdBook.
//!
//! The pipeline is deliberately linear:
//!
//! ```text
//!   path on disk
//!        │  workspace::open      detect file / folder / mdBook project
//!        ▼
//!   Workspace (outline + tree)
//!        │  project::prepare      materialise a buildable mdBook project
//!        ▼
//!   Project
//!        │  render::build_book    mdBook loads SUMMARY.md, runs preprocessors,
//!        │                        renders HTML (also feeds the in-app preview)
//!        ▼
//!   BuiltBook ──► epub::export   mdbook-markdown -> XHTML -> epub-builder
//!             └─► pdf::export    print.html     -> headless Chromium
//! ```

pub mod chrome;
pub mod epub;
pub mod error;
pub mod frontmatter;
pub mod model;
pub mod pdf;
pub mod project;
pub mod render;
pub mod workspace;

use std::path::{Path, PathBuf};
use std::time::Instant;

pub use error::{Error, Result};
pub use model::*;
pub use project::Project;
pub use render::BuiltBook;

/// Default cache root: `<app data>/mdepuber/projects`.
///
/// The Tauri layer passes an explicit directory; this is the fallback for the
/// CLI and for tests.
pub fn default_cache_root() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("mdepuber")
        .join("projects")
}

/// Open a path and produce the workspace description used by the UI.
pub fn scan(path: &Path) -> Result<Workspace> {
    workspace::open(path)
}

/// Read a chapter's Markdown source for the preview pane.
///
/// The returned `body` never contains the front matter block; the parsed header
/// is returned alongside it so the UI can show it as metadata.
pub fn read_chapter(path: &Path) -> Result<ChapterDoc> {
    let raw = workspace::read_chapter(path)?;
    let (parsed, body) = frontmatter::split(&raw);
    Ok(ChapterDoc {
        body: body.to_string(),
        front_matter: parsed.map(|p| p.front_matter).unwrap_or_default(),
    })
}

/// Turn a title into something safe for a file name.
pub fn sanitize_file_name(input: &str) -> String {
    let trimmed = input.trim();
    let base = if trimmed.is_empty() { "book" } else { trimmed };
    let mut out = String::with_capacity(base.len());
    for c in base.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => {
                out.push('_')
            }
            c if (c as u32) < 0x20 => out.push('_'),
            c => out.push(c),
        }
    }
    let out = out.trim_matches(['.', ' ']).to_string();
    if out.is_empty() {
        "book".to_string()
    } else {
        out
    }
}

/// Which formats an export request asks for (never empty, order preserved).
fn requested_formats(req: &ExportRequest) -> Vec<BookFormat> {
    if req.formats.is_empty() {
        vec![BookFormat::Epub]
    } else {
        let mut out = Vec::new();
        for f in &req.formats {
            if !out.contains(f) {
                out.push(*f);
            }
        }
        out
    }
}

/// Overlay the caller's metadata on what we detected from the source.
///
/// The GUI pre-fills its form from the detected metadata, so "explicitly set"
/// and "detected" usually agree. The CLI, however, only sends what the user
/// passed on the command line — and an mdBook project's `book.toml` authors and
/// description have to survive that.
fn merge_meta(detected: &BookMeta, requested: &BookMeta) -> BookMeta {
    fn pick(requested: &str, default: &str, detected: &str) -> String {
        let r = requested.trim();
        let d = detected.trim();
        if r.is_empty() || (r == default && !d.is_empty()) {
            d.to_string()
        } else {
            r.to_string()
        }
    }

    fn clean(list: &[String]) -> Vec<String> {
        list.iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn pick_list(requested: &[String], detected: &[String]) -> Vec<String> {
        let r = clean(requested);
        if r.is_empty() {
            clean(detected)
        } else {
            r
        }
    }

    let defaults = BookMeta::default();
    BookMeta {
        title: pick(&requested.title, &defaults.title, &detected.title),
        authors: pick_list(&requested.authors, &detected.authors),
        language: pick(&requested.language, &defaults.language, &detected.language),
        publisher: pick(&requested.publisher, &defaults.publisher, &detected.publisher),
        description: pick(
            &requested.description,
            &defaults.description,
            &detected.description,
        ),
        subjects: pick_list(&requested.subjects, &detected.subjects),
        rights: pick(&requested.rights, &defaults.rights, &detected.rights),
        series: pick(&requested.series, &defaults.series, &detected.series),
        cover: requested
            .cover
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| detected.cover.clone()),
        identifier: requested
            .identifier
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| detected.identifier.clone()),
    }
}

/// Rescale a stage's internal 0-100 progress into `base..base + span` of the
/// overall bar, so a two-format export never reports a lower percentage after
/// a higher one.
fn scale_progress<'a>(
    base: u8,
    span: u8,
    inner: ProgressFn<'a>,
) -> Box<dyn Fn(Progress) + Send + Sync + 'a> {
    Box::new(move |mut event: Progress| {
        let local = event.percent.min(100) as u16;
        event.percent = base.saturating_add((local * span as u16 / 100) as u8);
        inner(event);
    })
}

/// Run a full export: mdBook build, then every requested format.
pub fn export_book(
    req: &ExportRequest,
    cache_root: &Path,
    progress: ProgressFn<'_>,
) -> Result<ExportReport> {
    let started = Instant::now();
    let source = PathBuf::from(&req.source);
    if !source.exists() {
        return Err(Error::NotFound(req.source.clone()));
    }

    let formats = requested_formats(req);
    let ws = workspace::open(&source)?;
    let mut warnings = ws.warnings.clone();

    let meta = merge_meta(&ws.meta, &req.meta).filled(ws.title.clone());

    let output_dir = if req.output_dir.trim().is_empty() {
        source
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(req.output_dir.trim())
    };
    std::fs::create_dir_all(&output_dir)?;

    let stem = sanitize_file_name(if req.base_name.trim().is_empty() {
        &meta.title
    } else {
        &req.base_name
    });
    let epub_path = output_dir.join(format!("{stem}.epub"));
    let pdf_path = output_dir.join(format!("{stem}.pdf"));

    let want_epub = formats.contains(&BookFormat::Epub);
    let want_pdf = formats.contains(&BookFormat::Pdf);

    let build_band = scale_progress(2, 38, progress);
    let (epub_band, pdf_band) = match (want_epub, want_pdf) {
        (true, true) => (
            scale_progress(40, 34, progress),
            scale_progress(74, 25, progress),
        ),
        (true, false) => (
            scale_progress(40, 59, progress),
            scale_progress(0, 0, progress),
        ),
        (false, true) => (
            scale_progress(0, 0, progress),
            scale_progress(40, 59, progress),
        ),
        (false, false) => (
            scale_progress(0, 0, progress),
            scale_progress(0, 0, progress),
        ),
    };

    progress(Progress::new("start", "开始转换…", 1));
    let built = render::build_book(
        &source,
        ws.kind,
        &meta,
        cache_root,
        ThemeMode::Project,
        &req.excluded,
        &req.order,
        &build_band,
    )?;
    warnings.extend(built.warnings.clone());
    warnings.extend(built.project.warnings.clone());

    let mut bytes_epub = 0u64;
    let mut bytes_pdf = 0u64;

    if want_epub {
        let w = epub::export(
            &built,
            &meta,
            &req.epub,
            req.toc_page,
            req.toc_depth,
            &epub_path,
            &epub_band,
        )?;
        warnings.extend(w);
        bytes_epub = std::fs::metadata(&epub_path).map(|m| m.len()).unwrap_or(0);
    }

    if want_pdf {
        let w = pdf::export(&built, &req.pdf, req.toc_page, req.toc_depth, &pdf_path, &pdf_band)?;
        warnings.extend(w);
        bytes_pdf = std::fs::metadata(&pdf_path).map(|m| m.len()).unwrap_or(0);
    }

    progress(Progress::new("done", "完成", 100));

    // De-duplicate warnings, keeping the first occurrence.
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|w| seen.insert(w.clone()));

    Ok(ExportReport {
        epub_path: want_epub.then(|| epub_path.display().to_string()),
        pdf_path: want_pdf.then(|| pdf_path.display().to_string()),
        html_dir: built.html_dir.display().to_string(),
        chapter_count: built.chapter_count(),
        bytes_epub,
        bytes_pdf,
        duration_ms: started.elapsed().as_millis(),
        warnings,
    })
}

/// Build the HTML preview for a source and return the built book.
///
/// Used by the GUI's "mdBook 预览" tab.
pub fn build_preview(
    source: &Path,
    meta: &BookMeta,
    cache_root: &Path,
    excluded: &[String],
    order: &[String],
    progress: ProgressFn<'_>,
) -> Result<BuiltBook> {
    let ws = workspace::open(source)?;
    let meta = meta.clone().filled(ws.title);
    render::build_book(
        source,
        ws.kind,
        &meta,
        cache_root,
        ThemeMode::FollowSystem,
        excluded,
        order,
        progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_sanitised() {
        assert_eq!(sanitize_file_name("我的书: 第一版/草稿"), "我的书_ 第一版_草稿");
        assert_eq!(sanitize_file_name("   "), "book");
        assert_eq!(sanitize_file_name(".."), "book");
    }

    #[test]
    fn default_formats_is_epub() {
        let req = ExportRequest::default();
        assert_eq!(requested_formats(&req), vec![BookFormat::Epub]);
    }

    #[test]
    fn detected_book_toml_metadata_survives_an_empty_request() {
        let detected = BookMeta {
            title: "项目书名".into(),
            authors: vec!["项目作者".into()],
            language: "en".into(),
            description: "来自 book.toml".into(),
            ..BookMeta::default()
        };
        // What the CLI sends when the user passes no metadata flags.
        let requested = BookMeta::default();

        let merged = merge_meta(&detected, &requested);
        assert_eq!(merged.title, "项目书名");
        assert_eq!(merged.authors, vec!["项目作者".to_string()]);
        assert_eq!(merged.language, "en");
        assert_eq!(merged.description, "来自 book.toml");
    }

    #[test]
    fn explicit_request_overrides_detected_metadata() {
        let detected = BookMeta {
            title: "旧书名".into(),
            authors: vec!["旧作者".into()],
            language: "en".into(),
            ..BookMeta::default()
        };
        let requested = BookMeta {
            title: "新书名".into(),
            authors: vec!["新作者".into()],
            language: "ja".into(),
            ..BookMeta::default()
        };

        let merged = merge_meta(&detected, &requested);
        assert_eq!(merged.title, "新书名");
        assert_eq!(merged.authors, vec!["新作者".to_string()]);
        assert_eq!(merged.language, "ja");
    }
}
