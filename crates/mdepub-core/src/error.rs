//! Error type shared by the whole conversion pipeline.

/// Result alias used across `mdepub-core`.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while turning Markdown into an e-book.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("路径不存在：{0}")]
    NotFound(String),

    #[error("不支持的文件类型：{0}（只支持 .md / .markdown / 文件夹 / mdBook 项目）")]
    UnsupportedSource(String),

    #[error("没有在该路径下找到任何 Markdown 文件：{0}")]
    NoChapters(String),

    #[error("mdBook 构建失败：{0}")]
    MdBook(#[from] anyhow::Error),

    #[error("EPUB 生成失败：{0}")]
    Epub(String),

    #[error("PDF 生成失败：{0}")]
    Pdf(String),

    #[error("没有找到可用的 Chrome / Chromium / Edge，无法生成 PDF。已尝试：{0}")]
    BrowserNotFound(String),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("序列化错误：{0}")]
    Serde(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Convenience constructor for ad-hoc messages.
    pub fn other(msg: impl Into<String>) -> Self {
        Error::Other(msg.into())
    }

    /// Short machine-readable code, handy for the frontend.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotFound(_) => "not_found",
            Error::UnsupportedSource(_) => "unsupported_source",
            Error::NoChapters(_) => "no_chapters",
            Error::MdBook(_) => "mdbook",
            Error::Epub(_) => "epub",
            Error::Pdf(_) => "pdf",
            Error::BrowserNotFound(_) => "browser_not_found",
            Error::Io(_) => "io",
            Error::Serde(_) => "serde",
            Error::Other(_) => "other",
        }
    }

    /// Serialize into the `{code, message}` shape the frontend expects.
    pub fn to_payload(&self) -> serde_json::Value {
        serde_json::json!({ "code": self.code(), "message": self.to_string() })
    }
}

impl From<epub_builder::Error> for Error {
    fn from(value: epub_builder::Error) -> Self {
        Error::Epub(value.to_string())
    }
}
