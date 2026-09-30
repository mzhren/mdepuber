//! Locating and driving a Chromium-based browser for HTML -> PDF printing.
//!
//! We deliberately shell out to the user's own Chrome/Chromium/Edge instead of
//! bundling a 300 MB rendering engine: it keeps the app small and gives us the
//! system font stack (which is what makes CJK PDFs look right).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// How long a single print run may take before we give up on the browser.
const PRINT_TIMEOUT_SECS: u64 = 180;

/// Environment variables that can pin the browser, most specific first.
const ENV_KEYS: &[&str] = &["MDEPUB_BROWSER", "CHROME_PATH", "CHROMIUM_PATH", "PUPPETEER_EXECUTABLE_PATH"];

#[cfg(target_os = "macos")]
const CANDIDATES: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    "/Applications/Vivaldi.app/Contents/MacOS/Vivaldi",
    "/Applications/Arc.app/Contents/MacOS/Arc",
];

#[cfg(target_os = "windows")]
const CANDIDATES: &[&str] = &[
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
];

#[cfg(all(unix, not(target_os = "macos")))]
const CANDIDATES: &[&str] = &[
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/usr/bin/microsoft-edge",
    "/snap/bin/chromium",
    "/var/lib/flatpak/exports/bin/com.google.Chrome",
];

const PATH_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
    "microsoft-edge",
    "msedge",
    "brave-browser",
];

/// Find a usable Chromium binary, or explain what we looked for.
pub fn find_browser() -> Result<PathBuf> {
    let mut tried: Vec<String> = Vec::new();

    for key in ENV_KEYS {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            let path = PathBuf::from(value);
            if path.is_file() {
                return Ok(path);
            }
            tried.push(format!("${key}={value}"));
        }
    }

    for candidate in CANDIDATES {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Ok(path);
        }
        tried.push((*candidate).to_string());
    }

    for name in PATH_NAMES {
        if let Some(found) = which(name) {
            return Ok(found);
        }
        tried.push((*name).to_string());
    }

    Err(Error::BrowserNotFound(tried.join("、 ")))
}

fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// Options for one print run.
pub struct PrintJob<'a> {
    pub browser: &'a Path,
    /// `file://` URL of the page to print.
    pub url: &'a str,
    pub output: &'a Path,
    pub header_footer: bool,
    pub outline: bool,
    pub scale: f32,
    /// Directory used for the throw-away browser profile.
    pub profile_dir: &'a Path,
}

/// Print a local HTML file to PDF.
///
/// Tries modern headless first and falls back to the legacy headless shell,
/// because `--print-to-pdf` support has moved around between Chrome versions.
pub fn print_to_pdf(job: &PrintJob<'_>) -> Result<()> {
    if let Some(parent) = job.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(job.output);

    let mut last_error = String::new();
    for headless in ["--headless=new", "--headless=old", "--headless"] {
        match run_once(job, headless) {
            Ok(()) => return Ok(()),
            Err(e) => last_error = e.to_string(),
        }
    }

    Err(Error::Pdf(format!(
        "Chromium 打印失败（已尝试 new/old headless）：{last_error}"
    )))
}

fn run_once(job: &PrintJob<'_>, headless: &str) -> Result<()> {
    let profile = job.profile_dir.join(headless.trim_start_matches('-').replace('=', "-"));
    std::fs::create_dir_all(&profile)?;

    // Chrome spawns helper processes that can outlive the parent and keep
    // inherited pipes open, so `Command::output()` may never return. Send both
    // streams to a file and supervise the child with our own deadline.
    let log_path = profile.join("browser.log");
    let log = std::fs::File::create(&log_path)?;
    let log_err = log.try_clone()?;

    let mut cmd = Command::new(job.browser);
    cmd.arg(headless)
        .arg("--disable-gpu")
        .arg("--no-sandbox")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg("--disable-background-networking")
        .arg("--allow-file-access-from-files")
        .arg("--disable-sync")
        .arg("--disable-translate")
        .arg("--metrics-recording-only")
        .arg("--mute-audio")
        .arg("--hide-scrollbars")
        .arg("--run-all-compositor-stages-before-draw")
        .arg("--virtual-time-budget=30000")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--print-to-pdf={}", job.output.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));

    if job.header_footer {
        cmd.arg("--print-to-pdf-header-footer");
    } else {
        cmd.arg("--no-pdf-header-footer").arg("--print-to-pdf-no-header");
    }
    if job.outline {
        cmd.arg("--generate-pdf-document-outline");
    }

    cmd.arg(job.url);

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Pdf(format!("无法启动浏览器 {}：{e}", job.browser.display())))?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(PRINT_TIMEOUT_SECS);
    let mut exited = false;
    while std::time::Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => {
                exited = true;
                break;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(120)),
            Err(e) => return Err(Error::Pdf(format!("等待浏览器退出失败：{e}"))),
        }
        // Chrome usually writes the PDF well before it tears everything down.
        if job.output.is_file() && file_is_complete(job.output) {
            break;
        }
    }

    if !exited {
        let _ = child.kill();
        let _ = child.wait();
    }

    if job.output.is_file() {
        let len = std::fs::metadata(job.output).map(|m| m.len()).unwrap_or(0);
        if len > 512 {
            return Ok(());
        }
        let _ = std::fs::remove_file(job.output);
        return Err(Error::Pdf("浏览器生成了空的 PDF".to_string()));
    }

    let log_text = std::fs::read_to_string(&log_path).unwrap_or_default();
    let tail: String = log_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .join(" / ");
    Err(Error::Pdf(format!("浏览器没有生成 PDF：{tail}")))
}

/// Chrome writes the PDF incrementally; wait until the size stops changing.
fn file_is_complete(path: &Path) -> bool {
    let first = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if first == 0 {
        return false;
    }
    std::thread::sleep(std::time::Duration::from_millis(220));
    let second = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    second == first && second > 512
}

/// Turn an absolute path into a `file://` URL Chromium accepts.
pub fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for component in path.to_string_lossy().replace('\\', "/").split('/') {
        if component.is_empty() {
            continue;
        }
        url.push('/');
        for c in component.chars() {
            match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | ':' => url.push(c),
                _ => {
                    let mut buf = [0u8; 4];
                    for byte in c.encode_utf8(&mut buf).as_bytes() {
                        url.push_str(&format!("%{byte:02X}"));
                    }
                }
            }
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_are_encoded() {
        let url = file_url(Path::new("/tmp/我的 书/print.html"));
        assert!(url.starts_with("file:///tmp/"));
        assert!(url.ends_with("/print.html"));
        assert!(!url.contains(' '));
    }
}
