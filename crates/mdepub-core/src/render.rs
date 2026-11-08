//! Driving mdBook itself: load the project, run the built-in HTML renderer, and
//! hand the preprocessed book to the EPUB writer.

use std::path::{Path, PathBuf};

use std::collections::{HashMap, HashSet};

use mdbook_core::book::{Book, BookItem};
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

/// Drops chapters the user removed from the workspace file list.
///
/// This is deliberately a *render-time* filter rather than something that edits
/// `SUMMARY.md`: the source files on disk are never modified, and the exclusion
/// applies uniformly to the HTML preview, the PDF and the EPUB.
struct ExclusionPreprocessor {
    /// Normalised, source-relative chapter paths.
    excluded: HashSet<String>,
}

impl ExclusionPreprocessor {
    fn new(excluded: &[String]) -> Self {
        Self {
            excluded: excluded.iter().map(|path| normalise_rel(path)).collect(),
        }
    }

    fn is_excluded(&self, chapter: &mdbook_core::book::Chapter) -> bool {
        // `source_path` is the real file name (`README.md`), while `path` has
        // already been rewritten by mdBook's index preprocessor (`index.md`).
        // Match against either so exclusions survive that rewrite.
        [chapter.source_path.as_ref(), chapter.path.as_ref()]
            .into_iter()
            .flatten()
            .any(|path| self.excluded.contains(&normalise_rel(&path.to_string_lossy())))
    }
}

impl Preprocessor for ExclusionPreprocessor {
    fn name(&self) -> &str {
        "mdepuber-exclude"
    }

    fn run(&self, _ctx: &PreprocessorContext, mut book: Book) -> anyhow::Result<Book> {
        fn retain(items: &mut Vec<BookItem>, pre: &ExclusionPreprocessor) -> usize {
            let before = items.len();
            items.retain(|item| match item {
                BookItem::Chapter(chapter) => !pre.is_excluded(chapter),
                _ => true,
            });
            let mut removed = before - items.len();
            for item in items.iter_mut() {
                if let BookItem::Chapter(chapter) = item {
                    removed += retain(&mut chapter.sub_items, pre);
                }
            }
            removed
        }
        retain(&mut book.items, self);
        Ok(book)
    }
}

/// Normalise a relative path for comparison: forward slashes, no `./` prefix.
fn normalise_rel(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

/// Reorders sibling chapters the way the user arranged them in the outline.
///
/// Like [`ExclusionPreprocessor`] this is a render-time transformation: the
/// files on disk keep their names and places, only the book tree mdBook walks
/// (and therefore the HTML preview, the PDF page order and the EPUB spine) is
/// rearranged. Reordering happens *between siblings only* — each contiguous
/// run of chapter items is sorted by the requested rank — so nesting, part
/// titles and separators keep their structure.
struct OrderPreprocessor {
    /// Requested order as normalised source-relative paths → position.
    rank: HashMap<String, usize>,
}

impl OrderPreprocessor {
    fn new(order: &[String]) -> Self {
        Self {
            rank: order
                .iter()
                .enumerate()
                .map(|(i, path)| (normalise_rel(path), i))
                .collect(),
        }
    }
}

impl Preprocessor for OrderPreprocessor {
    fn name(&self) -> &str {
        "mdepuber-order"
    }

    fn run(&self, _ctx: &PreprocessorContext, mut book: Book) -> anyhow::Result<Book> {
        apply_chapter_order(&mut book.items, &self.rank);
        Ok(book)
    }
}

/// Sort every contiguous run of chapter items among its siblings by `rank`,
/// then recurse into each chapter's children. Items that are not in `rank`
/// (part titles, separators, unknown chapters) keep their relative order.
fn apply_chapter_order(items: &mut Vec<BookItem>, rank: &HashMap<String, usize>) {
    // Contiguous [start, end) runs of `BookItem::Chapter`.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if matches!(items[i], BookItem::Chapter(_)) {
            let start = i;
            while i < items.len() && matches!(items[i], BookItem::Chapter(_)) {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }

    let rank_of = |item: &BookItem| match item {
        BookItem::Chapter(chapter) => [chapter.source_path.as_ref(), chapter.path.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|path| rank.get(&normalise_rel(&path.to_string_lossy())).copied())
            .unwrap_or(usize::MAX),
        _ => usize::MAX,
    };
    for (start, end) in runs {
        // `sort_by_key` is stable, so unranked chapters stay put.
        items[start..end].sort_by_key(rank_of);
    }

    for item in items.iter_mut() {
        if let BookItem::Chapter(chapter) = item {
            apply_chapter_order(&mut chapter.sub_items, rank);
        }
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
    excluded: &[String],
    order: &[String],
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
    if !order.is_empty() {
        mdbook.with_preprocessor(OrderPreprocessor::new(order));
    }
    if !excluded.is_empty() {
        mdbook.with_preprocessor(ExclusionPreprocessor::new(excluded));
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chapter(name: &str, path: &str) -> BookItem {
        BookItem::Chapter(mdbook_core::book::Chapter {
            name: name.to_string(),
            path: Some(PathBuf::from(path)),
            source_path: Some(PathBuf::from(path)),
            ..Default::default()
        })
    }

    pub(super) fn paths(items: &[BookItem]) -> Vec<String> {
        let mut out = Vec::new();
        for item in items {
            if let BookItem::Chapter(ch) = item {
                if let Some(p) = ch.path.as_ref() {
                    out.push(p.to_string_lossy().to_string());
                }
                out.extend(paths(&ch.sub_items));
            }
        }
        out
    }

    #[test]
    fn sibling_runs_are_reordered() {
        let mut items = vec![chapter("A", "a.md"), chapter("B", "b.md"), chapter("C", "c.md")];
        apply_chapter_order(
            &mut items,
            &HashMap::from([("c.md".into(), 0), ("a.md".into(), 1), ("b.md".into(), 2)]),
        );
        assert_eq!(paths(&items), vec!["c.md", "a.md", "b.md"]);
    }

    #[test]
    fn separators_and_part_titles_break_the_runs() {
        let mut items = vec![
            chapter("A", "a.md"),
            chapter("B", "b.md"),
            BookItem::PartTitle("第二部分".into()),
            chapter("C", "c.md"),
        ];
        apply_chapter_order(
            &mut items,
            &HashMap::from([("c.md".into(), 0), ("b.md".into(), 1), ("a.md".into(), 2)]),
        );
        // a/b swap inside their run, the part title stays, c stays first after it.
        assert_eq!(paths(&items), vec!["b.md", "a.md", "c.md"]);
    }

    #[test]
    fn nested_children_sort_within_their_parent() {
        let mut child_b = mdbook_core::book::Chapter {
            name: "B1".into(),
            path: Some(PathBuf::from("b/1.md")),
            source_path: Some(PathBuf::from("b/1.md")),
            ..Default::default()
        };
        child_b.sub_items = vec![chapter("B2", "b/2.md")];
        let mut items = vec![BookItem::Chapter(child_b), chapter("A", "a.md")];
        apply_chapter_order(
            &mut items,
            &HashMap::from([("a.md".into(), 0), ("b/1.md".into(), 1), ("b/2.md".into(), 2)]),
        );
        assert_eq!(paths(&items), vec!["a.md", "b/1.md", "b/2.md"]);
    }

    #[test]
    fn missing_rank_keeps_stable_position() {
        let mut items = vec![
            chapter("A", "a.md"),
            chapter("B", "b.md"),
            chapter("C", "c.md"),
        ];
        apply_chapter_order(&mut items, &HashMap::from([("c.md".into(), 0)]));
        // Only c is ranked: it moves to the front of its run, a/b keep order.
        assert_eq!(paths(&items), vec!["c.md", "a.md", "b.md"]);
    }

    #[test]
    fn index_rewrites_are_matched_like_exclusions() {
        // mdBook's index preprocessor rewrites README.md → index.md; the rank
        // must match either path.
        let mut home = mdbook_core::book::Chapter {
            name: "Home".into(),
            path: Some(PathBuf::from("index.md")),
            source_path: Some(PathBuf::from("README.md")),
            ..Default::default()
        };
        home.sub_items = vec![chapter("X", "x.md")];
        let mut items = vec![BookItem::Chapter(home), chapter("Y", "y.md")];
        apply_chapter_order(&mut items, &HashMap::from([("y.md".into(), 0), ("readme.md".into(), 1)]));
        assert_eq!(paths(&items), vec!["y.md", "index.md", "x.md"]);
    }
}

#[cfg(test)]
mod build_order_tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// End-to-end: a folder book built with a custom order must come back with
    /// the book tree (and therefore HTML/PDF/EPUB) in exactly that order.
    #[test]
    fn folder_book_respects_requested_order() {
        let src = tempfile::tempdir().unwrap();
        for name in ["alpha.md", "beta.md", "gamma.md"] {
            fs::write(src.path().join(name), format!("# {name}\n\n内容。\n")).unwrap();
        }
        let cache = tempfile::tempdir().unwrap();
        let noop = |_p: Progress| {};

        let built = build_book(
            Path::new(src.path()),
            SourceKind::Folder,
            &BookMeta::default(),
            cache.path(),
            ThemeMode::Project,
            &[],
            &[],
            &noop,
        )
        .unwrap();
        let natural: Vec<String> = super::tests::paths(&built.book.items);
        assert_eq!(natural, vec!["alpha.md", "beta.md", "gamma.md"]);

        let built = build_book(
            Path::new(src.path()),
            SourceKind::Folder,
            &BookMeta::default(),
            cache.path(),
            ThemeMode::Project,
            &[],
            &["gamma.md".into(), "alpha.md".into(), "beta.md".into()],
            &noop,
        )
        .unwrap();
        assert_eq!(
            super::tests::paths(&built.book.items),
            vec!["gamma.md", "alpha.md", "beta.md"]
        );
        // The user's folder itself is untouched.
        assert_eq!(
            fs::read_dir(src.path()).unwrap().count(),
            3,
            "source folder must not gain a SUMMARY.md"
        );
    }
}
