//! EPUB generation.
//!
//! The book itself comes from mdBook (it parsed `book.toml`, walked
//! `SUMMARY.md` and ran the built-in preprocessors). Each chapter's Markdown is
//! rendered with `mdbook-markdown`, which is the exact pulldown-cmark
//! configuration mdBook uses for its own HTML output, then post-processed into
//! XHTML and packaged with `epub-builder`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use epub_builder::{EpubBuilder, EpubContent, EpubVersion, PageDirection, ReferenceType, ZipLibrary};
use lol_html::{element, rewrite_str, RewriteStrSettings};
use mdbook_core::book::{Book, BookItem};
use mdbook_markdown::pulldown_cmark::{html as cmark_html, CowStr, Event, Tag, TagEnd};
use mdbook_markdown::MarkdownOptions;
use mdbook_renderer::{RenderContext, Renderer};

use crate::error::{Error, Result};
use crate::model::{BookMeta, EpubOptions, Progress, ProgressFn};
use crate::render::BuiltBook;

/// mdBook `Renderer` used only so we can call `MDBook::preprocess_book`,
/// which runs `{{#include}}` and friends before we render the EPUB.
pub struct EpubRenderer;

impl Renderer for EpubRenderer {
    fn name(&self) -> &str {
        "epub"
    }

    fn render(&self, _ctx: &RenderContext) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A chapter flattened out of the mdBook tree.
#[derive(Debug, Clone)]
struct FlatChapter {
    title: String,
    content: String,
    /// Path relative to the source dir, after mdBook's index preprocessor.
    rel: Option<PathBuf>,
    /// The real file path relative to the source dir.
    source_rel: Option<PathBuf>,
    /// 1-based nesting depth.
    depth: i32,
    /// Output file name inside the EPUB (`chapter-001.xhtml`).
    href: String,
}

fn flatten(items: &[BookItem], depth: i32, out: &mut Vec<FlatChapter>) {
    for item in items {
        match item {
            BookItem::Chapter(ch) => {
                let has_path = ch
                    .path
                    .as_ref()
                    .map(|p| !p.as_os_str().is_empty())
                    .unwrap_or(false);
                if has_path || !ch.content.trim().is_empty() {
                    out.push(FlatChapter {
                        title: ch.name.clone(),
                        content: ch.content.clone(),
                        rel: ch.path.clone(),
                        source_rel: ch.source_path.clone().or_else(|| ch.path.clone()),
                        depth: depth.max(1),
                        href: String::new(),
                    });
                }
                flatten(&ch.sub_items, depth + 1, out);
            }
            BookItem::PartTitle(_) | BookItem::Separator => {}
        }
    }
}

fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Join `target` onto `base` (already split into segments), resolving `.`/`..`.
fn norm_join(base: &[String], target: &str) -> Option<String> {
    let mut parts: Vec<String> = base.to_vec();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other.to_string()),
        }
    }
    Some(parts.join("/"))
}

fn is_external(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
        || lower.starts_with("data:")
        || lower.starts_with("//")
        || lower.starts_with("ftp://")
}

/// Guess an image MIME type from magic bytes, falling back to the extension.
fn sniff_mime(bytes: &[u8], path: &Path) -> String {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "image/jpeg".into();
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "image/png".into();
    }
    if bytes.starts_with(b"GIF8") {
        return "image/gif".into();
    }
    if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp".into();
    }
    if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
        return "image/svg+xml".into();
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        _ => "image/jpeg",
    }
    .to_string()
}

/// Slugify a heading the way mdBook does, but CJK-aware.
fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for c in text.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_dash = false;
        } else if c == '-' || c == '_' || c.is_whitespace() {
            if !last_dash {
                out.push('-');
                last_dash = true;
            }
        }
    }
    out.trim_matches('-').to_string()
}

/// Give every heading a stable `id` so intra-book `#anchors` keep working.
fn inject_heading_ids<'a>(events: Vec<Event<'a>>) -> Vec<Event<'a>> {
    let mut out: Vec<Event<'a>> = Vec::with_capacity(events.len());
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut idx = 0usize;

    while idx < events.len() {
        if let Event::Start(Tag::Heading {
            level,
            id,
            classes,
            attrs,
        }) = &events[idx]
        {
            let explicit = id.as_ref().map(|s| s.to_string());
            let mut end = idx + 1;
            let mut text = String::new();
            while end < events.len() {
                match &events[end] {
                    Event::End(TagEnd::Heading(_)) => break,
                    Event::Text(t) | Event::Code(t) => text.push_str(t),
                    _ => {}
                }
                end += 1;
            }
            let mut slug = explicit.unwrap_or_else(|| slugify(&text));
            if slug.is_empty() {
                slug = "section".to_string();
            }
            let counter = seen.entry(slug.clone()).or_insert(0);
            if *counter > 0 {
                slug = format!("{slug}-{counter}");
            }
            *counter += 1;

            out.push(Event::Start(Tag::Heading {
                level: *level,
                id: Some(CowStr::from(slug)),
                classes: classes.clone(),
                attrs: attrs.clone(),
            }));
            out.extend(events[idx + 1..end].iter().cloned());
            if end < events.len() {
                out.push(events[end].clone());
                idx = end + 1;
            } else {
                idx = end;
            }
            continue;
        }
        out.push(events[idx].clone());
        idx += 1;
    }
    out
}

/// Markdown -> HTML using mdBook's own pulldown-cmark configuration.
///
/// Front matter is stripped here too: `FrontMatterPreprocessor` normally does it
/// upstream, but this keeps the function correct when called on its own.
pub fn markdown_to_html(markdown: &str) -> String {
    let markdown = crate::frontmatter::strip(markdown);
    let options = MarkdownOptions::default();
    let parser = mdbook_markdown::new_cmark_parser(markdown, &options);
    let events: Vec<Event<'_>> = inject_heading_ids(parser.collect());
    let mut out = String::with_capacity(markdown.len() * 2);
    cmark_html::push_html(&mut out, events.into_iter());
    out
}

/// An asset discovered while rewriting a chapter.
struct Asset {
    href: String,
    bytes: Vec<u8>,
    mime: String,
}

/// Rewrite links/images in a rendered chapter so they work inside the EPUB.
fn rewrite_chapter_html(
    html: &str,
    src_dir: &Path,
    chapter_dir: &[String],
    link_map: &HashMap<String, String>,
    next_asset: &mut usize,
    assets: &mut Vec<Asset>,
    warnings: &mut Vec<String>,
) -> Result<String> {
    let assets_cell = RefCell::new(Vec::<Asset>::new());
    let warnings_cell = RefCell::new(Vec::<String>::new());
    let counter = RefCell::new(*next_asset);

    let settings = RewriteStrSettings {
        element_content_handlers: vec![
            element!("script, iframe, object, embed, link, base, meta[http-equiv]", |el| {
                el.remove();
                Ok(())
            }),
            element!("a[href]", |el| {
                let Some(href) = el.get_attribute("href") else {
                    return Ok(());
                };
                let (target, fragment) = match href.split_once('#') {
                    Some((t, f)) => (t.to_string(), Some(f.to_string())),
                    None => (href.clone(), None),
                };
                if target.is_empty() || is_external(&target) {
                    return Ok(());
                }
                let decoded = percent_decode(&target);
                let Some(resolved) = norm_join(chapter_dir, &decoded) else {
                    return Ok(());
                };
                let key = resolved.clone();
                let candidates = [
                    key.clone(),
                    key.trim_end_matches('/').to_string(),
                    format!("{key}.md"),
                    key.trim_end_matches(".md").to_string(),
                    key.trim_end_matches(".html").to_string(),
                    format!("{}.md", key.trim_end_matches(".html")),
                ];
                for cand in candidates.iter() {
                    if let Some(dest) = link_map.get(cand) {
                        let new_href = match &fragment {
                            Some(f) => format!("{dest}#{f}"),
                            None => dest.clone(),
                        };
                        let _ = el.set_attribute("href", &new_href);
                        return Ok(());
                    }
                }
                Ok(())
            }),
            // pulldown-cmark writes `checked=""`; XHTML readers prefer the
            // explicit `checked="checked"` form.
            element!("input", |el| {
                for name in [
                    "checked", "disabled", "readonly", "required", "multiple", "selected",
                ] {
                    if el.has_attribute(name) && el.get_attribute(name).as_deref() == Some("") {
                        let _ = el.set_attribute(name, name);
                    }
                }
                Ok(())
            }),
            element!("img[src]", |el| {
                let Some(src) = el.get_attribute("src") else {
                    return Ok(());
                };
                if src.is_empty() || is_external(&src) || src.starts_with('/') {
                    return Ok(());
                }
                let decoded = percent_decode(&src);
                let Some(resolved) = norm_join(chapter_dir, &decoded) else {
                    return Ok(());
                };
                let abs = src_dir.join(&resolved);
                let Ok(bytes) = std::fs::read(&abs) else {
                    warnings_cell
                        .borrow_mut()
                        .push(format!("找不到图片，已在 EPUB 中保留原路径：{resolved}"));
                    return Ok(());
                };
                let file_name = abs
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("image");
                let mut n = counter.borrow_mut();
                *n += 1;
                let safe_name = file_name.replace(
                    |c: char| !(c.is_alphanumeric() || c == '.' || c == '-' || c == '_'),
                    "_",
                );
                let href = format!("images/{:03}-{}", *n, safe_name);
                let mime = sniff_mime(&bytes, &abs);
                assets_cell.borrow_mut().push(Asset {
                    href: href.clone(),
                    bytes,
                    mime,
                });
                let _ = el.set_attribute("src", &href);
                Ok(())
            }),
        ],
        ..RewriteStrSettings::default()
    };

    let rewritten = rewrite_str(html, settings).map_err(|e| Error::Epub(e.to_string()))?;
    let collected = assets_cell.into_inner();
    if !collected.is_empty() {
        *next_asset += collected.len();
        assets.extend(collected);
    }
    warnings.extend(warnings_cell.into_inner());
    Ok(rewritten)
}

fn xhtml_document(title: &str, lang: &str, body_class: &str, body: &str) -> String {
    let escaped_title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="{lang}" lang="{lang}">
<head>
  <meta charset="utf-8"/>
  <title>{escaped_title}</title>
  <link rel="stylesheet" type="text/css" href="stylesheet.css"/>
</head>
<body class="{body_class}">
{body}
</body>
</html>
"#
    )
}

/// A node of the generated table of contents.
struct TocNode {
    href: String,
    title: String,
    children: Vec<TocNode>,
}

/// Build the TOC tree from the flat chapter list.
///
/// `FlatChapter::depth` comes from the SUMMARY nesting, but a hand-written
/// `SUMMARY.md` can jump levels (1 -> 3). Clamping the step to one keeps the
/// rendered `<ol>` well-formed no matter what the source does.
fn build_toc_tree(chapters: &[FlatChapter], max_depth: i32) -> Vec<TocNode> {
    let mut clamped: Vec<(i32, &FlatChapter)> = Vec::with_capacity(chapters.len());
    let mut previous = 1;
    for chapter in chapters {
        let mut depth = chapter.depth.max(1);
        if depth > previous + 1 {
            depth = previous + 1;
        }
        if depth > max_depth.max(1) {
            // Deeper than the configured limit: fold it into its parent level.
            depth = max_depth.max(1);
        }
        clamped.push((depth, chapter));
        previous = depth;
    }

    fn level(items: &[(i32, &FlatChapter)], cursor: &mut usize, depth: i32) -> Vec<TocNode> {
        let mut nodes = Vec::new();
        while *cursor < items.len() {
            let (item_depth, chapter) = items[*cursor];
            if item_depth != depth {
                break;
            }
            *cursor += 1;
            nodes.push(TocNode {
                href: chapter.href.clone(),
                title: chapter.title.clone(),
                children: level(items, cursor, depth + 1),
            });
        }
        nodes
    }

    let mut cursor = 0;
    level(&clamped, &mut cursor, 1)
}

fn render_toc_nodes(nodes: &[TocNode], out: &mut String) {
    if nodes.is_empty() {
        return;
    }
    out.push_str("<ol>\n");
    for node in nodes {
        out.push_str(&format!(
            "<li><a href=\"{}\">{}</a>",
            escape(&node.href),
            escape(&node.title)
        ));
        render_toc_nodes(&node.children, out);
        out.push_str("</li>\n");
    }
    out.push_str("</ol>\n");
}

/// The XHTML body of the generated contents page.
fn toc_page_body(chapters: &[FlatChapter], max_depth: i32, heading: &str) -> String {
    let tree = build_toc_tree(chapters, max_depth);
    if tree.is_empty() {
        return String::new();
    }
    let mut list = String::new();
    render_toc_nodes(&tree, &mut list);
    format!(
        "<div class=\"toc-page\">\n<h1 class=\"toc-title\">{}</h1>\n<nav epub:type=\"toc\" id=\"toc-page\">\n{list}</nav>\n</div>\n",
        escape(heading)
    )
}

/// The stylesheet shipped inside every exported EPUB.
pub fn stylesheet(opts: &EpubOptions) -> String {
    let family = match opts.font_family.as_str() {
        "sans" => "system-ui, -apple-system, \"Segoe UI\", \"PingFang SC\", \"Hiragino Sans GB\", \"Microsoft YaHei\", \"Noto Sans CJK SC\", sans-serif",
        "system" => "-apple-system, \"PingFang SC\", \"Hiragino Sans GB\", \"Microsoft YaHei\", \"Noto Sans CJK SC\", serif",
        _ => "Georgia, \"Songti SC\", \"SimSun\", \"Noto Serif CJK SC\", \"Source Han Serif SC\", serif",
    };
    let font_size = if opts.font_size > 0.0 { opts.font_size } else { 1.0 };
    let line_height = if opts.line_height > 0.0 { opts.line_height } else { 1.6 };

    format!(
        r#"@charset "utf-8";

html {{ font-size: 100%; }}
body {{
  font-family: {family};
  font-size: {font_size}em;
  line-height: {line_height};
  margin: 0 5%;
  padding: 0;
  text-align: justify;
  word-wrap: break-word;
  -epub-hyphens: auto;
}}

h1, h2, h3, h4, h5, h6 {{
  font-family: inherit;
  line-height: 1.3;
  margin: 1.4em 0 0.6em;
  page-break-after: avoid;
  break-after: avoid;
  text-align: left;
}}
h1 {{ font-size: 1.6em; margin-top: 0; }}
h2 {{ font-size: 1.35em; }}
h3 {{ font-size: 1.15em; }}
h4, h5, h6 {{ font-size: 1em; }}
h1 + p, h2 + p, h3 + p {{ margin-top: 0.2em; }}

p {{ margin: 0 0 0.8em; }}
a {{ color: inherit; text-decoration: underline; }}
strong {{ font-weight: bold; }}
em {{ font-style: italic; }}
hr {{ border: 0; border-top: 1px solid #cccccc; margin: 1.5em 0; }}

img {{ max-width: 100%; height: auto; }}
figure {{ margin: 1em 0; text-align: center; }}
figcaption {{ font-size: 0.9em; color: #666666; }}

ul, ol {{ margin: 0 0 0.9em; padding-left: 1.6em; }}
li {{ margin: 0.2em 0; }}
li > p {{ margin-bottom: 0.3em; }}

blockquote {{
  margin: 1em 0;
  padding: 0.2em 1em;
  border-left: 3px solid #bbbbbb;
  color: #444444;
  background: #f7f7f7;
}}

code {{
  font-family: "SF Mono", Menlo, Consolas, "Liberation Mono", "Courier New", monospace;
  font-size: 0.9em;
  background: #f2f2f2;
  padding: 0 0.2em;
  border-radius: 2px;
}}
pre {{
  font-family: "SF Mono", Menlo, Consolas, "Liberation Mono", "Courier New", monospace;
  font-size: 0.85em;
  line-height: 1.45;
  background: #f5f5f5;
  border: 1px solid #e0e0e0;
  border-radius: 4px;
  padding: 0.7em 0.9em;
  overflow-x: auto;
  white-space: pre-wrap;
  word-wrap: break-word;
  page-break-inside: avoid;
}}
pre code {{ background: transparent; padding: 0; font-size: 1em; }}

table {{
  border-collapse: collapse;
  width: 100%;
  margin: 1em 0;
  font-size: 0.92em;
}}
th, td {{ border: 1px solid #cccccc; padding: 0.4em 0.6em; text-align: left; }}
th {{ background: #f2f2f2; font-weight: bold; }}

.markdown-alert, .admonition {{
  margin: 1em 0;
  padding: 0.7em 1em;
  border-left: 4px solid #6b7cff;
  background: #f4f6ff;
  border-radius: 3px;
}}
.markdown-alert-title {{ font-weight: bold; margin-bottom: 0.3em; }}
blockquote.markdown-alert > p:last-child {{ margin-bottom: 0; }}
blockquote.markdown-alert::before {{
  display: block;
  font-weight: bold;
  margin-bottom: 0.25em;
}}
blockquote.markdown-alert-note::before {{ content: "注意"; color: #3b5bdb; }}
blockquote.markdown-alert-tip::before {{ content: "提示"; color: #1f8a53; }}
blockquote.markdown-alert-important::before {{ content: "重要"; color: #6d3fd4; }}
blockquote.markdown-alert-warning::before {{ content: "警告"; color: #a86a12; }}
blockquote.markdown-alert-caution::before {{ content: "小心"; color: #b3392f; }}
blockquote.markdown-alert-tip {{ border-left-color: #35a06a; background: #f1fbf5; }}
blockquote.markdown-alert-important {{ border-left-color: #8a5cf6; background: #f6f3ff; }}
blockquote.markdown-alert-warning {{ border-left-color: #d79a2b; background: #fdf8ef; }}
blockquote.markdown-alert-caution {{ border-left-color: #e0574c; background: #fdf2f1; }}

.footnote-reference {{ font-size: 0.75em; vertical-align: super; }}
.footnote-definition {{ font-size: 0.9em; border-top: 1px solid #dddddd; padding-top: 0.5em; }}

.toc-page {{ page-break-before: always; break-before: page; }}
.toc-title {{ text-align: center; margin-bottom: 1.4em; }}
nav#toc-page ol {{ list-style: none; padding-left: 0; margin: 0; }}
nav#toc-page ol ol {{ padding-left: 1.4em; }}
nav#toc-page li {{ margin: 0.5em 0; }}
nav#toc-page ol ol {{ margin: 0.2em 0 0.2em 0; font-size: 0.95em; }}
nav#toc-page > ol > li > a {{ font-weight: bold; }}
nav#toc-page ol ol a {{ color: #555555; }}
nav#toc-page a {{ text-decoration: none; }}

.cover {{ margin: 0; padding: 0; text-align: center; }}
.cover img {{ max-width: 100%; max-height: 100%; }}
.titlepage {{ text-align: center; padding-top: 20%; }}
.titlepage h1 {{ font-size: 2em; text-align: center; }}
.titlepage .author {{ font-size: 1.1em; margin-top: 1.5em; }}
.titlepage .publisher {{ font-size: 0.9em; margin-top: 3em; color: #666666; }}

.chapter {{ page-break-before: always; break-before: page; }}
{extra}
"#,
        extra = opts.extra_css
    )
}

/// Export the book as EPUB.
pub fn export(
    built: &BuiltBook,
    meta: &BookMeta,
    opts: &EpubOptions,
    toc_page: bool,
    toc_depth: u32,
    out_path: &Path,
    progress: ProgressFn<'_>,
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let src_dir = built.project.src_dir.clone();

    // 1. Flatten the mdBook tree.
    let mut chapters: Vec<FlatChapter> = Vec::new();
    flatten(&built.book.items, 1, &mut chapters);
    if chapters.is_empty() {
        return Err(Error::Epub(
            "这本书没有任何章节可导出。如果刚刚排除了章节，请至少保留一个。".to_string(),
        ));
    }
    for (i, ch) in chapters.iter_mut().enumerate() {
        ch.href = format!("chapter-{:03}.xhtml", i + 1);
    }

    // 2. Map source paths -> output files, so `.md` links keep working.
    let mut link_map: HashMap<String, String> = HashMap::new();
    for ch in &chapters {
        for key in [ch.rel.as_ref(), ch.source_rel.as_ref()].into_iter().flatten() {
            let s = to_slash(key);
            link_map.insert(s.clone(), ch.href.clone());
            if let Some(stripped) = s.strip_suffix(".md") {
                link_map.insert(format!("{stripped}.html"), ch.href.clone());
            }
            if let Some(stem) = s.strip_suffix(".md") {
                link_map.insert(stem.to_string(), ch.href.clone());
            }
        }
    }

    progress(Progress::new("epub", "正在排版 EPUB 章节…", 5));
    let mut assets: Vec<Asset> = Vec::new();
    let mut next_asset = 0usize;
    let total = chapters.len();
    let mut bodies: Vec<String> = Vec::with_capacity(total);

    for (i, ch) in chapters.iter().enumerate() {
        let chapter_dir: Vec<String> = ch
            .rel
            .as_ref()
            .or(ch.source_rel.as_ref())
            .map(|p| {
                let s = to_slash(p);
                let mut parts: Vec<String> = s.split('/').map(|x| x.to_string()).collect();
                parts.pop();
                parts
            })
            .unwrap_or_default();

        let html = markdown_to_html(&ch.content);
        let html = rewrite_chapter_html(
            &html,
            &src_dir,
            &chapter_dir,
            &link_map,
            &mut next_asset,
            &mut assets,
            &mut warnings,
        )?;
        bodies.push(html);

        let percent = 5 + ((i + 1) * 65 / total.max(1)) as u8;
        progress(
            Progress::new(
                "epub",
                format!("正在排版第 {} / {} 章…", i + 1, total),
                percent.min(70),
            )
            .with_steps(i + 1, total),
        );
    }

    // 3. Package.
    progress(Progress::new("epub", "正在写入 EPUB 容器…", 80));

    let zip = ZipLibrary::new()?;
    let mut builder = EpubBuilder::new(zip)?;
    builder.epub_version(match opts.version.as_str() {
        "2" => EpubVersion::V20,
        _ => EpubVersion::V33,
    });

    if opts.page_direction == "rtl" {
        builder.epub_direction(PageDirection::Rtl);
    } else if opts.page_direction == "ltr" {
        builder.epub_direction(PageDirection::Ltr);
    }

    let lang = if meta.language.trim().is_empty() {
        "zh-CN"
    } else {
        meta.language.trim()
    };
    let title = if meta.title.trim().is_empty() {
        "未命名书籍"
    } else {
        meta.title.trim()
    };

    builder.set_title(title);
    builder.set_languages(vec![lang.to_string()]);
    builder.set_generator(format!("mdepuber {} (mdBook)", env!("CARGO_PKG_VERSION")));
    for author in meta.authors.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
        builder.add_author(author);
    }
    if !meta.description.trim().is_empty() {
        builder.add_description(meta.description.trim());
    }
    if !meta.publisher.trim().is_empty() {
        builder
            .metadata("publisher", meta.publisher.trim())
            .map_err(|e| Error::Epub(e.to_string()))?;
    }
    if !meta.rights.trim().is_empty() {
        builder
            .metadata("rights", meta.rights.trim())
            .map_err(|e| Error::Epub(e.to_string()))?;
    }
    if !meta.series.trim().is_empty() {
        builder
            .metadata("belongs-to-collection", meta.series.trim())
            .map_err(|e| Error::Epub(e.to_string()))?;
    }
    for subject in meta.subjects.iter().filter(|s| !s.trim().is_empty()) {
        builder
            .metadata("subject", subject.trim())
            .map_err(|e| Error::Epub(e.to_string()))?;
    }
    match meta
        .identifier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(uuid::Uuid::parse_str)
    {
        Some(Ok(id)) => builder.set_uuid(id),
        Some(Err(_)) => warnings.push(
            "自定义标识符不是合法 UUID，EPUB 将使用自动生成的 UUID（ISBN 请写入版权页）。".to_string(),
        ),
        None => {}
    }
    let now = Utc::now();
    builder.set_publication_date(now);
    builder.set_modified_date(now);

    builder
        .stylesheet(stylesheet(opts).as_bytes())
        .map_err(|e| Error::Epub(e.to_string()))?;

    // Cover image.
    let mut cover_href: Option<String> = None;
    if let Some(cover) = meta.cover.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(cover);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let mime = sniff_mime(&bytes, &path);
                let ext = match mime.as_str() {
                    "image/png" => "png",
                    "image/gif" => "gif",
                    "image/webp" => "webp",
                    "image/svg+xml" => "svg",
                    _ => "jpg",
                };
                let href = format!("cover.{ext}");
                builder
                    .add_cover_image(href.clone(), bytes.as_slice(), mime.as_str())
                    .map_err(|e| Error::Epub(e.to_string()))?;
                cover_href = Some(href);
            }
            Err(e) => warnings.push(format!("封面图片无法读取（{cover}）：{e}")),
        }
    }

    if opts.cover_page {
        if let Some(href) = &cover_href {
            let body = format!(
                "<div class=\"cover\"><img src=\"{href}\" alt=\"{}\"/></div>",
                title.replace('"', "&quot;")
            );
            builder
                .add_content(
                    EpubContent::new("cover.xhtml", xhtml_document(title, lang, "cover", &body).into_bytes().as_slice())
                        .title("封面")
                        .reftype(ReferenceType::Cover),
                )
                .map_err(|e| Error::Epub(e.to_string()))?;
        }
    }

    // The contents page goes immediately after the cover, ahead of everything
    // else, so it is the first page a reader lands on after the artwork.
    if toc_page {
        let body = toc_page_body(&chapters, toc_depth as i32, "目录");
        if body.is_empty() {
            warnings.push("目录页未生成：书籍里没有可列出的章节。".to_string());
        } else {
            builder
                .add_content(
                    EpubContent::new(
                        "toc-page.xhtml",
                        xhtml_document("目录", lang, "toc-page", &body).into_bytes().as_slice(),
                    )
                    .title("目录")
                    .reftype(ReferenceType::Toc),
                )
                .map_err(|e| Error::Epub(e.to_string()))?;
        }
    }

    if opts.title_page {
        let mut body = format!("<div class=\"titlepage\"><h1>{}</h1>", escape(title));
        if meta.authors.iter().any(|a| !a.trim().is_empty()) {
            body.push_str(&format!(
                "<p class=\"author\">{}</p>",
                escape(&meta.author_line())
            ));
        }
        if !meta.publisher.trim().is_empty() {
            body.push_str(&format!(
                "<p class=\"publisher\">{}</p>",
                escape(meta.publisher.trim())
            ));
        }
        body.push_str("</div>");
        builder
            .add_content(
                EpubContent::new(
                    "titlepage.xhtml",
                    xhtml_document(title, lang, "titlepage", &body).into_bytes().as_slice(),
                )
                .title("书名页")
                .reftype(ReferenceType::TitlePage),
            )
            .map_err(|e| Error::Epub(e.to_string()))?;
    }

    for (i, ch) in chapters.iter().enumerate() {
        let body = format!("<div class=\"chapter\">{}</div>", bodies[i]);
        let doc = xhtml_document(&ch.title, lang, "chapter", &body);
        let content = EpubContent::new(ch.href.clone(), doc.as_bytes())
            .title(ch.title.clone())
            .level(ch.depth);
        builder
            .add_content(content)
            .map_err(|e| Error::Epub(e.to_string()))?;
    }

    for asset in assets {
        builder
            .add_resource(asset.href.as_str(), asset.bytes.as_slice(), asset.mime.as_str())
            .map_err(|e| Error::Epub(e.to_string()))?;
    }

    progress(Progress::new("epub", "正在压缩 EPUB…", 94));
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(out_path)?;
    let writer = std::io::BufWriter::new(file);
    builder.generate(writer).map_err(|e| Error::Epub(e.to_string()))?;

    Ok(warnings)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build the book's XHTML bodies without packaging (used by tests).
pub fn render_chapter_bodies(built: &BuiltBook) -> Result<Vec<(String, String)>> {
    let mut chapters: Vec<FlatChapter> = Vec::new();
    flatten(&built.book.items, 1, &mut chapters);
    let mut out = Vec::new();
    for ch in chapters {
        out.push((ch.title, markdown_to_html(&ch.content)));
    }
    Ok(out)
}

/// Timestamp helper kept for API symmetry with the PDF writer.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Convenience: the `Book` mdBook produced, for callers that need the tree.
pub fn book_chapters(book: &Book) -> Vec<(String, i32)> {
    let mut flat = Vec::new();
    flatten(&book.items, 1, &mut flat);
    flat.into_iter().map(|c| (c.title, c.depth)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(href: &str, title: &str, depth: i32) -> FlatChapter {
        FlatChapter {
            title: title.to_string(),
            content: String::new(),
            rel: None,
            source_rel: None,
            depth,
            href: href.to_string(),
        }
    }

    #[test]
    fn toc_page_lists_chapters_with_their_links() {
        let chapters = vec![
            chapter("chapter-001.xhtml", "前言", 1),
            chapter("chapter-002.xhtml", "第一章", 1),
            chapter("chapter-003.xhtml", "小节", 2),
        ];
        let html = toc_page_body(&chapters, 3, "目录");
        assert!(html.contains("<h1 class=\"toc-title\">目录</h1>"), "{html}");
        assert!(html.contains("href=\"chapter-001.xhtml\">前言<"), "{html}");
        assert!(html.contains("href=\"chapter-003.xhtml\">小节<"), "{html}");
        // The third chapter is nested one level under the second.
        assert_eq!(html.matches("<ol>").count(), 2, "{html}");
        assert_eq!(html.matches("</ol>").count(), 2, "{html}");
    }

    #[test]
    fn toc_page_survives_skipped_summary_levels() {
        // A hand-written SUMMARY can jump 1 -> 3; the rendered list must stay
        // balanced instead of emitting stray closing tags.
        let chapters = vec![
            chapter("a.xhtml", "A", 1),
            chapter("b.xhtml", "B", 3),
            chapter("c.xhtml", "C", 1),
        ];
        let html = toc_page_body(&chapters, 3, "目录");
        assert_eq!(html.matches("<ol>").count(), html.matches("</ol>").count());
        assert_eq!(html.matches("<li>").count(), html.matches("</li>").count());
        assert!(html.contains("B"), "{html}");
    }

    #[test]
    fn toc_depth_can_flatten_the_list() {
        let chapters = vec![
            chapter("a.xhtml", "A", 1),
            chapter("b.xhtml", "B", 2),
        ];
        let flat = toc_page_body(&chapters, 1, "目录");
        assert_eq!(flat.matches("<ol>").count(), 1, "depth 1 should be flat");
        assert!(flat.contains("B"), "deeper entries are folded in, not dropped");
        let nested = toc_page_body(&chapters, 3, "目录");
        assert_eq!(nested.matches("<ol>").count(), 2);
    }

    #[test]
    fn toc_page_escapes_titles() {
        let chapters = vec![chapter("a.xhtml", "A & B <tag>", 1)];
        let html = toc_page_body(&chapters, 3, "目录");
        assert!(html.contains("A &amp; B &lt;tag&gt;"), "{html}");
        assert!(!html.contains("<tag>"), "{html}");
    }

    #[test]
    fn front_matter_never_reaches_the_epub() {
        let html = markdown_to_html("---\ntitle: 头部标题\ntags: [a, b]\n---\n\n正文段落。\n");
        assert!(!html.contains("tags"), "front matter leaked: {html}");
        assert!(!html.contains("头部标题"), "front matter leaked: {html}");
        assert!(html.contains("正文段落。"), "{html}");
    }

    #[test]
    fn heading_ids_are_generated_and_unique() {
        let html = markdown_to_html("# 标题一\n\n## 标题一\n\n### Section A\n");
        assert!(html.contains("id=\"标题一\""), "{html}");
        assert!(html.contains("id=\"标题一-1\""), "{html}");
        assert!(html.contains("id=\"section-a\""), "{html}");
    }

    #[test]
    fn explicit_heading_id_is_respected() {
        let html = markdown_to_html("# Title {#custom}\n");
        assert!(html.contains("id=\"custom\""), "{html}");
    }

    #[test]
    fn natural_join_handles_parent_dirs() {
        let base = vec!["guide".to_string(), "deep".to_string()];
        assert_eq!(norm_join(&base, "../intro.md").unwrap(), "guide/intro.md");
        assert_eq!(norm_join(&base, "a.md").unwrap(), "guide/deep/a.md");
    }
}
