//! Workspace discovery: turn "a path the user picked" into an ordered outline
//! plus a file tree for the sidebar.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mdbook_core::config::Config;
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::frontmatter;
use crate::model::{BookMeta, ChapterEntry, FsNode, FrontMatter, SourceKind, Workspace};

/// Directories we never walk into.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    "book",
    "dist",
    ".cargo-home",
    ".venv",
    "__pycache__",
    ".idea",
    ".vscode",
];

/// File extensions treated as Markdown.
pub fn is_markdown(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("md" | "markdown" | "mdown" | "mkd")
    )
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.') && name != "."
}

/// Natural ("chapter2" < "chapter10") comparison, CJK-friendly.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                if x.is_ascii_digit() && y.is_ascii_digit() {
                    let mut xs = String::new();
                    while let Some(c) = ai.peek().copied() {
                        if c.is_ascii_digit() {
                            xs.push(c);
                            ai.next();
                        } else {
                            break;
                        }
                    }
                    let mut ys = String::new();
                    while let Some(c) = bi.peek().copied() {
                        if c.is_ascii_digit() {
                            ys.push(c);
                            bi.next();
                        } else {
                            break;
                        }
                    }
                    let xn: u128 = xs.trim_start_matches('0').parse().unwrap_or(0);
                    let yn: u128 = ys.trim_start_matches('0').parse().unwrap_or(0);
                    match xn.cmp(&yn) {
                        Ordering::Equal => {}
                        other => return other,
                    }
                } else {
                    ai.next();
                    bi.next();
                    match x.to_lowercase().cmp(y.to_lowercase()) {
                        Ordering::Equal => {}
                        other => return other,
                    }
                }
            }
        }
    }
}

/// Count "words": CJK characters count individually, latin runs count once.
pub fn count_words(text: &str) -> usize {
    let mut count = 0usize;
    let mut in_word = false;
    for c in text.chars() {
        if c.is_whitespace() {
            in_word = false;
            continue;
        }
        let cjk = matches!(c as u32,
            0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF);
        if cjk {
            count += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                count += 1;
                in_word = true;
            }
        }
    }
    count
}

/// Split a document into (front matter, body) and derive the title + word count
/// from both halves.
///
/// Front matter never counts towards the word count and never leaks into the
/// rendered body.
pub fn analyze(text: &str, fallback: &str) -> (String, usize, FrontMatter) {
    let (parsed, body) = frontmatter::split(text);
    let fm = parsed.map(|p| p.front_matter).unwrap_or_default();
    let title = fm
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| title_from_body(body, fallback));
    (title, count_words(body), fm)
}

/// Title from the document body only: first `#` heading, else the first
/// meaningful line, else the file stem.
pub fn title_from_markdown(text: &str, fallback: &str) -> String {
    analyze(text, fallback).0
}

/// Body-only title extraction (no front matter involved).
fn title_from_body(text: &str, fallback: &str) -> String {
    let mut first_line: Option<String> = None;
    let mut in_fence = false;
    for raw in text.lines().take(80) {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            let t = tidy_title(rest);
            if !t.is_empty() {
                return t;
            }
        }
        if first_line.is_none() {
            let cleaned = line
                .trim_start_matches(['#', '>', '-', '*', ' ', '\t'])
                .trim();
            if !cleaned.is_empty() {
                first_line = Some(tidy_title(cleaned));
            }
        }
    }
    first_line
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn tidy_title(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            '*' | '`' | '_' | '~' => {}
            '\\' => {}
            _ if depth > 0 => {}
            _ => out.push(c),
        }
    }
    let out = out.trim().to_string();
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > 80 {
        format!("{}…", out.chars().take(79).collect::<String>())
    } else {
        out
    }
}

fn read_to_string_lossy(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    // Strip a UTF-8 BOM if present, then decode lossily so we never hard-fail on
    // a single stray byte in an otherwise fine document.
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Read a chapter's Markdown for the preview pane (capped at ~8 MiB).
pub fn read_chapter(path: &Path) -> Result<String> {
    if !path.exists() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    let meta = std::fs::metadata(path)?;
    if meta.len() > 8 * 1024 * 1024 {
        return Err(Error::Other(format!(
            "文件过大（{:.1} MB），预览已跳过",
            meta.len() as f64 / 1_048_576.0
        )));
    }
    read_to_string_lossy(path)
}

/// Save an edited chapter back to disk, atomically.
///
/// The temp file lands next to the original so the final rename stays on one
/// filesystem; the original's permissions are carried over so a swap never
/// changes how the file can be read or executed.
pub fn write_chapter(path: &Path, content: &str) -> Result<()> {
    if !path.exists() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    if !path.is_file() {
        return Err(Error::Other(format!("不是文件：{}", path.display())));
    }
    if !is_markdown(path) {
        return Err(Error::Other(format!(
            "只支持保存 Markdown 文件：{}",
            path.display()
        )));
    }
    let bytes = content.as_bytes();
    // Mirror the read cap, otherwise the next preview would refuse the file.
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(Error::Other(format!(
            "内容过大（{:.1} MB），已停止保存",
            bytes.len() as f64 / 1_048_576.0
        )));
    }
    let dir = path
        .parent()
        .ok_or_else(|| Error::Other(format!("无法定位文件目录：{}", path.display())))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("chapter");
    // The pid suffix keeps two app instances from clobbering each other's temp.
    let tmp = dir.join(format!(".{name}.{}.mdepub-tmp", std::process::id()));

    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(meta.permissions().mode()));
    }
    #[cfg(windows)]
    if path.exists() {
        // Windows' rename refuses to overwrite, so make room first.
        std::fs::remove_file(path)?;
    }
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error.into());
    }
    Ok(())
}

/// A raw chapter parsed out of `SUMMARY.md` before mdBook sees it.
#[derive(Debug, Clone)]
struct SummaryEntry {
    title: String,
    rel: String,
    level: u32,
}

/// Minimal `SUMMARY.md` parser: understands `- [Title](file.md)` (any bullet,
/// any indentation) and `[Title](file.md)` link-only lines.
fn parse_summary(text: &str) -> Vec<SummaryEntry> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("---") {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let body = trimmed
            .trim_start_matches(['-', '*', '+'])
            .trim_start();
        // [Title](target)
        let Some(open) = body.find('[') else { continue };
        if open != 0 {
            continue;
        }
        let Some(close) = body.find("](") else { continue };
        if close <= open {
            continue;
        }
        let title = body[open + 1..close].trim().to_string();
        let rest = &body[close + 2..];
        let Some(end) = rest.find(')') else { continue };
        let mut target = rest[..end].trim().to_string();
        if target.starts_with('<') && target.ends_with('>') {
            target = target[1..target.len() - 1].to_string();
        }
        if target.is_empty() || target.starts_with("http://") || target.starts_with("https://") {
            continue;
        }
        // Strip anchors for the outline.
        let target = target.split('#').next().unwrap_or("").trim().to_string();
        if target.is_empty() {
            continue;
        }
        let level = (indent / 2).min(5) as u32 + 1;
        out.push(SummaryEntry {
            title: if title.is_empty() {
                target.clone()
            } else {
                title
            },
            rel: target,
            level,
        });
    }
    out
}

/// Open a Markdown file, a folder of Markdown files, or an mdBook project.
pub fn open(path: &Path) -> Result<Workspace> {
    if !path.exists() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    if path.is_file() {
        if !is_markdown(path) {
            return Err(Error::UnsupportedSource(path.display().to_string()));
        }
        return open_single_file(path);
    }
    if path.join("book.toml").is_file() {
        open_mdbook(path)
    } else {
        open_folder(path)
    }
}

fn open_single_file(path: &Path) -> Result<Workspace> {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let parent = abs.parent().unwrap_or(Path::new(".")).to_path_buf();
    let text = read_to_string_lossy(&abs)?;
    let stem = abs
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("未命名")
        .to_string();
    let (title, words, fm) = analyze(&text, &stem);
    let rel = abs
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("index.md")
        .to_string();

    let chapter = ChapterEntry {
        id: rel.clone(),
        title: title.clone(),
        path: abs.display().to_string(),
        rel: rel.clone(),
        level: 1,
        words,
    };
    let tree = vec![FsNode {
        name: rel.clone(),
        path: abs.display().to_string(),
        is_dir: false,
        is_markdown: true,
        in_book: true,
        children: Vec::new(),
    }];

    Ok(Workspace {
        root: abs.display().to_string(),
        kind: SourceKind::SingleFile,
        title: title.clone(),
        src_dir: parent.display().to_string(),
        chapters: vec![chapter],
        tree,
        warnings: Vec::new(),
        meta: meta_from_frontmatter(&fm, &parent, title),
        total_words: words,
    })
}

/// Build book metadata from a document's front matter.
///
/// `base_dir` is used to turn a relative `image:` into an absolute cover path.
fn meta_from_frontmatter(fm: &FrontMatter, base_dir: &Path, title: String) -> BookMeta {
    let mut meta = BookMeta::default().filled(title);
    if let Some(description) = fm.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        meta.description = description.to_string();
    }
    if let Some(language) = fm.language.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        meta.language = language.to_string();
    }
    if !fm.authors.is_empty() {
        meta.authors = fm.authors.clone();
    }
    if !fm.tags.is_empty() {
        meta.subjects = fm.tags.clone();
    }
    if let Some(image) = fm.image.as_deref().map(str::trim).filter(|i| !i.is_empty()) {
        let candidate = Path::new(image);
        let resolved = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            base_dir.join(candidate)
        };
        if resolved.is_file() {
            meta.cover = Some(resolved.display().to_string());
        }
    }
    meta
}

fn open_mdbook(root: &Path) -> Result<Workspace> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut warnings = Vec::new();
    let config = Config::from_disk(root.join("book.toml")).map_err(Error::MdBook)?;
    let src_dir = root.join(&config.book.src);
    if !src_dir.is_dir() {
        return Err(Error::Other(format!(
            "book.toml 里写的源目录不存在：{}",
            src_dir.display()
        )));
    }

    let summary_path = src_dir.join("SUMMARY.md");
    let mut chapters: Vec<ChapterEntry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if summary_path.is_file() {
        let summary = read_to_string_lossy(&summary_path)?;
        for entry in parse_summary(&summary) {
            let full = src_dir.join(&entry.rel);
            if !full.is_file() {
                if is_markdown(&full) {
                    warnings.push(format!("SUMMARY.md 引用了不存在的文件：{}", entry.rel));
                }
                continue;
            }
            let key = entry.rel.clone();
            if !seen.insert(key.clone()) {
                continue;
            }
            let words = read_to_string_lossy(&full)
                .map(|t| analyze(&t, "未命名").1)
                .unwrap_or(0);
            chapters.push(ChapterEntry {
                id: key.clone(),
                title: entry.title,
                path: full.display().to_string(),
                rel: key,
                level: entry.level,
                words,
            });
        }
    }

    if chapters.is_empty() {
        warnings.push("没有从 SUMMARY.md 解析到章节，已按文件名顺序回退。".to_string());
        chapters = discover_markdown(&src_dir)?;
    }

    if chapters.is_empty() {
        return Err(Error::NoChapters(src_dir.display().to_string()));
    }

    let title = config
        .book
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| {
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("未命名书籍")
                .to_string()
        });

    let meta = BookMeta {
        title: title.clone(),
        authors: config
            .book
            .authors
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect(),
        language: config
            .book
            .language
            .clone()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| "zh-CN".to_string()),
        description: config
            .book
            .description
            .clone()
            .unwrap_or_default()
            .trim()
            .to_string(),
        ..BookMeta::default()
    }
    .filled(title.clone());

    let book_files: HashSet<PathBuf> = chapters.iter().map(|c| PathBuf::from(&c.rel)).collect();
    let tree = build_tree(&src_dir, &book_files)?;
    let total_words = chapters.iter().map(|c| c.words).sum();

    Ok(Workspace {
        root: root.display().to_string(),
        kind: SourceKind::MdBook,
        title,
        src_dir: src_dir.display().to_string(),
        chapters,
        tree,
        warnings,
        meta,
        total_words,
    })
}

fn open_folder(root: &Path) -> Result<Workspace> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut warnings = Vec::new();
    let mut chapters = discover_markdown(&root)?;
    if chapters.is_empty() {
        return Err(Error::NoChapters(root.display().to_string()));
    }
    // A root README/index makes a natural preface.
    chapters.sort_by(|a, b| {
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
        rank(&a.rel)
            .cmp(&rank(&b.rel))
            .then_with(|| natural_cmp(&a.rel, &b.rel))
    });

    let title = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("未命名书籍")
        .to_string();

    if chapters.len() > 400 {
        warnings.push(format!(
            "共发现 {} 个 Markdown 文件，章节较多，导出可能耗时。",
            chapters.len()
        ));
    }

    let book_files: HashSet<PathBuf> = chapters.iter().map(|c| PathBuf::from(&c.rel)).collect();
    let tree = build_tree(&root, &book_files)?;
    let total_words = chapters.iter().map(|c| c.words).sum();

    Ok(Workspace {
        root: root.display().to_string(),
        kind: SourceKind::Folder,
        title: title.clone(),
        src_dir: root.display().to_string(),
        chapters,
        tree,
        warnings,
        meta: BookMeta::default().filled(title),
        total_words,
    })
}

/// Collect every Markdown file under `dir`, in natural order, as level-1 chapters.
fn discover_markdown(dir: &Path) -> Result<Vec<ChapterEntry>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            if is_hidden(&name) {
                return false;
            }
            if e.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref()) {
                return false;
            }
            true
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && is_markdown(entry.path()) {
            files.push(entry.into_path());
        }
    }
    files.sort_by(|a, b| natural_cmp(&rel_str(dir, a), &rel_str(dir, b)));

    let mut chapters = Vec::new();
    for full in files {
        let rel = rel_str(dir, &full);
        let stem = full
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名")
            .to_string();
        let (title, words) = match read_to_string_lossy(&full) {
            Ok(text) => {
                let (title, words, _) = analyze(&text, &stem);
                (title, words)
            }
            Err(_) => (stem, 0),
        };
        chapters.push(ChapterEntry {
            id: rel.clone(),
            title,
            path: full.display().to_string(),
            rel,
            level: 1,
            words,
        });
    }
    Ok(chapters)
}

fn rel_str(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Build the sidebar tree out of the Markdown tree plus any assets next to it.
fn build_tree(root: &Path, book_files: &HashSet<PathBuf>) -> Result<Vec<FsNode>> {
    #[derive(Default)]
    struct Dir {
        dirs: Vec<(String, Dir)>,
        files: Vec<FsNode>,
    }

    /// Insert `node` under `dir`, creating intermediate directories.
    ///
    /// `dirs` must contain only the *directory* components — the file name is
    /// carried by `node` itself. Passing the full relative path here (including
    /// the file name) silently wraps every file in a directory named after
    /// itself.
    fn insert(dir: &mut Dir, dirs: &[String], node: FsNode) {
        if dirs.is_empty() {
            dir.files.push(node);
            return;
        }
        let parts = dirs;
        let head = parts[0].clone();
        if !dir.dirs.iter().any(|(n, _)| n == &head) {
            dir.dirs.push((head.clone(), Dir::default()));
        }
        let idx = dir.dirs.iter().position(|(n, _)| n == &head).unwrap();
        insert(&mut dir.dirs[idx].1, &parts[1..], node);
    }

    fn finish(dir: Dir, base: &Path, out: &mut Vec<FsNode>) {
        let mut dirs = dir.dirs;
        dirs.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        for (name, sub) in dirs {
            let path = base.join(&name);
            let mut children = Vec::new();
            finish(sub, &path, &mut children);
            out.push(FsNode {
                name,
                path: path.display().to_string(),
                is_dir: true,
                is_markdown: false,
                in_book: false,
                children,
            });
        }
        let mut files = dir.files;
        files.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        out.extend(files);
    }

    let mut root_dir = Dir::default();
    // No depth cap: blog layouts like `content/zh/posts/2026/09/<slug>/index.md`
    // are deeper than any arbitrary limit, and a truncated tree silently hides
    // files. `SKIP_DIRS` + the hidden-file filter keep the walk bounded.
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            if is_hidden(&name) {
                return false;
            }
            if e.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref()) {
                return false;
            }
            true
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = rel_str(root, entry.path());
        let node = FsNode {
            name: entry.file_name().to_string_lossy().to_string(),
            path: entry.path().display().to_string(),
            is_dir: false,
            is_markdown: is_markdown(entry.path()),
            in_book: book_files.contains(&PathBuf::from(&rel)),
            children: Vec::new(),
        };
        // Split "a/b/c.md" into directory parts ["a", "b"] plus the file node.
        let segments: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        let Some((dirs, _file)) = segments.split_at_checked(segments.len().saturating_sub(1)) else {
            continue;
        };
        let dirs: Vec<String> = dirs.iter().map(|s| s.to_string()).collect();
        insert(&mut root_dir, &dirs, node);
    }

    let mut out = Vec::new();
    finish(root_dir, root, &mut out);
    Ok(out)
}

/// Whether a directory tree holds any Markdown at all.
///
/// Deliberately uncapped: this is what decides whether an export is allowed to
/// proceed, and a depth limit here made deeply nested blogs (`…/2026/09/slug/`)
/// fail with "no Markdown found" even though every post was there.
pub fn contains_markdown(dir: &Path) -> bool {
    WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            if is_hidden(&name) {
                return false;
            }
            !(e.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref()))
        })
        .filter_map(|e| e.ok())
        .any(|e| e.file_type().is_file() && is_markdown(e.path()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_beats_lexicographic() {
        let mut v = vec!["ch10.md", "ch2.md", "ch1.md"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["ch1.md", "ch2.md", "ch10.md"]);
    }

    #[test]
    fn parses_nested_summary() {
        let md = "# Summary\n\n[前言](preface.md)\n\n- [第一章](ch1.md)\n  - [小节](ch1/a.md)\n";
        let got = parse_summary(md);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].rel, "preface.md");
        assert_eq!(got[1].level, 1);
        assert_eq!(got[2].level, 2);
    }

    #[test]
    fn counts_cjk_and_latin() {
        assert_eq!(count_words("你好 世界 hello world"), 4 + 2);
    }

    #[test]
    fn picks_h1_as_title() {
        assert_eq!(
            title_from_markdown("# 我的 **书**\n\n正文", "fallback"),
            "我的 书"
        );
    }

    #[test]
    fn deeply_nested_blog_layouts_are_discovered() {
        // Mirrors `content/zh/posts/2026/09/<slug>/index.md`.
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("content/zh/posts/2026/09/software-engineering-is-dead");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(
            deep.join("index.md"),
            "---\ntitle: 软件工程亡了\ntags: [a, b]\n---\n\n正文。\n",
        )
        .unwrap();

        assert!(contains_markdown(dir.path()), "deep markdown was not found");

        let ws = open(dir.path()).unwrap();
        assert_eq!(ws.chapters.len(), 1);
        assert_eq!(ws.chapters[0].title, "软件工程亡了");

        // …and the sidebar tree must not truncate it either.
        fn flatten(nodes: &[FsNode], out: &mut Vec<String>) {
            for node in nodes {
                out.push(node.name.clone());
                flatten(&node.children, out);
            }
        }
        let mut names = Vec::new();
        flatten(&ws.tree, &mut names);
        assert!(names.contains(&"index.md".to_string()), "tree = {names:?}");
    }

    #[test]
    fn the_tree_does_not_wrap_files_in_a_directory_of_their_own_name() {
        // Regression: every file used to end up as `<name>/<name>` because the
        // whole relative path (file name included) was treated as directories.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("guide/deep")).unwrap();
        std::fs::write(dir.path().join("top.md"), "# Top\n").unwrap();
        std::fs::write(dir.path().join("guide/one.md"), "# One\n").unwrap();
        std::fs::write(dir.path().join("guide/deep/two.md"), "# Two\n").unwrap();

        let ws = open(dir.path()).unwrap();

        fn shape(nodes: &[FsNode]) -> Vec<String> {
            let mut out = Vec::new();
            for node in nodes {
                let mark = if node.is_dir { "/" } else { "" };
                out.push(format!("{}{mark}", node.name));
                let mut inner = shape(&node.children);
                out.append(&mut inner);
            }
            out
        }
        let got = shape(&ws.tree);
        // Files sit at the level their path says, with no self-named wrapper.
        assert!(got.contains(&"top.md".to_string()), "{got:?}");
        assert!(!got.contains(&"top.md/".to_string()), "{got:?}");
        assert!(got.contains(&"guide/".to_string()), "{got:?}");
        assert!(!got.contains(&"guide.md/".to_string()), "{got:?}");
        assert!(got.contains(&"deep/".to_string()), "{got:?}");
        assert!(got.contains(&"two.md".to_string()), "{got:?}");
        assert!(!got.contains(&"one.md/".to_string()), "{got:?}");
        assert!(!got.contains(&"two.md/".to_string()), "{got:?}");
    }

    #[test]
    fn front_matter_title_wins_over_the_heading() {
        let doc = "---\ntitle: 来自头部的标题\n---\n\n# 正文里的标题\n\n内容\n";
        assert_eq!(title_from_markdown(doc, "fallback"), "来自头部的标题");
    }

    #[test]
    fn front_matter_is_not_counted_as_words_and_not_in_the_body() {
        let doc = "---\ntitle: T\ndescription: 一段很长的描述文字\n---\n\n你好 世界\n";
        let (title, words, fm) = analyze(doc, "fallback");
        assert_eq!(title, "T");
        assert!(fm.is_present());
        // "你好" (2 CJK chars) + "世界" (2) + "T" comes from front matter so it
        // must not be counted at all.
        assert_eq!(words, 4, "front matter leaked into the word count");
    }

    #[test]
    fn front_matter_cover_is_resolved_relative_to_the_document() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("images")).unwrap();
        std::fs::write(dir.path().join("images/cover.png"), b"x").unwrap();
        let doc = dir.path().join("post.md");
        std::fs::write(&doc, "---\ntitle: 有封面\nimage: images/cover.png\n---\n正文\n").unwrap();

        let ws = open(&doc).unwrap();
        assert_eq!(ws.title, "有封面");
        // `open` canonicalises the source, so compare canonical paths (on macOS
        // /var is a symlink to /private/var).
        let expected = dir
            .path()
            .join("images/cover.png")
            .canonicalize()
            .unwrap();
        assert_eq!(
            ws.meta.cover.as_deref().map(Path::new).map(|p| p.canonicalize().unwrap()),
            Some(expected)
        );
    }

    #[test]
    fn write_chapter_round_trips_and_guards() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "# 旧内容\n").unwrap();

        write_chapter(&file, "# 新内容\n\n正文。").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# 新内容\n\n正文。");
        // No temp leftovers in the book directory.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

        // Only Markdown files are writable through this path.
        let other = dir.path().join("cover.png");
        std::fs::write(&other, b"png").unwrap();
        assert!(write_chapter(&other, "x").is_err());
        // Missing targets are rejected instead of recreated.
        assert!(write_chapter(&dir.path().join("ghost.md"), "x").is_err());
    }
}
