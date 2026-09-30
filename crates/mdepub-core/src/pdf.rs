//! PDF export: mdBook renders the single-page `print.html`, and a headless
//! Chromium prints it. The `@page` rules come from a generated stylesheet that
//! is registered as `output.html.additional-css` *before* mdBook runs, so the
//! user's project is never modified.

use std::path::{Path, PathBuf};

use crate::chrome::{self, PrintJob};
use crate::error::{Error, Result};
use crate::model::{PdfOptions, Progress, ProgressFn};
use crate::render::BuiltBook;

fn page_dimensions(opts: &PdfOptions) -> String {
    let custom = opts.custom_width_mm.max(50.0);
    let custom_h = opts.custom_height_mm.max(50.0);
    match opts.page_size.as_str() {
        "A3" => "297mm 420mm".to_string(),
        "A4" => "210mm 297mm".to_string(),
        "A5" => "148mm 210mm".to_string(),
        "B5" => "176mm 250mm".to_string(),
        "Letter" => "8.5in 11in".to_string(),
        "Legal" => "8.5in 14in".to_string(),
        "custom" => format!("{custom}mm {custom_h}mm"),
        _ => "210mm 297mm".to_string(),
    }
}

/// The stylesheet injected into `print.html` for the PDF run.
pub fn print_stylesheet(opts: &PdfOptions) -> String {
    let size = page_dimensions(opts);
    let margin = opts.margin_mm.clamp(0.0, 60.0);
    let scale = if opts.scale > 0.2 { opts.scale } else { 1.0 };

    let dark = opts.theme == "dark";
    let sepia = opts.theme == "sepia";

    let mut css = String::new();
    css.push_str("/* mdEPUBer PDF print stylesheet (generated) */\n");

    if dark {
        css.push_str(
            r#":root, html, html.light, html.rust, html.coal, html.navy, html.ayu {
  --bg: #16181d;
  --fg: #e6e8ea;
  --sidebar-bg: #1c1f24;
  --sidebar-fg: #cfd3d6;
  --sidebar-active: #6b9bff;
  --sidebar-spacer: #2a2e35;
  --scrollbar: #2a2e35;
  --scrollbar-track: #16181d;
  --scrollbar-thumb: #3a3f47;
  --searchbar-border-color: #2a2e35;
  --searchbar-bg: #1c1f24;
  --searchbar-fg: #e6e8ea;
  --searchresults-header-fg: #e6e8ea;
  --searchresults-border-color: #2a2e35;
  --searchresults-li-bg: #1c1f24;
  --search-mark-bg: #4d6bfe;
  --quote-bg: #1e2229;
  --quote-border-color: #3a3f47;
  --table-border-color: #3a3f47;
  --code-block-bg: #1b1f26;
  --code-block-border: #2a2e35;
}
"#,
        );
    } else if sepia {
        css.push_str(
            r#":root, html, html.light, html.rust, html.coal, html.navy, html.ayu {
  --bg: #f6efe1;
  --fg: #3b3226;
  --quote-bg: #efe6d3;
  --quote-border-color: #d6c9ad;
  --table-border-color: #d6c9ad;
  --code-block-bg: #efe6d3;
}
"#,
        );
    }

    css.push_str(&format!(
        r#"
@media print {{
  @page {{
    size: {size};
    margin: {margin}mm;
  }}
  html {{
    -webkit-print-color-adjust: exact;
    print-color-adjust: exact;
  }}
  body {{
    zoom: {scale};
  }}
  /* mdBook's on-screen chrome has no place on paper. */
  .sidebar, #sidebar, .sidebar-resizer, #menu-bar, .menu-bar, nav.nav-wrapper,
  .nav-chapters, .nav-wide-wrapper, .mobile-nav-chapters, #searchbar,
  #searchresults, .search-results, .buttons, .copy-button, .theme-popup {{
    display: none !important;
  }}
  .page-wrapper, #page-wrapper, main, #content, .content {{
    margin: 0 !important;
    padding: 0 !important;
    max-width: none !important;
    width: auto !important;
    left: 0 !important;
  }}
  a {{ color: inherit !important; text-decoration: none !important; }}
  a[href^="http"]::after {{
    content: " (" attr(href) ")";
    font-size: 0.75em;
    word-break: break-all;
  }}
  h1, h2, h3, h4, h5, h6 {{
    page-break-after: avoid;
    break-after: avoid;
  }}
  pre, blockquote, table, figure, img {{
    page-break-inside: avoid;
    break-inside: avoid;
  }}
  .chapter, main > h1:not(:first-child) {{
    page-break-before: always;
    break-before: page;
  }}
  .mdepub-toc {{
    page-break-after: always;
    break-after: page;
  }}
  .mdepub-toc .toc-title {{
    text-align: center;
    margin: 0 0 1.4em;
  }}
  .mdepub-toc ol {{
    list-style: none;
    padding-left: 0;
    margin: 0;
  }}
  .mdepub-toc ol ol {{
    padding-left: 1.4em;
  }}
  .mdepub-toc li {{
    margin: 0.5em 0;
  }}
  .mdepub-toc ol ol {{
    margin-top: 0.2em;
    font-size: 0.92em;
  }}
  .mdepub-toc > ol > li > a {{
    font-weight: 500;
  }}
  .mdepub-toc ol ol a {{
    color: #444444;
  }}
  .mdepub-toc a {{
    text-decoration: none;
  }}
}}
{extra}
"#,
        extra = opts.extra_css
    ));

    css
}

/// Strip tags and decode the few entities mdBook emits inside headings.
fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

/// Minimal HTML escaping for text and attribute values we emit ourselves.
fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Read the `id` attribute out of an opening tag.
fn attr_id(open_tag: &str) -> Option<String> {
    let at = open_tag.find("id=\"")?;
    let rest = &open_tag[at + 4..];
    let end = rest.find('"')?;
    let value = &rest[..end];
    (!value.is_empty()).then(|| value.to_string())
}

/// Every `<h1>`..`<h3>` inside `<main>`, as `(level, id, title)`.
///
/// mdBook's print page emits the chapter title as an `<h1>` and de-duplicates
/// ids across the whole document, so reading them back out of the generated
/// HTML is the only way to get anchors that are guaranteed to resolve.
fn extract_headings(html: &str) -> Vec<(u8, String, String)> {
    let start = html.find("<main>").map(|p| p + 6).unwrap_or(0);
    let end = html[start..]
        .find("</main>")
        .map(|p| start + p)
        .unwrap_or(html.len());
    let main = &html[start..end];

    let bytes = main.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i + 3 < bytes.len() {
        let is_heading = bytes[i] == b'<'
            && bytes[i + 1] == b'h'
            && (b'1'..=b'3').contains(&bytes[i + 2])
            && (bytes[i + 3] == b' ' || bytes[i + 3] == b'>');
        if !is_heading {
            i += 1;
            continue;
        }
        let level = bytes[i + 2] - b'0';
        let Some(tag_end) = main[i..].find('>').map(|p| i + p) else {
            break;
        };
        let Some(id) = attr_id(&main[i..=tag_end]) else {
            i = tag_end + 1;
            continue;
        };
        let close = format!("</h{level}>");
        let content_start = tag_end + 1;
        let content_end = main[content_start..]
            .find(&close)
            .map(|p| content_start + p)
            .unwrap_or(content_start);
        // Only top-level headings are link targets worth listing; skip the
        // anchor wrapper mdBook puts inside every heading.
        let title = plain_text(&main[content_start..content_end]);
        if !title.is_empty() {
            out.push((level, id, title));
        }
        i = content_end + close.len();
    }

    out
}

/// The contents section injected at the top of `print.html`.
pub fn toc_section(headings: &[(u8, String, String)], max_depth: u32) -> String {
    let max_level = max_depth.clamp(1, 3) as u8;

    // Normalise the levels first. A document can start at `<h2>` and a
    // hand-written page can jump 1 -> 3; either would otherwise emit a nested
    // `<ol>` with no `<li>` to hang it off.
    let mut items: Vec<(u8, &str, &str)> = Vec::new();
    let mut previous = 1u8;
    for (level, id, title) in headings {
        if *level > max_level || title.is_empty() {
            continue;
        }
        let mut level = (*level).max(1);
        if items.is_empty() {
            level = 1;
        } else if level > previous + 1 {
            level = previous + 1;
        }
        items.push((level, id.as_str(), title.as_str()));
        previous = level;
    }
    if items.is_empty() {
        return String::new();
    }

    let mut list = String::from("<ol>\n");
    let mut depth = 1u8;
    let mut li_open = false;

    for (level, id, title) in &items {
        while *level > depth {
            // A nested list belongs inside the `<li>` that is already open.
            list.push_str("<ol>\n");
            depth += 1;
            li_open = false;
        }
        while *level < depth {
            if li_open {
                list.push_str("</li>\n");
                li_open = false;
            }
            list.push_str("</ol>\n</li>\n");
            depth -= 1;
        }
        if li_open {
            list.push_str("</li>\n");
        }
        list.push_str(&format!(
            "<li><a href=\"#{}\">{}</a>",
            escape_markup(id),
            escape_markup(title)
        ));
        li_open = true;
    }

    while depth > 1 {
        if li_open {
            list.push_str("</li>\n");
            li_open = false;
        }
        list.push_str("</ol>\n</li>\n");
        depth -= 1;
    }
    if li_open {
        list.push_str("</li>\n");
    }
    list.push_str("</ol>\n");

    format!(
        "<section class=\"mdepub-toc\" epub:type=\"toc\">\n<h1 class=\"toc-title\">目录</h1>\n{list}</section>\n"
    )
}

/// Inject the print stylesheet (and optionally a contents page) into mdBook's
/// `print.html`.
///
/// Doing this by rewriting the HTML (rather than going through mdBook's
/// `additional-css`) keeps everything inside the build directory, so relative
/// asset paths keep working over `file://` and no stray files appear next to
/// the user's project.
pub fn inject_print_css(
    source_html: &Path,
    css: &str,
    contents: Option<&str>,
    out_html: &Path,
) -> Result<()> {
    let html = std::fs::read_to_string(source_html)?;

    // Contents first, so the offsets used for the stylesheet stay valid.
    let with_toc = match contents {
        Some(block) if !block.is_empty() => match html.find("<main>") {
            Some(pos) => {
                let at = pos + "<main>".len();
                format!("{}{}{}", &html[..at], block, &html[at..])
            }
            None => html,
        },
        _ => html,
    };

    let tag = format!("<style>\n{css}\n</style>\n");
    let patched = if let Some(pos) = with_toc.rfind("</head>") {
        let mut s = String::with_capacity(with_toc.len() + tag.len());
        s.push_str(&with_toc[..pos]);
        s.push_str(&tag);
        s.push_str(&with_toc[pos..]);
        s
    } else {
        format!("{tag}{with_toc}")
    };
    std::fs::write(out_html, patched)?;
    Ok(())
}

/// Print the already-built book to `out_path`.
pub fn export(
    built: &BuiltBook,
    opts: &PdfOptions,
    toc_page: bool,
    toc_depth: u32,
    out_path: &Path,
    progress: ProgressFn<'_>,
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();

    let source_page = if built.print_html.is_file() {
        built.print_html.clone()
    } else {
        let index = built.html_dir.join("index.html");
        if !index.is_file() {
            return Err(Error::Pdf(format!(
                "找不到可打印的 HTML：{}",
                built.html_dir.display()
            )));
        }
        warnings.push("已回退到 index.html 生成 PDF（缺少 print.html）。".to_string());
        index
    };

    let page = built.html_dir.join("print-mdepub.html");
    let contents = if toc_page {
        let source = std::fs::read_to_string(&source_page).unwrap_or_default();
        let block = toc_section(&extract_headings(&source), toc_depth);
        if block.is_empty() {
            warnings.push("PDF 目录页未生成：未能从打印页中提取到标题。".to_string());
        }
        Some(block)
    } else {
        None
    };
    inject_print_css(&source_page, &print_stylesheet(opts), contents.as_deref(), &page)?;

    progress(Progress::new("pdf", "正在查找可用的 Chromium 内核…", 10));
    let browser = chrome::find_browser()?;
    warnings.push(format!("使用 {} 生成 PDF。", browser.display()));

    let profile = built.project.cache_dir.join("browser-profile");
    std::fs::create_dir_all(&profile)?;

    let url = chrome::file_url(&canonical(&page));
    progress(Progress::new(
        "pdf",
        "正在渲染 PDF（首次运行可能较慢）…",
        35,
    ));

    let job = PrintJob {
        browser: &browser,
        url: &url,
        output: out_path,
        header_footer: opts.header_footer,
        outline: opts.outline,
        scale: opts.scale,
        profile_dir: &profile,
    };
    chrome::print_to_pdf(&job)?;

    if !out_path.is_file() {
        return Err(Error::Pdf("PDF 文件未生成".to_string()));
    }
    Ok(warnings)
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a5_page_is_written_into_the_css() {
        let opts = PdfOptions {
            page_size: "A5".into(),
            margin_mm: 12.0,
            ..PdfOptions::default()
        };
        let css = print_stylesheet(&opts);
        assert!(css.contains("size: 148mm 210mm"));
        assert!(css.contains("margin: 12mm"));
    }

    #[test]
    fn extracts_headings_from_the_print_page() {
        let html = r##"<!doctype html><html><head></head><body>
        <nav><h2 id="nav-noise">Menu</h2></nav>
        <main>
        <h1 id="前言"><a href="#前言" class="header">前言</a></h1>
        <p>body</p>
        <h1 id="软件工程亡了"><a href="#软件工程亡了" class="header">软件工程亡了</a></h1>
        <h2 id="为什么会这样"><a href="#为什么会这样" class="header">为什么会这样</a></h2>
        </main></body></html>"##;
        let got = extract_headings(html);
        let ids: Vec<&str> = got.iter().map(|(_, id, _)| id.as_str()).collect();
        // The sidebar's `<h2>` lives outside <main> and must not be listed.
        assert_eq!(ids, vec!["前言", "软件工程亡了", "为什么会这样"]);
        assert_eq!(got[0].2, "前言");
        assert_eq!(got[0].0, 1);
        assert_eq!(got[2].0, 2);
    }

    #[test]
    fn toc_section_nests_by_heading_level() {
        let headings = vec![
            (1u8, "a".to_string(), "第一章".to_string()),
            (2u8, "b".to_string(), "小节".to_string()),
            (1u8, "c".to_string(), "第二章".to_string()),
        ];
        let html = toc_section(&headings, 3);
        assert!(html.contains("href=\"#a\""));
        assert!(html.contains("href=\"#b\""));
        assert_eq!(html.matches("<ol>").count(), html.matches("</ol>").count());
        assert_eq!(html.matches("<li>").count(), html.matches("</li>").count());
        assert_eq!(html.matches("<ol>").count(), 2, "{html}");
    }

    #[test]
    fn toc_section_respects_the_depth_limit() {
        let headings = vec![
            (1u8, "a".to_string(), "A".to_string()),
            (2u8, "b".to_string(), "B".to_string()),
            (3u8, "c".to_string(), "C".to_string()),
        ];
        let html = toc_section(&headings, 1);
        assert_eq!(html.matches("<ol>").count(), 1, "{html}");
        assert!(!html.contains("#c"), "h3 must be filtered out at depth 1");
    }

    #[test]
    fn toc_section_escapes_titles() {
        let headings = vec![(1u8, "a".to_string(), "A & <b>".to_string())];
        let html = toc_section(&headings, 3);
        assert!(html.contains("A &amp; &lt;b&gt;"), "{html}");
    }

    #[test]
    fn custom_page_size_is_used() {
        let opts = PdfOptions {
            page_size: "custom".into(),
            custom_width_mm: 100.0,
            custom_height_mm: 150.0,
            ..PdfOptions::default()
        };
        assert!(print_stylesheet(&opts).contains("size: 100mm 150mm"));
    }
}
