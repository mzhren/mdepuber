//! Driving mdBook itself: load the project, run the built-in HTML renderer, and
//! hand the preprocessed book to the EPUB writer.

use std::path::{Path, PathBuf};

use mdbook_core::book::Book;
use mdbook_core::config::Config;
use mdbook_driver::MDBook;
use mdbook_html::HtmlHandlebars;
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};

use crate::epub::EpubRenderer;
use crate::frontmatter;
use crate::error::{Error, Result};
use crate::model::{BookMeta, Progress, ProgressFn, SourceKind, ThemeMode};
use crate::project::{self, Project};

/// Strips leading YAML / TOML front matter from every chapter before any
/// renderer sees it.
///
/// mdBook has no front matter support: without this, a blog post that opens
/// with a `---` block renders that block as body text (the opening `---` turns
/// into a thematic break and the closing one into a setext heading). Registering
/// this as a real preprocessor means the HTML output — and therefore the
/// in-app preview *and* the PDF — is fixed too, not just the EPUB.
struct FrontMatterPreprocessor;

impl Preprocessor for FrontMatterPreprocessor {
    fn name(&self) -> &str {
        "mdepuber-frontmatter"
    }

    fn run(&self, _ctx: &PreprocessorContext, mut book: Book) -> anyhow::Result<Book> {
        book.for_each_chapter_mut(|chapter| {
            let body = frontmatter::strip(&chapter.content).to_string();
            if body.len() != chapter.content.len() {
                chapter.content = body;
            }
        });
        Ok(book)
    }
}

/// A book that has been loaded and rendered by mdBook.
pub struct BuiltBook {
    pub project: Project,
    pub config: Config,
    /// The book after mdBook's preprocessors ran (`{{#include}}`, index, …).
    pub book: Book,
    pub html_dir: PathBuf,
    pub print_html: PathBuf,
    pub warnings: Vec<String>,
}

impl BuiltBook {
    /// Number of real chapters (excluding part titles and separators).
    pub fn chapter_count(&self) -> usize {
        fn walk(items: &[mdbook_core::book::BookItem], n: &mut usize) {
            for item in items {
                if let mdbook_core::book::BookItem::Chapter(ch) = item {
                    if !ch.path.as_ref().map(|p| p.as_os_str().is_empty()).unwrap_or(true) {
                        *n += 1;
                    }
                    walk(&ch.sub_items, n);
                }
            }
        }
        let mut n = 0;
        walk(&self.book.items, &mut n);
        n
    }
}

/// Prepare the project and run mdBook's HTML renderer.
pub fn build_book(
    source: &Path,
    kind: SourceKind,
    meta: &BookMeta,
    cache_root: &Path,
    theme_mode: ThemeMode,
    progress: ProgressFn<'_>,
) -> Result<BuiltBook> {
    progress(Progress::new("prepare", "正在准备 mdBook 项目…", 8));
    let project = project::prepare(source, kind, meta, cache_root)?;
    let mut warnings = project.warnings.clone();

    let (config, cfg_warnings) = project::config_for(&project, meta, theme_mode)?;
    warnings.extend(cfg_warnings);

    progress(Progress::new("load", "正在读取 SUMMARY.md 与章节…", 30));
    let mut mdbook = MDBook::load_with_config(&project.root, config).map_err(Error::MdBook)?;
    // Runs for every renderer (HTML for the preview/PDF, and our EPUB path).
    mdbook.with_preprocessor(FrontMatterPreprocessor);

    progress(Progress::new("render", "正在使用 mdBook 渲染 HTML…", 55));
    mdbook
        .execute_build_process(&HtmlHandlebars::new())
        .map_err(Error::MdBook)?;

    let html_dir = project.html_dir();
    if !html_dir.is_dir() {
        return Err(Error::MdBook(anyhow::anyhow!(
            "mdBook 未生成 HTML 输出目录：{}",
            html_dir.display()
        )));
    }

    let print_html = project.print_html();
    if !print_html.is_file() {
        warnings.push(
            "mdBook 未生成 print.html（可能配置了 output.html.print.enable = false），PDF 将改用 index.html。"
                .to_string(),
        );
    }

    progress(Progress::new("preprocess", "正在运行 mdBook 预处理器…", 85));
    let (book, _ctx) = mdbook
        .preprocess_book(&EpubRenderer)
        .map_err(Error::MdBook)?;

    Ok(BuiltBook {
        project,
        config: mdbook.config.clone(),
        book,
        html_dir,
        print_html,
        warnings,
    })
}

/// Total characters in the preprocessed book (used for progress weighting).
pub fn book_char_count(book: &Book) -> usize {
    fn walk(items: &[mdbook_core::book::BookItem], n: &mut usize) {
        for item in items {
            if let mdbook_core::book::BookItem::Chapter(ch) = item {
                *n += ch.content.chars().count();
                walk(&ch.sub_items, n);
            }
        }
    }
    let mut n = 0;
    walk(&book.items, &mut n);
    n
}
