//! Serving the mdBook HTML output to the preview iframe.
//!
//! mdBook's output is a small static website (HTML + CSS + JS + fonts), so the
//! cleanest way to show a faithful preview is to serve it over a private URI
//! scheme and point an `<iframe>` at it. That gives us mdBook's own theme,
//! sidebar, search and print view for free.

use std::path::{Component, Path, PathBuf};

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime};

use crate::AppState;

/// Extra MIME types mdBook emits that `mime_guess` may not know about.
fn mime_for(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("woff2") => "font/woff2".to_string(),
        Some("woff") => "font/woff".to_string(),
        Some("ttf") => "font/ttf".to_string(),
        Some("otf") => "font/otf".to_string(),
        Some("js" | "mjs") => "text/javascript; charset=utf-8".to_string(),
        Some("css") => "text/css; charset=utf-8".to_string(),
        Some("html" | "htm") => "text/html; charset=utf-8".to_string(),
        Some("json" | "map") => "application/json; charset=utf-8".to_string(),
        Some("svg") => "image/svg+xml".to_string(),
        _ => mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
    }
}

fn error_response(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><body style=\"font:14px/1.6 -apple-system,system-ui,sans-serif;padding:32px;color:#9aa0a6\">{message}</body>"
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(body.into_bytes())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

/// Reject anything that tries to escape the preview root.
fn safe_join(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for part in relative.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        let candidate = Path::new(part);
        for component in candidate.components() {
            match component {
                Component::Normal(seg) => out.push(seg),
                _ => return None,
            }
        }
    }
    Some(out)
}

/// Handle one `mdepub://` request.
pub fn handle<R: Runtime>(app: &AppHandle<R>, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let state = app.state::<AppState>();

    let raw_path = request.uri().path().to_string();
    let decoded = percent_encoding::percent_decode_str(raw_path.trim_start_matches('/'))
        .decode_utf8_lossy()
        .to_string();

    // `/@source/<absolute path>` serves assets that live next to the Markdown
    // (images and the like), so the chapter preview can show them. Anything
    // outside the workspace source directory is refused.
    if let Some(requested) = decoded.strip_prefix("@source") {
        return serve_workspace_asset(state.source_root().as_deref(), requested);
    }

    let root = match state.preview_root() {
        Some(root) => root,
        None => {
            return error_response(
                StatusCode::NOT_FOUND,
                "还没有构建任何书籍。请先在左侧打开一个 Markdown 文件或文件夹，然后点击「构建预览」。",
            )
        }
    };

    let relative = if decoded.is_empty() {
        "index.html".to_string()
    } else if decoded.ends_with('/') {
        format!("{decoded}index.html")
    } else {
        decoded
    };

    let Some(full) = safe_join(&root, &relative) else {
        return error_response(StatusCode::FORBIDDEN, "非法的预览路径。");
    };

    match std::fs::read(&full) {
        Ok(bytes) => {
            let mime = mime_for(&full);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::CACHE_CONTROL, "no-store")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(bytes)
                .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "响应构建失败"))
        }
        Err(_) => error_response(
            StatusCode::NOT_FOUND,
            &format!("预览文件不存在：{}", full.display()),
        ),
    }
}

/// Resolve `requested` inside `root`, refusing anything that escapes it.
///
/// Both sides are canonicalised, so `..` segments and symlinks cannot be used to
/// read arbitrary files off the disk.
fn resolve_asset(root: &Path, requested: &str) -> Result<PathBuf, (StatusCode, String)> {
    let root = root
        .canonicalize()
        .map_err(|e| (StatusCode::NOT_FOUND, format!("工作区目录不可访问：{e}")))?;
    let target = Path::new(requested)
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, format!("找不到资源：{requested}")))?;
    if !target.starts_with(&root) {
        return Err((
            StatusCode::FORBIDDEN,
            format!("该资源不在当前工作区内：{}", target.display()),
        ));
    }
    Ok(target)
}

/// Serve a file from the open workspace, refusing anything outside it.
fn serve_workspace_asset(root: Option<&Path>, requested: &str) -> Response<Vec<u8>> {
    let Some(root) = root else {
        return error_response(StatusCode::NOT_FOUND, "当前没有打开的工作区。");
    };
    let target = match resolve_asset(root, requested) {
        Ok(target) => target,
        Err((status, message)) => return error_response(status, &message),
    };
    match std::fs::read(&target) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime_for(&target))
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(bytes)
            .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "响应构建失败")),
        Err(e) => error_response(
            StatusCode::NOT_FOUND,
            &format!("读取资源失败：{}（{e}）", target.display()),
        ),
    }
}

/// The URL prefix the webview must use for the preview iframe.
pub fn base_url() -> &'static str {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        "mdepub://localhost"
    } else {
        "http://mdepub.localhost"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_files_inside_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("images")).unwrap();
        let image = dir.path().join("images/a.png");
        std::fs::write(&image, b"png").unwrap();

        let resolved = resolve_asset(dir.path(), &image.to_string_lossy()).unwrap();
        assert_eq!(resolved, image.canonicalize().unwrap());
    }

    #[test]
    fn rejects_path_traversal_out_of_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("book");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::write(dir.path().join("secret.txt"), b"top secret").unwrap();

        let escape = inside.join("../secret.txt");
        let err = resolve_asset(&inside, &escape.to_string_lossy()).unwrap_err();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
        assert!(err.1.contains("不在当前工作区内"), "{}", err.1);
    }

    #[test]
    fn rejects_symlinks_pointing_outside() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("book");
        std::fs::create_dir_all(&inside).unwrap();
        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, b"top secret").unwrap();

        #[cfg(unix)]
        {
            let link = inside.join("link.txt");
            if std::os::unix::fs::symlink(&secret, &link).is_ok() {
                let err = resolve_asset(&inside, &link.to_string_lossy()).unwrap_err();
                assert_eq!(err.0, StatusCode::FORBIDDEN, "symlink escaped the root");
            }
        }
    }

    #[test]
    fn missing_files_report_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.png");
        let err = resolve_asset(dir.path(), &missing.to_string_lossy()).unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }

    #[test]
    fn mime_types_cover_mdbook_assets() {
        assert!(mime_for(Path::new("a.css")).starts_with("text/css"));
        assert!(mime_for(Path::new("a.js")).starts_with("text/javascript"));
        assert_eq!(mime_for(Path::new("a.woff2")), "font/woff2");
        assert_eq!(mime_for(Path::new("a.png")), "image/png");
    }

    #[test]
    fn safe_join_blocks_escaping_the_preview_root() {
        let root = Path::new("/tmp/preview");
        assert!(safe_join(root, "index.html").is_some());
        assert!(safe_join(root, "chapter/one.html").is_some());
        assert!(safe_join(root, "../../etc/passwd").is_none());
    }
}
