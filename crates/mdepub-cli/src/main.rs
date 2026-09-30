//! `mdepub` — headless front-end for `mdepub-core`.
//!
//! Handy for scripting, for CI, and for verifying an export without launching
//! the GUI.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use mdepub_core::model::{BookFormat, BookMeta, EpubOptions, ExportRequest, PdfOptions};
use mdepub_core::Progress;

#[derive(Parser, Debug)]
#[command(
    name = "mdepub",
    version,
    about = "把 Markdown 文件/文件夹（或 mdBook 项目）转换成 EPUB / PDF"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 列出某个路径下的章节结构。
    Scan {
        /// Markdown 文件、文件夹或 mdBook 项目目录。
        source: PathBuf,

        /// 以 JSON 输出（供脚本/CI 使用）。
        #[arg(long)]
        json: bool,
    },
    /// 输出某个章节的正文（已剥离 front matter）与解析出的头部，JSON 格式。
    Chapter {
        /// 单个 Markdown 文件路径。
        path: PathBuf,
    },
    /// 导出 EPUB / PDF。
    Export {
        /// Markdown 文件、文件夹或 mdBook 项目目录。
        source: PathBuf,

        /// 输出目录（默认与源文件同级）。
        #[arg(short, long)]
        out: Option<PathBuf>,

        /// 输出文件名（不含扩展名）。
        #[arg(short = 'n', long)]
        name: Option<String>,

        /// 生成的格式，可重复（默认 epub）。
        #[arg(short, long, value_enum)]
        format: Vec<Format>,

        /// 书名。
        #[arg(long)]
        title: Option<String>,

        /// 作者，可重复。
        #[arg(long)]
        author: Vec<String>,

        /// 语言代码，例如 zh-CN / en。
        #[arg(long, default_value = "zh-CN")]
        lang: String,

        /// 封面图片路径。
        #[arg(long)]
        cover: Option<PathBuf>,

        /// PDF 页面尺寸：A3/A4/A5/B5/Letter/Legal/custom。
        #[arg(long, default_value = "A4")]
        page_size: String,

        /// PDF 页边距（毫米）。
        #[arg(long, default_value_t = 18.0)]
        margin: f32,

        /// PDF 配色：light/dark/sepia。
        #[arg(long, default_value = "light")]
        theme: String,

        /// 排除某个章节（相对于书籍源目录的路径），可重复。
        #[arg(long = "exclude")]
        exclude: Vec<String>,

        /// 在封面之后插入一页可见的目录页（EPUB 与 PDF 都会生成）。
        #[arg(long)]
        toc_page: bool,

        /// 目录页列到第几级标题（1-3，默认 3）。
        #[arg(long, default_value_t = 3)]
        toc_depth: u32,

        /// EPUB 正文字体：serif/sans/system。
        #[arg(long, default_value = "serif")]
        font: String,

        /// 缓存目录（默认使用系统数据目录）。
        #[arg(long)]
        cache: Option<PathBuf>,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    Epub,
    Pdf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan { source, json } => scan(&source, json),
        Command::Chapter { path } => chapter(&path),
        Command::Export {
            source,
            out,
            name,
            format,
            title,
            author,
            lang,
            cover,
            page_size,
            margin,
            theme,
            font,
            exclude,
            toc_page,
            toc_depth,
            cache,
        } => export(
            source, out, name, format, title, author, lang, cover, page_size, margin, theme, font,
            exclude, toc_page, toc_depth, cache,
        ),
    }
}

fn scan(source: &PathBuf, json: bool) -> Result<()> {
    let ws = mdepub_core::scan(source).with_context(|| format!("无法打开 {}", source.display()))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&ws)?);
        return Ok(());
    }
    println!("源类型    : {:?}", ws.kind);
    println!("书名      : {}", ws.title);
    println!("源目录    : {}", ws.src_dir);
    println!("章节数    : {}", ws.chapters.len());
    println!("总词数    : {}", ws.total_words);
    for w in &ws.warnings {
        println!("提示      : {w}");
    }
    println!();
    for (i, ch) in ws.chapters.iter().enumerate() {
        println!(
            "{:>3}. {}{}  ({})",
            i + 1,
            "  ".repeat(ch.level.saturating_sub(1) as usize),
            ch.title,
            ch.rel
        );
    }
    Ok(())
}

fn chapter(path: &PathBuf) -> Result<()> {
    let doc = mdepub_core::read_chapter(path)
        .with_context(|| format!("无法读取 {}", path.display()))?;
    println!("{}", serde_json::to_string_pretty(&doc)?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn export(
    source: PathBuf,
    out: Option<PathBuf>,
    name: Option<String>,
    format: Vec<Format>,
    title: Option<String>,
    author: Vec<String>,
    lang: String,
    cover: Option<PathBuf>,
    page_size: String,
    margin: f32,
    theme: String,
    font: String,
    excluded: Vec<String>,
    toc_page: bool,
    toc_depth: u32,
    cache: Option<PathBuf>,
) -> Result<()> {
    let mut formats: Vec<BookFormat> = format
        .iter()
        .map(|f| match f {
            Format::Epub => BookFormat::Epub,
            Format::Pdf => BookFormat::Pdf,
        })
        .collect();
    if formats.is_empty() {
        formats.push(BookFormat::Epub);
    }

    let cache_root = cache.unwrap_or_else(mdepub_core::default_cache_root);

    let req = ExportRequest {
        source: source.display().to_string(),
        meta: BookMeta {
            title: title.unwrap_or_default(),
            authors: author,
            language: lang,
            cover: cover.map(|c| c.display().to_string()),
            ..BookMeta::default()
        },
        epub: EpubOptions {
            font_family: font,
            ..EpubOptions::default()
        },
        pdf: PdfOptions {
            page_size,
            margin_mm: margin,
            theme,
            ..PdfOptions::default()
        },
        formats,
        excluded,
        toc_page,
        toc_depth,
        output_dir: out.map(|o| o.display().to_string()).unwrap_or_default(),
        base_name: name.unwrap_or_default(),
    };

    let report = mdepub_core::export_book(&req, &cache_root, &|p: Progress| {
        println!("[{:>3}%] {}", p.percent, p.message);
    })
    .with_context(|| format!("导出 {} 失败", req.source))?;

    println!();
    println!("章节数    : {}", report.chapter_count);
    println!("耗时      : {} ms", report.duration_ms);
    if let Some(path) = &report.epub_path {
        println!("EPUB      : {path}  ({} KB)", report.bytes_epub / 1024);
    }
    if let Some(path) = &report.pdf_path {
        println!("PDF       : {path}  ({} KB)", report.bytes_pdf / 1024);
    }
    println!("HTML 预览 : {}", report.html_dir);
    for w in &report.warnings {
        println!("提示      : {w}");
    }
    Ok(())
}
