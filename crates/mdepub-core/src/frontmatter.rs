//! YAML (`---`) and TOML (`+++`) front matter.
//!
//! mdBook has no front matter support at all: given a blog post that starts
//! with a `---` block it renders the block as body text — the opening `---`
//! becomes a thematic break and the closing one turns the last key into a
//! setext heading. So mdepuber parses the block itself, strips it before
//! anything is rendered, and surfaces the fields it understands.
//!
//! The parser is deliberately conservative: a file only counts as having front
//! matter when the delimiter is the very first line *and* a matching closing
//! delimiter exists, so a document that merely starts with a horizontal rule
//! is left untouched.

use serde_yaml_ng::Value as Yaml;

use crate::model::{FrontMatter, FrontMatterField};

/// Which syntax delimited the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Syntax {
    Yaml,
    Toml,
}

impl Syntax {
    fn openers(self) -> &'static [&'static str] {
        match self {
            Syntax::Yaml => &["---"],
            Syntax::Toml => &["+++"],
        }
    }

    /// YAML also accepts `...` as an end-of-document marker.
    fn closers(self) -> &'static [&'static str] {
        match self {
            Syntax::Yaml => &["---", "..."],
            Syntax::Toml => &["+++"],
        }
    }
}

fn line_trimmed(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r']).trim_end()
}

/// Locate a leading front matter block.
///
/// Returns the syntax, the raw block (without delimiters) and the byte offset
/// where the body starts.
fn locate(content: &str) -> Option<(Syntax, &str, usize)> {
    let mut lines = content.split_inclusive('\n');
    let first = lines.next()?;
    let first_trimmed = line_trimmed(first);

    let syntax = [Syntax::Yaml, Syntax::Toml]
        .into_iter()
        .find(|s| s.openers().contains(&first_trimmed))?;

    let mut offset = first.len();
    let block_start = offset;

    for line in lines {
        let trimmed = line_trimmed(line);
        if syntax.closers().contains(&trimmed) {
            return Some((syntax, &content[block_start..offset], offset + line.len()));
        }
        offset += line.len();
    }

    // No closing delimiter: not front matter, just a document starting with a
    // horizontal rule.
    None
}

/// `key: value` on the first meaningful line — our fallback heuristic for YAML
/// that is valid in practice but that the parser rejects.
fn looks_like_mapping(block: &str) -> bool {
    block
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut chars = line.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && line.contains(':')
                && !line.starts_with("- ")
        })
        .unwrap_or(false)
}

/// Parse the block into the fields we understand.
fn parse_yaml(block: &str) -> Option<Yaml> {
    serde_yaml_ng::from_str::<Yaml>(block).ok()
}

fn parse_toml(block: &str) -> Option<toml::Value> {
    toml::from_str::<toml::Value>(block).ok()
}

/// Render any scalar-ish value as a single display line.
fn yaml_display(value: &Yaml) -> String {
    match value {
        Yaml::Null => String::new(),
        Yaml::Bool(b) => b.to_string(),
        Yaml::Number(n) => n.to_string(),
        Yaml::String(s) => s.trim().to_string(),
        Yaml::Sequence(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(yaml_display)
                .filter(|s| !s.is_empty())
                .collect();
            format!("[ {} ]", parts.join(", "))
        }
        Yaml::Mapping(_) => serde_json::to_string(value).unwrap_or_default(),
        Yaml::Tagged(t) => yaml_display(&t.value),
    }
}

fn yaml_string(value: &Yaml) -> Option<String> {
    let out = match value {
        Yaml::String(s) => s.trim().to_string(),
        Yaml::Number(n) => n.to_string(),
        Yaml::Bool(b) => b.to_string(),
        // `image: { url: … }` and friends.
        Yaml::Mapping(map) => ["url", "src", "path", "href"]
            .iter()
            .find_map(|key| map.get(Yaml::from(*key)).and_then(yaml_string))
            .unwrap_or_default(),
        Yaml::Sequence(items) => items.first().and_then(yaml_string).unwrap_or_default(),
        _ => String::new(),
    };
    (!out.is_empty()).then_some(out)
}

fn yaml_list(value: &Yaml) -> Vec<String> {
    match value {
        Yaml::Sequence(items) => items
            .iter()
            .filter_map(yaml_string)
            .filter(|s| !s.is_empty())
            .collect(),
        Yaml::String(s) => s
            .split([',', '，'])
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn toml_display(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.trim().to_string(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(toml_display).filter(|s| !s.is_empty()).collect();
            format!("[ {} ]", parts.join(", "))
        }
        other => other.to_string(),
    }
}

fn toml_string(value: &toml::Value) -> Option<String> {
    let out = toml_display(value);
    (!out.is_empty()).then_some(out)
}

fn toml_list(value: &toml::Value) -> Vec<String> {
    match value {
        toml::Value::Array(items) => items.iter().filter_map(toml_string).collect(),
        toml::Value::String(s) => s
            .split([',', '，'])
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

const TITLE_KEYS: &[&str] = &["title", "name"];
const DESCRIPTION_KEYS: &[&str] = &["description", "summary", "excerpt", "abstract"];
const DATE_KEYS: &[&str] = &["date", "published", "pubdate", "published_at", "created"];
const IMAGE_KEYS: &[&str] = &["image", "cover", "featured_image", "featured", "thumbnail", "banner"];
const TAG_KEYS: &[&str] = &["tags", "keywords", "categories", "category", "topics"];
const AUTHOR_KEYS: &[&str] = &["author", "authors", "creator"];
const LANG_KEYS: &[&str] = &["lang", "language", "locale"];
const DRAFT_KEYS: &[&str] = &["draft", "unpublished", "hidden"];

fn first_key<'a, V>(map: &'a [(String, V)], keys: &[&str]) -> Option<&'a V> {
    keys.iter().find_map(|key| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    })
}

fn as_bool(display: &str) -> bool {
    matches!(
        display.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "1"
    )
}

/// Everything a caller needs from a front matter block.
pub struct Parsed {
    pub front_matter: FrontMatter,
    /// Whether the block was stripped even though parsing it failed.
    pub tolerant: bool,
}

/// YAML block -> front matter. `None` means "this is not front matter".
fn from_yaml(block: &str) -> Option<(FrontMatter, bool)> {
    let entries: Vec<(String, Yaml)> = match parse_yaml(block) {
        Some(Yaml::Mapping(map)) => map
            .into_iter()
            .filter_map(|(k, v)| Some((yaml_string(&k)?, v)))
            .collect(),
        // Valid YAML that is not a mapping (or unparseable): only accept it when
        // it still looks like a `key: value` header, and say so.
        _ => {
            if !looks_like_mapping(block) {
                return None;
            }
            return Some((tolerant_yaml(block), true));
        }
    };

    let fields = entries
        .iter()
        .map(|(key, value)| FrontMatterField {
            key: key.clone(),
            value: yaml_display(value),
        })
        .collect();

    Some((
        FrontMatter {
            format: "yaml".to_string(),
            title: first_key(&entries, TITLE_KEYS).and_then(yaml_string),
            description: first_key(&entries, DESCRIPTION_KEYS).and_then(yaml_string),
            date: first_key(&entries, DATE_KEYS).and_then(yaml_string),
            image: first_key(&entries, IMAGE_KEYS).and_then(yaml_string),
            tags: first_key(&entries, TAG_KEYS).map(yaml_list).unwrap_or_default(),
            authors: first_key(&entries, AUTHOR_KEYS).map(yaml_list).unwrap_or_default(),
            language: first_key(&entries, LANG_KEYS).and_then(yaml_string),
            draft: first_key(&entries, DRAFT_KEYS)
                .and_then(yaml_string)
                .map(|v| as_bool(&v))
                .unwrap_or(false),
            fields,
        },
        false,
    ))
}

/// Fallback for prose that *looks* like a YAML header but does not parse: split
/// each line on its first colon so the block still gets stripped and shown.
fn tolerant_yaml(block: &str) -> FrontMatter {
    let mut fields: Vec<FrontMatterField> = Vec::new();
    for line in block.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
            fields.push(FrontMatterField {
                key: key.trim().to_string(),
                value,
            });
        }
    }

    let lookup = |keys: &[&str]| -> Option<String> {
        fields
            .iter()
            .find(|f| keys.iter().any(|k| f.key.eq_ignore_ascii_case(k)))
            .map(|f| f.value.clone())
            .filter(|v| !v.is_empty())
    };
    let list = |keys: &[&str]| -> Vec<String> {
        lookup(keys)
            .map(|v| {
                v.trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split([',', '，'])
                    .map(|p| p.trim().trim_matches('"').trim_matches('\'').to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };

    FrontMatter {
        format: "yaml".to_string(),
        title: lookup(TITLE_KEYS),
        description: lookup(DESCRIPTION_KEYS),
        date: lookup(DATE_KEYS),
        image: lookup(IMAGE_KEYS),
        tags: list(TAG_KEYS),
        authors: list(AUTHOR_KEYS),
        language: lookup(LANG_KEYS),
        draft: lookup(DRAFT_KEYS).map(|v| as_bool(&v)).unwrap_or(false),
        fields,
    }
}

/// TOML block -> front matter.
fn from_toml(block: &str) -> Option<(FrontMatter, bool)> {
    let Some(toml::Value::Table(table)) = parse_toml(block) else {
        return None;
    };
    let entries: Vec<(String, toml::Value)> = table.into_iter().collect();
    let fields = entries
        .iter()
        .map(|(key, value)| FrontMatterField {
            key: key.clone(),
            value: toml_display(value),
        })
        .collect();

    Some((
        FrontMatter {
            format: "toml".to_string(),
            title: first_key(&entries, TITLE_KEYS).and_then(toml_string),
            description: first_key(&entries, DESCRIPTION_KEYS).and_then(toml_string),
            date: first_key(&entries, DATE_KEYS).and_then(toml_string),
            image: first_key(&entries, IMAGE_KEYS).and_then(toml_string),
            tags: first_key(&entries, TAG_KEYS).map(toml_list).unwrap_or_default(),
            authors: first_key(&entries, AUTHOR_KEYS).map(toml_list).unwrap_or_default(),
            language: first_key(&entries, LANG_KEYS).and_then(toml_string),
            draft: first_key(&entries, DRAFT_KEYS)
                .and_then(toml_string)
                .map(|v| as_bool(&v))
                .unwrap_or(false),
            fields,
        },
        false,
    ))
}

/// Split `content` into (front matter, body).
///
/// `None` means the document has no front matter and `content` is returned
/// unchanged as the body.
pub fn split(content: &str) -> (Option<Parsed>, &str) {
    let Some((syntax, block, body_start)) = locate(content) else {
        return (None, content);
    };
    let built = match syntax {
        Syntax::Yaml => from_yaml(block),
        Syntax::Toml => from_toml(block),
    };
    let Some((front_matter, tolerant)) = built else {
        // Delimiters with content that is not a header: leave the file alone.
        return (None, content);
    };
    let body = content[body_start..].trim_start_matches(['\n', '\r']);
    (
        Some(Parsed {
            front_matter,
            tolerant,
        }),
        body,
    )
}

/// Strip a leading front matter block, returning the body.
pub fn strip(content: &str) -> &str {
    split(content).1
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOG: &str = "---\ntitle: 软件工程亡了\ndate: 2026-09-19 12:16:39\ndescription: 文章借 AI Agent 范式转移，谈软件工程“亡天下”。\nimage:\ntags: [ 编程, 技术, 人工智能, 软件工程 ]\n---\n\n朋友发给我一张图片。\n";

    #[test]
    fn parses_the_reported_blog_header() {
        let (fm, body) = split(BLOG);
        let fm = fm.expect("front matter should be detected");
        assert_eq!(fm.front_matter.format, "yaml");
        assert_eq!(fm.front_matter.title.as_deref(), Some("软件工程亡了"));
        assert_eq!(fm.front_matter.date.as_deref(), Some("2026-09-19 12:16:39"));
        assert!(fm.front_matter.description.as_deref().unwrap().contains("范式转移"));
        assert_eq!(
            fm.front_matter.tags,
            vec!["编程", "技术", "人工智能", "软件工程"]
        );
        assert!(fm.front_matter.image.is_none(), "empty image must not parse");
        assert_eq!(body.trim(), "朋友发给我一张图片。");
        assert!(!body.contains("tags"), "front matter must not leak into the body");
    }

    #[test]
    fn keeps_every_field_for_display() {
        let (fm, _) = split(BLOG);
        let fm = fm.unwrap();
        let keys: Vec<&str> = fm.front_matter.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, vec!["title", "date", "description", "image", "tags"]);
        assert_eq!(fm.front_matter.fields[4].value, "[ 编程, 技术, 人工智能, 软件工程 ]");
    }

    #[test]
    fn plain_document_starting_with_a_rule_is_untouched() {
        let doc = "---\n\n正文第一段。\n\n---\n\n正文第二段。\n";
        let (fm, body) = split(doc);
        assert!(fm.is_none());
        assert_eq!(body, doc);
    }

    #[test]
    fn document_without_a_closing_delimiter_is_untouched() {
        let doc = "---\n# 标题\n正文\n";
        let (fm, body) = split(doc);
        assert!(fm.is_none());
        assert_eq!(body, doc);
    }

    #[test]
    fn toml_front_matter_is_supported() {
        let doc = "+++\ntitle = \"TOML 标题\"\ntags = [\"a\", \"b\"]\n+++\n\n正文。\n";
        let (fm, body) = split(doc);
        let fm = fm.unwrap();
        assert_eq!(fm.front_matter.format, "toml");
        assert_eq!(fm.front_matter.title.as_deref(), Some("TOML 标题"));
        assert_eq!(fm.front_matter.tags, vec!["a", "b"]);
        assert_eq!(body.trim(), "正文。");
    }

    #[test]
    fn quoted_values_may_contain_colons() {
        let doc = "---\ntitle: \"Foo: Bar\"\n---\nbody\n";
        let (fm, body) = split(doc);
        assert_eq!(fm.unwrap().front_matter.title.as_deref(), Some("Foo: Bar"));
        assert_eq!(body, "body\n");
    }

    #[test]
    fn crlf_line_endings_are_handled() {
        let doc = "---\r\ntitle: 你好\r\n---\r\n\r\n正文\r\n";
        let (fm, body) = split(doc);
        assert_eq!(fm.unwrap().front_matter.title.as_deref(), Some("你好"));
        assert_eq!(body.trim(), "正文");
    }

    #[test]
    fn draft_and_authors_are_read() {
        let doc = "---\nauthors: [张三, 李四]\ndraft: true\n---\nbody\n";
        let (fm, _) = split(doc);
        let fm = fm.unwrap();
        assert_eq!(fm.front_matter.authors, vec!["张三", "李四"]);
        assert!(fm.front_matter.draft);
    }

    #[test]
    fn nested_image_url_is_understood() {
        let doc = "---\nimage:\n  url: images/cover.png\n---\nbody\n";
        let (fm, _) = split(doc);
        assert_eq!(fm.unwrap().front_matter.image.as_deref(), Some("images/cover.png"));
    }
}
