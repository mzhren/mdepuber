//! mdepuber — a small desktop studio for turning Markdown (and mdBook projects)
//! into EPUB / PDF e-books.
//!
//! The heavy lifting lives in `mdepub-core`; this crate wires it to a Tauri
//! window, a private `mdepub://` protocol for the mdBook preview, and a handful
//! of commands the React front-end calls.

mod commands;
mod preview;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::Manager;

/// Shared application state.
pub struct AppState {
    /// Root directory for per-project build caches.
    cache_root: PathBuf,
    /// Directory currently served by the `mdepub://` preview protocol.
    preview_root: Mutex<Option<PathBuf>>,
    /// Source directory of the open workspace, so relative images referenced by
    /// a chapter can be served to the preview.
    source_root: Mutex<Option<PathBuf>>,
    /// Path passed on the command line (macOS "Open With", `mdepuber book.md`).
    initial_path: Option<PathBuf>,
}

impl AppState {
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    pub fn preview_root(&self) -> Option<PathBuf> {
        self.preview_root.lock().ok().and_then(|guard| guard.clone())
    }

    pub fn set_preview_root(&self, path: PathBuf) {
        if let Ok(mut guard) = self.preview_root.lock() {
            *guard = Some(path);
        }
    }

    pub fn source_root(&self) -> Option<PathBuf> {
        self.source_root.lock().ok().and_then(|guard| guard.clone())
    }

    pub fn set_source_root(&self, path: Option<PathBuf>) {
        if let Ok(mut guard) = self.source_root.lock() {
            *guard = path;
        }
    }

    pub fn initial_path(&self) -> Option<String> {
        self.initial_path
            .as_ref()
            .map(|path| path.display().to_string())
    }
}

/// First CLI argument that points at something that exists.
fn initial_path_from_args() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|candidate| candidate.exists())
}

/// Entry point used by `main.rs` and by the mobile/desktop shell.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // `MDEPUBER_CACHE_DIR` makes the build cache relocatable, which is
            // handy for portable installs and for testing in a sandbox.
            let cache_root = match std::env::var_os("MDEPUBER_CACHE_DIR") {
                Some(dir) if !dir.is_empty() => PathBuf::from(dir),
                _ => app.path().app_data_dir()?.join("projects"),
            };
            std::fs::create_dir_all(&cache_root)?;
            app.manage(AppState {
                cache_root,
                preview_root: Mutex::new(None),
                source_root: Mutex::new(None),
                initial_path: initial_path_from_args(),
            });
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("mdepub", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            // Serving files does blocking I/O; keep it off the webview thread.
            std::thread::spawn(move || {
                responder.respond(preview::handle(&app, &request));
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_workspace,
            commands::read_chapter,
            commands::read_chapter_raw,
            commands::write_chapter,
            commands::default_output_dir,
            commands::export_book,
            commands::build_preview,
            commands::preview_base,
            commands::open_path,
            commands::environment_info,
            commands::initial_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mdepuber");
}
