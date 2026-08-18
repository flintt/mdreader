use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use walkdir::{DirEntry, WalkDir};

use crate::document::is_markdown;

const MAX_DIRECTORY_FILES: usize = 5_000;

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub relative_path: String,
}

#[derive(Clone, Debug, Default)]
pub struct DirectoryNode {
    pub name: String,
    pub path: PathBuf,
    pub directories: BTreeMap<String, DirectoryNode>,
    pub files: Vec<FileEntry>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceTree {
    pub root_node: DirectoryNode,
    pub file_count: usize,
}

pub fn scan_workspace(path: PathBuf) -> Result<WorkspaceTree, String> {
    let root = path
        .canonicalize()
        .map_err(|error| format!("文件夹不存在或无法访问：{error}"))?;
    if !root.is_dir() {
        return Err("选择的路径不是文件夹".into());
    }

    let mut files = WalkDir::new(&root)
        .max_depth(12)
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
            Some(FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path().to_path_buf(),
                relative_path,
            })
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|file| file.relative_path.to_ascii_lowercase());

    let root_name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Workspace")
        .to_owned();
    let mut root_node = DirectoryNode {
        name: root_name,
        path: root.clone(),
        ..Default::default()
    };
    for file in &files {
        insert_file(&mut root_node, &root, file.clone());
    }

    Ok(WorkspaceTree {
        root_node,
        file_count: files.len(),
    })
}

fn insert_file(root_node: &mut DirectoryNode, root: &Path, file: FileEntry) {
    let mut directory = root_node;
    let segments = file.relative_path.split('/').collect::<Vec<_>>();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        let child_path = directory.path.join(segment);
        directory = directory
            .directories
            .entry(segment.to_string())
            .or_insert_with(|| DirectoryNode {
                name: segment.to_string(),
                path: child_path,
                ..Default::default()
            });
    }
    directory.files.push(file);
    directory
        .files
        .sort_by_key(|entry| entry.name.to_ascii_lowercase());
    debug_assert!(directory.path.starts_with(root));
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

impl DirectoryNode {
    pub fn contains_query(&self, query: &str) -> bool {
        query.is_empty()
            || self.name.to_ascii_lowercase().contains(query)
            || self
                .files
                .iter()
                .any(|file| file.relative_path.to_ascii_lowercase().contains(query))
            || self
                .directories
                .values()
                .any(|directory| directory.contains_query(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_keeps_matching_parent_directories_visible() {
        let mut root = DirectoryNode::default();
        root.directories.insert(
            "guides".into(),
            DirectoryNode {
                name: "guides".into(),
                files: vec![FileEntry {
                    name: "fast.md".into(),
                    path: PathBuf::from("guides/fast.md"),
                    relative_path: "guides/fast.md".into(),
                }],
                ..Default::default()
            },
        );
        assert!(root.contains_query("fast"));
        assert!(!root.contains_query("missing"));
    }
}
