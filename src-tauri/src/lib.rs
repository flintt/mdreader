use base64::{engine::general_purpose::STANDARD, Engine as _};
use percent_encoding::percent_decode_str;
use serde::Serialize;
use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};
use walkdir::{DirEntry, WalkDir};

const MAX_MARKDOWN_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_DIRECTORY_FILES: usize = 5_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileDocument {
    path: String,
    name: String,
    parent: String,
    content: String,
    modified_ms: u128,
    size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownFile {
    path: String,
    name: String,
    relative_path: String,
    depth: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedMarkdownLink {
    path: String,
    anchor: Option<String>,
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mdx" | "txt"
            )
        })
        .unwrap_or(false)
}

fn is_visible_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !name.starts_with('.')
        && !matches!(
            name.as_ref(),
            "node_modules" | "target" | "dist" | "build" | "vendor"
        )
}

#[tauri::command]
fn read_markdown_file(path: String) -> Result<FileDocument, String> {
    let requested = PathBuf::from(path);
    if !is_markdown(&requested) {
        return Err("不是支持的 Markdown 文档".into());
    }
    let canonical = requested
        .canonicalize()
        .map_err(|error| format!("文件不存在或无法访问：{error}"))?;
    let metadata =
        fs::metadata(&canonical).map_err(|error| format!("无法读取文件信息：{error}"))?;
    if !metadata.is_file() {
        return Err("选择的路径不是文件".into());
    }
    if metadata.len() > MAX_MARKDOWN_BYTES {
        return Err("文件超过 32 MB，为避免占用过多内存已停止读取".into());
    }
    let bytes = fs::read(&canonical).map_err(|error| format!("读取失败：{error}"))?;
    let content =
        String::from_utf8(bytes).map_err(|_| "当前仅支持 UTF-8 编码的 Markdown".to_string())?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    Ok(FileDocument {
        path: path_string(&canonical),
        name: canonical
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled.md".into()),
        parent: path_string(canonical.parent().unwrap_or(Path::new("."))),
        content,
        modified_ms,
        size: metadata.len(),
    })
}

#[tauri::command]
fn scan_markdown_directory(path: String) -> Result<Vec<MarkdownFile>, String> {
    let root = PathBuf::from(path)
        .canonicalize()
        .map_err(|error| format!("文件夹不存在或无法访问：{error}"))?;
    if !root.is_dir() {
        return Err("选择的路径不是文件夹".into());
    }

    let mut files: Vec<MarkdownFile> = WalkDir::new(&root)
        .max_depth(10)
        .follow_links(false)
        .into_iter()
        .filter_entry(is_visible_entry)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && is_markdown(entry.path()))
        .take(MAX_DIRECTORY_FILES)
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(&root).ok()?;
            let relative_path = relative
                .components()
                .filter_map(|component| match component {
                    Component::Normal(value) => Some(value.to_string_lossy()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/");
            Some(MarkdownFile {
                path: path_string(entry.path()),
                name: entry.file_name().to_string_lossy().into_owned(),
                depth: relative.components().count().saturating_sub(1),
                relative_path,
            })
        })
        .collect();

    files.sort_unstable_by_key(|file| file.relative_path.to_ascii_lowercase());
    Ok(files)
}

#[tauri::command]
fn read_local_image(base_dir: String, source: String) -> Result<String, String> {
    let source_without_query = source.split(['?', '#']).next().unwrap_or(&source);
    let decoded = percent_decode_str(source_without_query)
        .decode_utf8()
        .map_err(|_| "图片路径编码无效".to_string())?;
    let candidate = Path::new(&base_dir).join(decoded.as_ref());
    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("找不到图片：{error}"))?;
    let metadata = fs::metadata(&canonical).map_err(|error| format!("无法读取图片：{error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
        return Err("图片无效或超过 16 MB".into());
    }
    let mime = mime_guess::from_path(&canonical)
        .first_raw()
        .filter(|value| value.starts_with("image/"))
        .ok_or_else(|| "不支持的图片格式".to_string())?;
    let bytes = fs::read(canonical).map_err(|error| format!("读取图片失败：{error}"))?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
fn resolve_markdown_link(base_dir: String, href: String) -> Result<ResolvedMarkdownLink, String> {
    let mut parts = href.splitn(2, '#');
    let raw_path = parts.next().unwrap_or_default();
    let anchor = parts.next().map(|value| value.to_string());
    let decoded = percent_decode_str(raw_path)
        .decode_utf8()
        .map_err(|_| "链接路径编码无效".to_string())?;
    let resolved = Path::new(&base_dir)
        .join(decoded.as_ref())
        .canonicalize()
        .map_err(|error| format!("找不到链接文档：{error}"))?;
    if !is_markdown(&resolved) {
        return Err("链接目标不是 Markdown 文档".into());
    }
    Ok(ResolvedMarkdownLink {
        path: path_string(&resolved),
        anchor,
    })
}

#[tauri::command]
fn startup_markdown_file() -> Option<String> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|path| path.is_file() && is_markdown(path))
        .and_then(|path| path.canonicalize().ok())
        .map(|path| path_string(&path))
}

#[tauri::command]
fn file_modified_ms(path: String) -> Result<u128, String> {
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| format!("无法读取文件状态：{error}"))?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            read_markdown_file,
            scan_markdown_directory,
            read_local_image,
            resolve_markdown_link,
            startup_markdown_file,
            file_modified_ms
        ])
        .run(tauri::generate_context!())
        .expect("error while running MD Reader");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_markdown_extensions() {
        assert!(is_markdown(Path::new("README.md")));
        assert!(is_markdown(Path::new("notes.MARKDOWN")));
        assert!(is_markdown(Path::new("draft.mdx")));
        assert!(!is_markdown(Path::new("photo.png")));
    }
}
