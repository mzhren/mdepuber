//! Tauri commands — the bridge between the React UI and `mdepub-core`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use mdepub_core::model::{BookMeta, ChapterDoc, ExportReport, ExportRequest, Workspace};
use mdepub_core::Progress;

use crate::preview;
use crate::AppState;

/// Event name used for build progress.
pub const PROGRESS_EVENT: &str = "mdepub://progress";

/// Error shape handed back to JavaScript.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<mdepub_core::Error> for CommandError {
    fn from(value: mdepub_core::Error) -> Self {
        Self {
            code: value.code().to_string(),
            message: value.to_string(),
        }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(value: std::io::Error) -> Self {
        Self {
            code: "io".to_string(),
            message: value.to_string(),
        }
    }
}

type CmdResult<T> = std::result::Result<T, CommandError>;

fn cache_root(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().cache_root().to_path_buf()
}

/// Open a Markdown file, a folder of Markdown files, or an mdBook project.
#[tauri::command]
pub fn open_workspace(app: AppHandle, path: String) -> CmdResult<Workspace> {
    let path = PathBuf::from(&path);
    if !path.exists() {
        return Err(CommandError {
            code: "not_found".into(),
            message: format!("路径不存在：{path:?}"),
        });
    }
    let workspace = mdepub_core::scan(&path)?;
    // Remember where assets (images, attachments) live so the preview protocol
    // can serve them to `<img src="...">` in a chapter.
    app.state::<AppState>()
        .set_source_root(Some(PathBuf::from(&workspace.src_dir)));
    Ok(workspace)
}

/// Load one chapter for the preview pane.
///
/// The returned body never contains a YAML/TOML front matter block; the parsed
/// header comes back separately so the UI can show it as metadata.
#[tauri::command]
pub fn read_chapter(path: String) -> CmdResult<ChapterDoc> {
    Ok(mdepub_core::read_chapter(Path::new(&path))?)
}

/// Load the untouched file contents (front matter included) for the editor.
#[tauri::command]
pub fn read_chapter_raw(path: String) -> CmdResult<String> {
    Ok(mdepub_core::workspace::read_chapter(Path::new(&path))?)
}

/// Persist the editor's contents back to the file on disk.
#[tauri::command]
pub fn write_chapter(path: String, content: String) -> CmdResult<()> {
    Ok(mdepub_core::workspace::write_chapter(Path::new(&path), &content)?)
}

/// Where a book opened from `source` should be written by default.
#[tauri::command]
pub fn default_output_dir(source: String) -> String {
    // Always suggest a sibling of the source, so exporting never drops files
    // inside the book the user is working on.
    let path = PathBuf::from(&source);
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Run a full export, streaming progress on [`PROGRESS_EVENT`].
#[tauri::command]
pub async fn export_book(app: AppHandle, req: ExportRequest) -> CmdResult<ExportReport> {
    let cache = cache_root(&app);
    let emitter = app.clone();

    let joined = tauri::async_runtime::spawn_blocking(move || {
        mdepub_core::export_book(&req, &cache, &|progress: Progress| {
            let _ = emitter.emit(PROGRESS_EVENT, progress);
        })
    })
    .await;

    match joined {
        Ok(Ok(report)) => Ok(report),
        Ok(Err(err)) => Err(err.into()),
        Err(err) => Err(CommandError {
            code: "join".into(),
            message: format!("导出任务异常结束：{err}"),
        }),
    }
}

/// What the UI needs to point its preview iframe at the built book.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewInfo {
    /// e.g. `mdepub://localhost`
    pub base_url: String,
    /// Ready-to-use iframe `src`.
    pub index_url: String,
    pub html_dir: String,
    pub chapter_count: usize,
    pub warnings: Vec<String>,
}

/// Build the mdBook HTML output and make it available on the preview protocol.
#[tauri::command]
pub async fn build_preview(
    app: AppHandle,
    source: String,
    meta: BookMeta,
    #[allow(non_snake_case)] excluded: Option<Vec<String>>,
) -> CmdResult<PreviewInfo> {
    let excluded = excluded.unwrap_or_default();
    let cache = cache_root(&app);
    let emitter = app.clone();
    let source_path = PathBuf::from(&source);

    let joined = tauri::async_runtime::spawn_blocking(move || {
        mdepub_core::build_preview(&source_path, &meta, &cache, &excluded, &|progress: Progress| {
            let _ = emitter.emit(PROGRESS_EVENT, progress);
        })
    })
    .await;

    let built = match joined {
        Ok(Ok(built)) => built,
        Ok(Err(err)) => return Err(err.into()),
        Err(err) => {
            return Err(CommandError {
                code: "join".into(),
                message: format!("预览任务异常结束：{err}"),
            })
        }
    };

    let html_dir = built.html_dir.clone();
    let chapter_count = built.chapter_count();
    let warnings = built.warnings.clone();

    app.state::<AppState>().set_preview_root(html_dir.clone());

    let base_url = preview::base_url().to_string();
    Ok(PreviewInfo {
        index_url: format!("{base_url}/index.html"),
        base_url,
        html_dir: html_dir.display().to_string(),
        chapter_count,
        warnings,
    })
}

/// The URL prefix used by the preview protocol.
#[tauri::command]
pub fn preview_base() -> String {
    preview::base_url().to_string()
}

/// Open a produced file, or reveal it in the OS file manager.
#[tauri::command]
pub fn open_path(path: String, reveal: bool) -> CmdResult<()> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(CommandError {
            code: "not_found".into(),
            message: format!("文件不存在：{path}"),
        });
    }

    #[cfg(target_os = "macos")]
    let result = {
        let mut cmd = std::process::Command::new("open");
        if reveal {
            cmd.arg("-R");
        }
        cmd.arg(&target).spawn()
    };

    #[cfg(target_os = "windows")]
    let result = {
        if reveal {
            std::process::Command::new("explorer")
                .arg(format!("/select,{}", target.display()))
                .spawn()
        } else {
            std::process::Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(&target)
                .spawn()
        }
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let result = {
        let arg = if reveal {
            target.parent().map(Path::to_path_buf).unwrap_or(target.clone())
        } else {
            target.clone()
        };
        std::process::Command::new("xdg-open").arg(arg).spawn()
    };

    result.map_err(|e| CommandError {
        code: "open_failed".into(),
        message: format!("无法打开 {}：{e}", target.display()),
    })?;
    Ok(())
}

/// A path handed to the app on the command line, if any.
#[tauri::command]
pub fn initial_path(app: AppHandle) -> Option<String> {
    app.state::<AppState>().initial_path()
}

/// Human-readable platform information for the status bar.
#[tauri::command]
pub fn environment_info() -> serde_json::Value {
    let browser = mdepub_core::chrome::find_browser();
    serde_json::json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "appVersion": env!("CARGO_PKG_VERSION"),
        "pdfEngine": match &browser {
            Ok(path) => path.display().to_string(),
            Err(_) => String::new(),
        },
        "pdfAvailable": browser.is_ok(),
    })
}
