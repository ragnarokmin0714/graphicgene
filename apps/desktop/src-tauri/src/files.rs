//! The desktop's file IO: the autosave in the app's data folder, and project
//! files and exports through the system's open and save dialogs.
//!
//! This is the app layer the core leaves IO to (CLAUDE.md, "IO boundary"):
//! the page hands over text and bytes, and these put them on disk. Errors go
//! back to the page as text, for its status bar.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const AUTOSAVE_FILE: &str = "autosave.json";

type Result<T> = std::result::Result<T, String>;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn autosave_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(message)?
        .join(AUTOSAVE_FILE))
}

/// The autosaved project, or none before the first save.
#[tauri::command]
pub async fn read_autosave(app: AppHandle) -> Result<Option<String>> {
    match fs::read_to_string(autosave_path(&app)?) {
        Ok(json) => Ok(Some(json)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(message(error)),
    }
}

#[tauri::command]
pub async fn write_autosave(app: AppHandle, json: String) -> Result<()> {
    write_whole(&autosave_path(&app)?, json.as_bytes()).map_err(message)
}

/// A project file the user picked, as the page's `OpenedFile`.
#[derive(Serialize)]
pub struct OpenedFile {
    name: String,
    text: String,
}

/// Ask for a project file and read it; none if the user cancels.
///
/// The dialog blocks its thread until answered, which is why this and
/// `save_file` are async: Tauri runs async commands off the main thread.
#[tauri::command]
pub async fn open_project(app: AppHandle) -> Result<Option<OpenedFile>> {
    let picked = app
        .dialog()
        .file()
        .add_filter("graphicgene project", &["json"])
        .blocking_pick_file();
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(message)?;
    let text = fs::read_to_string(&path).map_err(message)?;
    Ok(Some(OpenedFile {
        name: file_name(&path),
        text,
    }))
}

/// Save the request's body where the user picks, suggesting the name in its
/// `x-file-name` header (percent-encoded). Answers the name it was saved
/// under, or none if the user cancels.
#[tauri::command]
pub async fn save_file(app: AppHandle, request: Request<'_>) -> Result<Option<String>> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the file's bytes as the request body".into());
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(percent_decode)
        .unwrap_or_else(|| "untitled".into());
    let mut dialog = app.dialog().file().set_file_name(&name);
    if let Some((label, extension)) = filter_for(&name) {
        dialog = dialog.add_filter(label, &[extension]);
    }
    let Some(picked) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(message)?;
    write_whole(&path, bytes).map_err(message)?;
    Ok(Some(file_name(&path)))
}

/// Write the file whole or not at all: into a temporary file beside it,
/// then renamed over it, so a crash mid-write cannot leave half a project.
/// `rename` replaces an existing file on Windows too.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".partial");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The save dialog's file type for a suggested name.
fn filter_for(name: &str) -> Option<(&'static str, &'static str)> {
    let (_, extension) = name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "json" => Some(("graphicgene project", "json")),
        "svg" => Some(("SVG image", "svg")),
        "png" => Some(("PNG image", "png")),
        _ => None,
    }
}

/// Undo `encodeURIComponent`: headers carry ASCII only, and a name may not be.
fn percent_decode(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                i += 3;
            }
            None => {
                decoded.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("graphicgene-desktop-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn writes_whole_files_into_new_folders_and_over_old_ones() {
        let folder = scratch("write");
        let path = folder.join("nested").join("autosave.json");
        write_whole(&path, b"first").unwrap();
        write_whole(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        let left: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1, "no temporary file left behind");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn decodes_names_the_page_encoded() {
        assert_eq!(
            percent_decode("graphicgene-project.json"),
            "graphicgene-project.json"
        );
        assert_eq!(percent_decode("graphicgene%402x.png"), "graphicgene@2x.png");
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87.svg"), "中文.svg");
        // A stray percent sign is kept, not dropped.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn offers_the_file_type_of_the_suggested_name() {
        assert_eq!(
            filter_for("graphicgene-project.json"),
            Some(("graphicgene project", "json"))
        );
        assert_eq!(filter_for("graphicgene@3x.PNG"), Some(("PNG image", "png")));
        assert_eq!(filter_for("graphicgene.svg"), Some(("SVG image", "svg")));
        assert_eq!(filter_for("untitled"), None);
    }
}
