//! Turning an arbitrary Markdown source into a *buildable mdBook project*.
//!
//! Two very different situations have to look the same to the rest of the
//! pipeline:
//!
//! * the user already has an mdBook project (`book.toml` + `src/SUMMARY.md`) —
//!   we build it in place and only redirect the build directory into our cache;
//! * the user has a loose `.md` file or a folder of them — we materialise a
//!   throw-away mdBook project inside the app cache, with a generated
//!   `SUMMARY.md`, and never touch the originals.

use std::path::{Path, PathBuf};

use mdbook_core::config::Config;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::model::{BookMeta, SourceKind, ThemeMode};
use crate::workspace;

/// Identifies directories that should never be copied into a synthetic project.
const COPY_SKIP_DIRS: &[&str] = &[
    ".git", ".svn", ".hg", "node_modules", "target", "book", "dist", ".cargo-home", ".venv",
    "__pycache__", ".idea", ".vscode", ".DS_Store",
];

/// Copy guard rails so a mis-picked folder cannot fill the disk.
const MAX_COPY_FILES: usize = 6000;
const MAX_COPY_BYTES: u64 = 512 * 1024 * 1024;

/// A prepared, buildable mdBook project.
#[derive(Debug, Clone)]
pub struct Project {
    /// Stable hash of the original source path.
    pub id: String,
    /// The path the user opened.
    pub source: PathBuf,
    pub kind: SourceKind,
    /// mdBook root (`book.toml` lives here).
    pub root: PathBuf,
    /// mdBook source directory (`SUMMARY.md` lives here).
    pub src_dir: PathBuf,
    /// Directory mdBook writes HTML into (`<build_dir>/html`).
    pub build_dir: PathBuf,
    /// Scratch space for generated CSS, caches, … for this project.
    pub cache_dir: PathBuf,
    /// True when `root` is the user's own project directory.
    pub in_place: bool,
    pub warnings: Vec<String>,
}

impl Project {
    /// Where the HTML renderer placed its output.
    ///
    /// mdBook nests renderer output in `<build_dir>/<renderer>` only when more
    /// than one renderer is configured; with our single HTML renderer it writes
    /// straight into `<build_dir>`. Check both so the pipeline never depends on
    /// that implementation detail.
    pub fn html_dir(&self) -> PathBuf {
        let nested = self.build_dir.join("html");
        if nested.join("index.html").is_file() {
            nested
        } else {
            self.build_dir.clone()
        }
    }

    /// The single-page "print" view produced by mdBook, used for PDF export.
    pub fn print_html(&self) -> PathBuf {
        self.html_dir().join("print.html")
    }
}

/// Stable, filesystem-safe id for a source path.
pub fn project_id(source: &Path) -> String {
    let canonical = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..8])
}

/// Prepare (and where needed, materialise) a buildable project.
///
/// `cache_root` is typically `~/Library/Application Support/<app>/projects`.
pub fn prepare(
    source: &Path,
    kind: SourceKind,
    meta: &BookMeta,
    cache_root: &Path,
) -> Result<Project> {
    // `meta` only feeds the render configuration; it is applied in `config_for`.
    let _ = meta;
    let source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let id = project_id(&source);
    let cache_dir = cache_root.join(&id);

    match kind {
        SourceKind::MdBook => prepare_in_place(source, id, cache_dir),
        SourceKind::Folder => {
            let mut warnings = Vec::new();
            let root = cache_dir.join("project");
            let src_dir = root.join("src");
            reset_dir(&root)?;
            std::fs::create_dir_all(&src_dir)?;
            copy_tree(&source, &src_dir, &mut warnings)?;
            if !workspace::contains_markdown(&src_dir) {
                return Err(Error::NoChapters(source.display().to_string()));
            }
            write_generated_summary(&src_dir, None, &mut warnings)?;
            Ok(Project {
                id,
                source,
                kind,
                root,
                src_dir,
                build_dir: cache_dir.join("book"),
                cache_dir,
                in_place: false,
                warnings,
            })
        }
        SourceKind::SingleFile => {
            let mut warnings = Vec::new();
            let root = cache_dir.join("project");
            let src_dir = root.join("src");
            reset_dir(&root)?;
            std::fs::create_dir_all(&src_dir)?;

            let file_name = source
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("index.md")
                .to_string();
            std::fs::copy(&source, src_dir.join(&file_name))?;

            // Pull in sibling assets (images, fonts, ...) so relative links in
            // the document keep working, without dragging in a whole tree.
            if let Some(parent) = source.parent() {
                copy_assets(parent, &src_dir, &file_name, &mut warnings)?;
            }
            write_generated_summary(&src_dir, Some(&file_name), &mut warnings)?;
            Ok(Project {
                id,
                source,
                kind,
                root,
                src_dir,
                build_dir: cache_dir.join("book"),
                cache_dir,
                in_place: false,
                warnings,
            })
        }
    }
}

fn prepare_in_place(source: PathBuf, id: String, cache_dir: PathBuf) -> Result<Project> {
    let mut warnings = Vec::new();
    let config = Config::from_disk(source.join("book.toml")).map_err(Error::MdBook)?;
    let src_dir = source.join(&config.book.src);
    if !src_dir.is_dir() {
        return Err(Error::Other(format!(
            "book.toml 中 book.src 指向的目录不存在：{}",
            src_dir.display()
        )));
    }
    if !src_dir.join("SUMMARY.md").is_file() {
        return Err(Error::Other(format!(
            "mdBook 项目缺少 src/SUMMARY.md：{}",
            src_dir.display()
        )));
    }
    std::fs::create_dir_all(&cache_dir)?;
    warnings.push("将以“就地构建”方式使用原项目，构建产物写入应用缓存目录。".to_string());
    Ok(Project {
        id,
        source: source.clone(),
        kind: SourceKind::MdBook,
        root: source,
        src_dir,
        build_dir: cache_dir.join("book"),
        cache_dir,
        in_place: true,
        warnings,
    })
}

/// Build the mdBook [`Config`] this project should be rendered with.
///
/// Third-party renderers and preprocessors are dropped: they need binaries we
/// cannot ship, and mdBook hard-fails when they are missing.
pub fn config_for(
    project: &Project,
    meta: &BookMeta,
    theme_mode: ThemeMode,
) -> Result<(Config, Vec<String>)> {
    let mut warnings = Vec::new();

    let base = if project.root.join("book.toml").is_file() {
        Config::from_disk(project.root.join("book.toml")).map_err(Error::MdBook)?
    } else {
        Config::default()
    };

    let mut clean = Config::default();
    clean.book = base.book.clone();
    clean.build = base.build.clone();
    clean.rust = base.rust.clone();

    if let Ok(outputs) = base.outputs::<toml::Value>() {
        for name in outputs.keys() {
            if name != "html" {
                warnings.push(format!(
                    "已忽略 [output.{name}]：mdepuber 内置 HTML / EPUB / PDF 三种输出。"
                ));
            }
        }
    }
    if let Ok(Some(html)) = base.get::<toml::Value>("output.html") {
        clean.set("output.html", html).map_err(Error::MdBook)?;
    }

    if let Ok(preprocessors) = base.preprocessors::<toml::Value>() {
        for (name, value) in preprocessors {
            if name == "links" || name == "index" {
                clean
                    .set(format!("preprocessor.{name}"), value)
                    .map_err(Error::MdBook)?;
            } else {
                warnings.push(format!(
                    "已忽略 [preprocessor.{name}]：外部预处理器未随应用分发。"
                ));
            }
        }
    }

    // Metadata from the UI wins over whatever was on disk.
    if !meta.title.trim().is_empty() {
        clean.book.title = Some(meta.title.trim().to_string());
    } else if clean.book.title.is_none() {
        clean.book.title = Some("未命名书籍".to_string());
    }
    let authors: Vec<String> = meta
        .authors
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    if !authors.is_empty() {
        clean.book.authors = authors;
    }
    if !meta.language.trim().is_empty() {
        clean.book.language = Some(meta.language.trim().to_string());
    }
    if !meta.description.trim().is_empty() {
        clean.book.description = Some(meta.description.trim().to_string());
    }

    // The in-app preview should follow the app's own light/dark toggle rather
    // than whatever the project pinned in `book.toml`.
    if theme_mode == ThemeMode::FollowSystem {
        clean
            .set("output.html.default-theme", "light")
            .map_err(Error::MdBook)?;
        clean
            .set("output.html.preferred-dark-theme", "navy")
            .map_err(Error::MdBook)?;
    }

    // Always build out of tree so the user's project stays clean.
    clean.build.build_dir = project.build_dir.clone();
    clean.build.create_missing = false;

    Ok((clean, warnings))
}

fn reset_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    Ok(())
}

fn should_skip(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') {
        return true;
    }
    if entry.file_type().is_dir() && COPY_SKIP_DIRS.contains(&name.as_ref()) {
        return true;
    }
    false
}

struct CopyBudget {
    files: usize,
    bytes: u64,
    exhausted: bool,
}

impl CopyBudget {
    fn new() -> Self {
        Self {
            files: 0,
            bytes: 0,
            exhausted: false,
        }
    }

    fn take(&mut self, len: u64) -> bool {
        if self.exhausted {
            return false;
        }
        if self.files + 1 > MAX_COPY_FILES || self.bytes + len > MAX_COPY_BYTES {
            self.exhausted = true;
            return false;
        }
        self.files += 1;
        self.bytes += len;
        true
    }
}

/// Recursively copy the source tree into the synthetic `src/` directory.
fn copy_tree(from: &Path, to: &Path, warnings: &mut Vec<String>) -> Result<()> {
    let mut budget = CopyBudget::new();
    for entry in WalkDir::new(from)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(from)
            .unwrap_or(entry.path())
            .to_path_buf();
        let target = to.join(&rel);
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if !budget.take(len) {
            warnings.push(format!(
                "源目录过大，已跳过其余文件（上限 {} 个 / {} MB）。",
                MAX_COPY_FILES,
                MAX_COPY_BYTES / 1024 / 1024
            ));
            break;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(entry.path(), &target)?;
    }
    Ok(())
}

/// Copy non-Markdown siblings (images etc.) next to a single opened file.
fn copy_assets(
    from: &Path,
    to: &Path,
    skip_name: &str,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let mut budget = CopyBudget::new();
    for entry in WalkDir::new(from)
        .follow_links(false)
        .max_depth(2)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == skip_name || workspace::is_markdown(entry.path()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(from)
            .unwrap_or(entry.path())
            .to_path_buf();
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if !budget.take(len) {
            warnings.push("同级资源文件过多，已跳过部分图片/附件。".to_string());
            break;
        }
        let target = to.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(entry.path(), &target)?;
    }
    Ok(())
}

/// Write a `SUMMARY.md` for a synthetic project.
///
/// When `single` is set the book contains exactly that one file, otherwise the
/// existing `SUMMARY.md` (if the user shipped one) is respected and a generated
/// one is only produced as a fallback.
fn write_generated_summary(
    src_dir: &Path,
    single: Option<&str>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let summary_path = src_dir.join("SUMMARY.md");

    if let Some(file) = single {
        let title = workspace::read_chapter(&src_dir.join(file))
            .map(|t| workspace::title_from_markdown(&t, "正文"))
            .unwrap_or_else(|_| "正文".to_string());
        let body = format!("# Summary\n\n- [{title}]({file})\n");
        std::fs::write(&summary_path, body)?;
        return Ok(());
    }

    if summary_path.is_file() {
        return Ok(());
    }

    let mut chapters = Vec::new();
    for entry in WalkDir::new(src_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && workspace::is_markdown(entry.path()) {
            chapters.push(entry.into_path());
        }
    }
    if chapters.is_empty() {
        return Err(Error::NoChapters(src_dir.display().to_string()));
    }
    chapters.sort_by(|a, b| {
        let ra = a.strip_prefix(src_dir).unwrap_or(a).to_string_lossy().replace('\\', "/");
        let rb = b.strip_prefix(src_dir).unwrap_or(b).to_string_lossy().replace('\\', "/");
        let rank = |rel: &str| -> u8 {
            let lower = rel.to_ascii_lowercase();
            if lower == "readme.md" || lower == "index.md" {
                0
            } else if !rel.contains('/') {
                1
            } else {
                2
            }
        };
        rank(&ra).cmp(&rank(&rb)).then_with(|| workspace::natural_cmp(&ra, &rb))
    });

    let mut body = String::from("# Summary\n\n");
    for path in &chapters {
        let rel = path
            .strip_prefix(src_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名")
            .to_string();
        let title = workspace::read_chapter(path)
            .map(|t| workspace::title_from_markdown(&t, &stem))
            .unwrap_or(stem);
        // SUMMARY.md treats `[` `]` `(` `)` as syntax.
        let safe_title = title.replace('[', "\\[").replace(']', "\\]");
        let safe_rel = rel.replace('(', "%28").replace(')', "%29").replace(' ', "%20");
        body.push_str(&format!("- [{safe_title}]({safe_rel})\n"));
    }
    std::fs::write(&summary_path, body)?;
    warnings.push(format!("已自动生成 SUMMARY.md（{} 个章节）。", chapters.len()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn single_file_project_gets_a_summary() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = tmp.path().join("hello.md");
        fs::write(&doc, "# 你好世界\n\n正文内容").unwrap();

        let cache = tmp.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        let meta = BookMeta::default().filled("你好世界");
        let project = prepare(&doc, SourceKind::SingleFile, &meta, &cache).unwrap();

        let summary = fs::read_to_string(project.src_dir.join("SUMMARY.md")).unwrap();
        assert!(summary.contains("你好世界"), "{summary}");
        assert!(summary.contains("hello.md"), "{summary}");
    }

    #[test]
    fn folder_project_copies_markdown_and_images() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("book");
        fs::create_dir_all(src.join("images")).unwrap();
        fs::write(src.join("01-intro.md"), "# 简介\n").unwrap();
        fs::write(src.join("02-body.md"), "# 正文\n").unwrap();
        fs::write(src.join("images/cover.png"), b"\x89PNG\r\n\x1a\n").unwrap();

        let cache = tmp.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        let meta = BookMeta::default().filled("测试书");
        let project = prepare(&src, SourceKind::Folder, &meta, &cache).unwrap();

        assert!(project.src_dir.join("01-intro.md").is_file());
        assert!(project.src_dir.join("images/cover.png").is_file());
        let summary = fs::read_to_string(project.src_dir.join("SUMMARY.md")).unwrap();
        assert!(summary.contains("01-intro.md"));
        // natural ordering: intro before body
        let a = summary.find("01-intro.md").unwrap();
        let b = summary.find("02-body.md").unwrap();
        assert!(a < b);
    }
}
