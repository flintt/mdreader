use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime},
};

use mermaid_rs_renderer::{RenderOptions, render_with_options};

const MAX_MARKDOWN_BYTES: u64 = 32 * 1024 * 1024;
const TARGET_PAGE_LINES: usize = 64;
const MERMAID_CACHE_VERSION: &str = "v4";

pub type MermaidCache = Arc<Mutex<HashMap<String, String>>>;

#[derive(Clone, Debug)]
pub struct Heading {
    pub id: String,
    pub title: String,
    pub level: usize,
    pub page_index: usize,
}

#[derive(Clone, Debug)]
pub struct MarkdownPage {
    pub source: String,
    pub estimated_height: f32,
}

#[derive(Clone, Debug)]
pub struct LoadedDocument {
    pub path: PathBuf,
    pub name: String,
    pub parent: PathBuf,
    pub pages: Vec<MarkdownPage>,
    pub headings: Vec<Heading>,
    pub modified: Option<SystemTime>,
    pub character_count: usize,
    pub reading_minutes: usize,
    pub mermaid_count: usize,
    pub load_ms: f64,
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mdx" | "txt"
            )
        })
}

pub fn load_document(
    path: PathBuf,
    mermaid_cache: &MermaidCache,
) -> Result<LoadedDocument, String> {
    let started = Instant::now();
    if !is_markdown(&path) {
        return Err("不是支持的 Markdown 文档".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("文件不存在或无法访问：{error}"))?;
    let metadata = fs::metadata(&canonical).map_err(|error| format!("无法读取文件：{error}"))?;
    if !metadata.is_file() {
        return Err("选择的路径不是文件".into());
    }
    if metadata.len() > MAX_MARKDOWN_BYTES {
        return Err("文件超过 32 MB，为避免占用过多内存已停止读取".into());
    }
    let source = fs::read_to_string(&canonical)
        .map_err(|error| format!("仅支持 UTF-8 Markdown，读取失败：{error}"))?;
    let (rendered, mut headings, mermaid_count) = preprocess_markdown(&source, mermaid_cache);
    let pages = split_markdown_pages(&rendered);
    let page_by_heading = pages
        .iter()
        .enumerate()
        .flat_map(|(page_index, page)| {
            page.source.lines().filter_map(move |line| {
                let (_, _, id) = parse_atx_heading(line)?;
                id.map(|id| (id, page_index))
            })
        })
        .collect::<HashMap<_, _>>();
    for heading in &mut headings {
        heading.page_index = page_by_heading.get(&heading.id).copied().unwrap_or(0);
    }
    let character_count = source.chars().count();
    let cjk_count = source
        .chars()
        .filter(|character| matches!(*character as u32, 0x3400..=0x9fff | 0xf900..=0xfaff))
        .count();
    let latin_words = source
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|word| !word.is_empty())
        .count();
    let reading_minutes = ((cjk_count as f64 / 400.0) + (latin_words as f64 / 220.0))
        .ceil()
        .max(1.0) as usize;

    Ok(LoadedDocument {
        name: canonical
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled.md")
            .to_owned(),
        parent: canonical.parent().unwrap_or(Path::new(".")).to_path_buf(),
        path: canonical,
        pages,
        headings,
        modified: metadata.modified().ok(),
        character_count,
        reading_minutes,
        mermaid_count,
        load_ms: started.elapsed().as_secs_f64() * 1_000.0,
    })
}

enum Fence {
    Regular {
        marker: char,
        length: usize,
    },
    Mermaid {
        marker: char,
        length: usize,
        source: String,
    },
}

fn preprocess_markdown(
    source: &str,
    mermaid_cache: &MermaidCache,
) -> (String, Vec<Heading>, usize) {
    let mut output = String::with_capacity(source.len());
    let mut headings = Vec::new();
    let mut fence: Option<Fence> = None;
    let mut mermaid_count = 0;

    for line in source.split_inclusive('\n') {
        match &mut fence {
            Some(Fence::Regular { marker, length }) => {
                output.push_str(line);
                if is_closing_fence(line, *marker, *length) {
                    fence = None;
                }
                continue;
            }
            Some(Fence::Mermaid {
                marker,
                length,
                source: diagram,
            }) => {
                if is_closing_fence(line, *marker, *length) {
                    mermaid_count += 1;
                    output.push_str(&render_mermaid_markdown(diagram, mermaid_cache));
                    fence = None;
                } else {
                    diagram.push_str(line);
                }
                continue;
            }
            None => {}
        }

        if let Some((marker, length, language)) = opening_fence(line) {
            if language.eq_ignore_ascii_case("mermaid") {
                fence = Some(Fence::Mermaid {
                    marker,
                    length,
                    source: String::new(),
                });
            } else {
                output.push_str(line);
                fence = Some(Fence::Regular { marker, length });
            }
            continue;
        }

        if let Some((level, title, existing_id)) = parse_atx_heading(line) {
            let id =
                existing_id.unwrap_or_else(|| format!("mdreader-heading-{}", headings.len() + 1));
            headings.push(Heading {
                id: id.clone(),
                title,
                level,
                page_index: 0,
            });
            if line.trim_end_matches(['\r', '\n']).contains("{#") {
                output.push_str(line);
            } else {
                let ending = if line.ends_with("\r\n") {
                    "\r\n"
                } else if line.ends_with('\n') {
                    "\n"
                } else {
                    ""
                };
                let body = line.trim_end_matches(['\r', '\n']);
                output.push_str(body);
                output.push_str(&format!(" {{#{id}}}{ending}"));
            }
        } else {
            output.push_str(line);
        }
    }

    if let Some(Fence::Mermaid {
        marker,
        length,
        source: diagram,
    }) = fence
    {
        output.push_str(&marker.to_string().repeat(length));
        output.push_str("mermaid\n");
        output.push_str(&diagram);
    }

    (output, headings, mermaid_count)
}

fn split_markdown_pages(source: &str) -> Vec<MarkdownPage> {
    let mut pages = Vec::new();
    let mut current = String::new();
    let mut current_lines = 0;
    let mut fence: Option<(char, usize)> = None;

    for line in source.split_inclusive('\n') {
        current.push_str(line);
        current_lines += 1;

        if let Some((marker, length)) = fence {
            if is_closing_fence(line, marker, length) {
                fence = None;
            }
        } else if let Some((marker, length, _)) = opening_fence(line) {
            fence = Some((marker, length));
        }

        if current_lines >= TARGET_PAGE_LINES && fence.is_none() && line.trim().is_empty() {
            push_markdown_page(&mut pages, std::mem::take(&mut current));
            current_lines = 0;
        }
    }

    if !current.is_empty() || pages.is_empty() {
        push_markdown_page(&mut pages, current);
    }
    pages
}

fn push_markdown_page(pages: &mut Vec<MarkdownPage>, source: String) {
    let estimated_height = estimate_page_height(&source);
    pages.push(MarkdownPage {
        source,
        estimated_height,
    });
}

fn estimate_page_height(source: &str) -> f32 {
    let units = source.lines().fold(0.0_f32, |total, line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            total + 0.35
        } else if trimmed.starts_with('#') {
            total + 1.8
        } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            total + 0.5
        } else {
            total + (line.chars().count().max(1) as f32 / 72.0).ceil()
        }
    });
    (units * 25.0 + 20.0).max(80.0)
}

fn opening_fence(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start();
    if line.len().saturating_sub(trimmed.len()) > 3 {
        return None;
    }
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if length < 3 {
        return None;
    }
    let language = trimmed[length..]
        .split_whitespace()
        .next()
        .unwrap_or_default();
    Some((marker, length, language))
}

fn is_closing_fence(line: &str, marker: char, minimum_length: usize) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= minimum_length && trimmed.chars().all(|character| character == marker)
}

fn parse_atx_heading(line: &str) -> Option<(usize, String, Option<String>)> {
    let content = line.trim_start();
    if line.len().saturating_sub(content.len()) > 3 {
        return None;
    }
    let level = content
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&level) || !content[level..].starts_with(char::is_whitespace) {
        return None;
    }
    let mut title = content[level..]
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_owned();
    let existing_id = title.rfind("{#").and_then(|start| {
        title.strip_suffix('}')?;
        let id = title[start + 2..title.len() - 1].trim().to_owned();
        title.truncate(start);
        title = title.trim().to_owned();
        (!id.is_empty()).then_some(id)
    });
    Some((level, title, existing_id))
}

fn render_mermaid_markdown(source: &str, cache: &MermaidCache) -> String {
    let key = blake3::hash(source.as_bytes()).to_hex().to_string();
    if let Some(uri) = cache.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        return mermaid_markdown(&uri);
    }

    let cache_directory =
        std::env::temp_dir().join(format!("mdreader-mermaid-{MERMAID_CACHE_VERSION}"));
    let svg_path = cache_directory.join(format!("{key}.svg"));
    if svg_path.is_file() {
        let uri = file_uri(&svg_path);
        if let Ok(mut cache) = cache.lock() {
            cache.insert(key, uri.clone());
        }
        return mermaid_markdown(&uri);
    }

    let result = std::panic::catch_unwind(|| render_with_options(source, mermaid_render_options()));
    match result {
        Ok(Ok(svg)) => {
            let svg = svg.replacen(
                "<svg ",
                "<svg shape-rendering=\"geometricPrecision\" text-rendering=\"optimizeLegibility\" ",
                1,
            );
            if let Err(error) = fs::create_dir_all(&cache_directory)
                .and_then(|()| fs::write(&svg_path, svg.as_bytes()))
            {
                return format!("\n> **Mermaid 缓存写入失败：** {error}\n\n");
            }
            let uri = file_uri(&svg_path);
            if let Ok(mut cache) = cache.lock() {
                cache.insert(key, uri.clone());
            }
            mermaid_markdown(&uri)
        }
        Ok(Err(error)) => format!(
            "\n> **Mermaid 图表无法渲染：** {}\n\n```mermaid\n{source}\n```\n\n",
            error.to_string().replace(['\r', '\n'], " ")
        ),
        Err(_) => format!("\n> **Mermaid 图表渲染异常**\n\n```mermaid\n{source}\n```\n\n"),
    }
}

fn mermaid_render_options() -> RenderOptions {
    let mut options = RenderOptions::modern()
        .with_node_spacing(60.0)
        .with_rank_spacing(64.0);

    options.theme.font_size = 16.0;
    options.theme.primary_border_color = "#64748B".into();
    options.theme.line_color = "#334155".into();
    options.theme.cluster_border = "#94A3B8".into();
    options.theme.sequence_actor_border = "#64748B".into();
    options.theme.sequence_actor_line = "#475569".into();

    options.layout.node_padding_x = 34.0;
    options.layout.node_padding_y = 18.0;
    options.layout.label_line_height = 1.4;
    options.layout.max_label_width_chars = 24;

    let flowchart = &mut options.layout.flowchart;
    flowchart.auto_spacing.min_spacing = 36.0;
    flowchart.auto_spacing.dense_scale_floor = 0.85;
    for bucket in &mut flowchart.auto_spacing.buckets {
        bucket.scale = bucket.scale.max(0.85);
    }
    flowchart.objective.max_aspect_ratio = 6.0;
    flowchart.routing.occupancy_weight = 1.4;

    options
}

fn mermaid_markdown(uri: &str) -> String {
    format!("\n<div data-mdreader-mermaid=\"{uri}\"></div>\n\n")
}

fn file_uri(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn adds_stable_heading_ids_without_touching_code_fences() {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let source = "# 标题\n\n```rust\n# not a heading\n```\n\n## Second\n";
        let (rendered, headings, _) = preprocess_markdown(source, &cache);
        assert_eq!(headings.len(), 2);
        assert!(rendered.contains("# 标题 {#mdreader-heading-1}"));
        assert!(rendered.contains("# not a heading"));
    }

    #[test]
    fn renders_mermaid_without_a_browser() {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let source = "```mermaid\nflowchart LR\nA --> B\n```\n";
        let (rendered, _, count) = preprocess_markdown(source, &cache);
        assert_eq!(count, 1);
        assert!(rendered.contains("file://"));
        assert!(rendered.contains(".svg"));
        assert!(rendered.contains("data-mdreader-mermaid"));
    }

    #[test]
    fn keeps_dense_mermaid_layout_readable() {
        let options = mermaid_render_options();
        assert_eq!(options.theme.font_size, 16.0);
        assert!(options.layout.node_spacing >= 60.0);
        assert!(options.layout.rank_spacing >= 64.0);
        assert!(options.layout.flowchart.auto_spacing.min_spacing >= 36.0);
        assert!(
            options
                .layout
                .flowchart
                .auto_spacing
                .buckets
                .iter()
                .all(|bucket| bucket.scale >= 0.85)
        );
    }

    #[test]
    fn preprocesses_a_megabyte_without_blocking_budget_regression() {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let section = "## Long document section\n\nText, table data, and `inline code`.\n\n";
        let repeats = 1_048_576 / section.len() + 1;
        let source = section.repeat(repeats);
        let started = Instant::now();
        let (rendered, headings, count) = preprocess_markdown(&source, &cache);
        let pages = split_markdown_pages(&rendered);

        assert_eq!(count, 0);
        assert_eq!(headings.len(), repeats);
        assert!(rendered.len() > source.len());
        assert!(pages.len() > 100);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "1 MB Markdown preprocessing exceeded the worker budget"
        );
    }
}
