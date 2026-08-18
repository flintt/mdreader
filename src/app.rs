use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Id, Layout,
    RichText, TextStyle, ThemePreference,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use fontdb::{Database, Family, Query};
use serde::{Deserialize, Serialize};

use crate::{
    document::{LoadedDocument, MermaidCache, is_markdown, load_document},
    tree::{DirectoryNode, FileEntry, WorkspaceTree, scan_workspace},
};

const MAX_RECENTS: usize = 10;
const MARKDOWN_FILTERS: &[&str] = &["md", "markdown", "mdown", "mkd", "mdx", "txt"];

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RecentDocument {
    path: PathBuf,
    name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Preferences {
    theme: ThemePreference,
    font_size: f32,
    content_width: f32,
    sidebar_open: bool,
    recents: Vec<RecentDocument>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            font_size: 17.0,
            content_width: 820.0,
            sidebar_open: true,
            recents: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarTab {
    Files,
    Outline,
}

enum WorkerMessage {
    Document {
        sequence: u64,
        result: Result<LoadedDocument, String>,
    },
    Workspace {
        sequence: u64,
        result: Result<WorkspaceTree, String>,
    },
}

pub struct NativeApp {
    preferences: Preferences,
    document: Option<LoadedDocument>,
    workspace: Option<WorkspaceTree>,
    markdown_cache: CommonMarkCache,
    page_heights: Vec<f32>,
    pending_page_scroll: Option<usize>,
    last_reader_width: f32,
    mermaid_cache: MermaidCache,
    sender: mpsc::Sender<WorkerMessage>,
    receiver: mpsc::Receiver<WorkerMessage>,
    document_sequence: u64,
    workspace_sequence: u64,
    loading_path: Option<PathBuf>,
    loading_workspace: bool,
    sidebar_tab: SidebarTab,
    file_filter: String,
    settings_open: bool,
    status_message: String,
    toast: Option<(String, Instant)>,
    last_modified_check: Instant,
}

impl NativeApp {
    pub fn new(context: &eframe::CreationContext<'_>) -> Self {
        install_system_font(&context.egui_ctx);
        let preferences: Preferences = context
            .storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default();
        context.egui_ctx.set_theme(preferences.theme);
        apply_typography(&context.egui_ctx, preferences.font_size);
        let (sender, receiver) = mpsc::channel();
        let mut app = Self {
            preferences,
            document: None,
            workspace: None,
            markdown_cache: CommonMarkCache::default(),
            page_heights: Vec::new(),
            pending_page_scroll: None,
            last_reader_width: 0.0,
            mermaid_cache: Arc::new(Mutex::new(HashMap::new())),
            sender,
            receiver,
            document_sequence: 0,
            workspace_sequence: 0,
            loading_path: None,
            loading_workspace: false,
            sidebar_tab: SidebarTab::Files,
            file_filter: String::new(),
            settings_open: false,
            status_message: "就绪".into(),
            toast: None,
            last_modified_check: Instant::now(),
        };

        if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) {
            if path.is_dir() {
                app.request_workspace(path, &context.egui_ctx);
            } else {
                app.request_document(path, &context.egui_ctx);
            }
        }
        app
    }

    fn request_document(&mut self, path: PathBuf, context: &egui::Context) {
        if self.loading_path.as_ref() == Some(&path) {
            return;
        }
        self.document_sequence += 1;
        let sequence = self.document_sequence;
        self.loading_path = Some(path.clone());
        self.status_message = "读取中…".into();
        let sender = self.sender.clone();
        let repaint = context.clone();
        let mermaid_cache = Arc::clone(&self.mermaid_cache);
        thread::spawn(move || {
            let result = load_document(path, &mermaid_cache);
            let _ = sender.send(WorkerMessage::Document { sequence, result });
            repaint.request_repaint();
        });
    }

    fn request_workspace(&mut self, path: PathBuf, context: &egui::Context) {
        self.workspace_sequence += 1;
        let sequence = self.workspace_sequence;
        self.loading_workspace = true;
        self.status_message = "扫描文件夹…".into();
        let sender = self.sender.clone();
        let repaint = context.clone();
        thread::spawn(move || {
            let result = scan_workspace(path);
            let _ = sender.send(WorkerMessage::Workspace { sequence, result });
            repaint.request_repaint();
        });
    }

    fn process_worker_messages(&mut self, context: &egui::Context) {
        let messages = self.receiver.try_iter().collect::<Vec<_>>();
        for message in messages {
            match message {
                WorkerMessage::Document { sequence, result }
                    if sequence == self.document_sequence =>
                {
                    self.loading_path = None;
                    match result {
                        Ok(document) => {
                            let _ = std::env::set_current_dir(&document.parent);
                            self.status_message = format!("原生渲染 {:.1} ms", document.load_ms);
                            self.remember_recent(&document);
                            context.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                                "{} — MD Reader",
                                strip_markdown_extension(&document.name)
                            )));
                            self.page_heights = document
                                .pages
                                .iter()
                                .map(|page| {
                                    initial_page_height(
                                        page.estimated_height,
                                        self.preferences.font_size,
                                        self.preferences.content_width,
                                    )
                                })
                                .collect();
                            self.pending_page_scroll = None;
                            self.last_reader_width = 0.0;
                            self.markdown_cache = CommonMarkCache::default();
                            self.document = Some(document);
                        }
                        Err(error) => self.show_error(error),
                    }
                }
                WorkerMessage::Workspace { sequence, result }
                    if sequence == self.workspace_sequence =>
                {
                    self.loading_workspace = false;
                    match result {
                        Ok(workspace) => {
                            let first =
                                first_file(&workspace.root_node).map(|file| file.path.clone());
                            self.status_message = format!("{} 个文档", workspace.file_count);
                            self.workspace = Some(workspace);
                            self.sidebar_tab = SidebarTab::Files;
                            self.preferences.sidebar_open = true;
                            if let Some(path) = first {
                                self.request_document(path, context);
                            }
                        }
                        Err(error) => self.show_error(error),
                    }
                }
                _ => {}
            }
        }
    }

    fn remember_recent(&mut self, document: &LoadedDocument) {
        self.preferences
            .recents
            .retain(|recent| recent.path != document.path);
        self.preferences.recents.insert(
            0,
            RecentDocument {
                path: document.path.clone(),
                name: document.name.clone(),
            },
        );
        self.preferences.recents.truncate(MAX_RECENTS);
    }

    fn show_error(&mut self, message: String) {
        self.status_message = "操作失败".into();
        self.toast = Some((message, Instant::now() + Duration::from_secs(4)));
    }

    fn open_file_dialog(&mut self, context: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", MARKDOWN_FILTERS)
            .set_title("打开 Markdown")
            .pick_file()
        {
            self.request_document(path, context);
        }
    }

    fn open_folder_dialog(&mut self, context: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("打开 Markdown 文件夹")
            .pick_folder()
        {
            self.request_workspace(path, context);
        }
    }

    fn handle_shortcuts_and_drop(&mut self, context: &egui::Context) {
        let (open_file, open_folder, toggle_sidebar, focus_search, dropped) =
            context.input(|input| {
                (
                    input.key_pressed(egui::Key::O)
                        && input.modifiers.command
                        && !input.modifiers.shift,
                    input.key_pressed(egui::Key::O)
                        && input.modifiers.command
                        && input.modifiers.shift,
                    input.key_pressed(egui::Key::B) && input.modifiers.command,
                    input.key_pressed(egui::Key::K) && input.modifiers.command,
                    input.raw.dropped_files.clone(),
                )
            });
        if open_file {
            self.open_file_dialog(context);
        }
        if open_folder {
            self.open_folder_dialog(context);
        }
        if toggle_sidebar {
            self.preferences.sidebar_open = !self.preferences.sidebar_open;
        }
        if focus_search {
            self.preferences.sidebar_open = true;
            self.sidebar_tab = SidebarTab::Files;
            context.memory_mut(|memory| memory.request_focus(Id::new("file-filter")));
        }
        if let Some(path) = dropped
            .into_iter()
            .map(|file| file.path().to_path_buf())
            .next()
        {
            if path.is_dir() {
                self.request_workspace(path, context);
            } else if is_markdown(&path) {
                self.request_document(path, context);
            } else {
                self.show_error("请拖入 Markdown 文件或文件夹".into());
            }
        }
    }

    fn show_toolbar(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        egui::Panel::top("native-toolbar")
            .exact_size(50.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button("☰")
                        .on_hover_text("显示/隐藏侧栏  Ctrl+B")
                        .clicked()
                    {
                        self.preferences.sidebar_open = !self.preferences.sidebar_open;
                    }
                    ui.separator();
                    let title = self
                        .document
                        .as_ref()
                        .map(|document| document.name.as_str())
                        .unwrap_or("MD Reader");
                    ui.label(RichText::new(title).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("设置").clicked() {
                            self.settings_open = !self.settings_open;
                        }
                        if ui.button("主题").clicked() {
                            self.preferences.theme = if context.theme() == egui::Theme::Dark {
                                ThemePreference::Light
                            } else {
                                ThemePreference::Dark
                            };
                            context.set_theme(self.preferences.theme);
                        }
                        if ui.button("打开文件").on_hover_text("Ctrl+O").clicked() {
                            self.open_file_dialog(&context);
                        }
                        if ui
                            .button("打开文件夹")
                            .on_hover_text("Ctrl+Shift+O")
                            .clicked()
                        {
                            self.open_folder_dialog(&context);
                        }
                        if self.loading_path.is_some() || self.loading_workspace {
                            ui.spinner();
                        }
                    });
                });
            });
    }

    fn show_sidebar(&mut self, root: &mut egui::Ui) {
        if !self.preferences.sidebar_open {
            return;
        }
        let context = root.ctx().clone();
        let active_path = self
            .loading_path
            .clone()
            .or_else(|| self.document.as_ref().map(|document| document.path.clone()));
        let mut selected_file = None;
        let mut selected_heading = None;
        egui::Panel::left("native-sidebar")
            .default_size(270.0)
            .size_range(190.0..=440.0)
            .resizable(true)
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.sidebar_tab, SidebarTab::Files, "文件");
                    ui.selectable_value(&mut self.sidebar_tab, SidebarTab::Outline, "大纲");
                });
                ui.separator();
                match self.sidebar_tab {
                    SidebarTab::Files => {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.file_filter)
                                .id(Id::new("file-filter"))
                                .hint_text("筛选文件  Ctrl+K"),
                        );
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let query = self.file_filter.trim().to_ascii_lowercase();
                                if let Some(workspace) = &self.workspace {
                                    ui.label(
                                        RichText::new(&workspace.root_node.name).small().strong(),
                                    );
                                    ui.add_space(4.0);
                                    show_directory_children(
                                        ui,
                                        &workspace.root_node,
                                        &query,
                                        active_path.as_deref(),
                                        0,
                                        &mut selected_file,
                                    );
                                } else if self.preferences.recents.is_empty() {
                                    ui.weak("打开文件夹后在这里浏览文档");
                                } else {
                                    ui.label(RichText::new("最近打开").small().strong());
                                    for recent in &self.preferences.recents {
                                        let selected =
                                            active_path.as_deref() == Some(recent.path.as_path());
                                        if ui
                                            .selectable_label(
                                                selected,
                                                strip_markdown_extension(&recent.name),
                                            )
                                            .clicked()
                                        {
                                            selected_file = Some(recent.path.clone());
                                        }
                                    }
                                }
                            });
                    }
                    SidebarTab::Outline => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if let Some(document) = &self.document {
                                    if document.headings.is_empty() {
                                        ui.weak("当前文档没有标题");
                                    }
                                    for heading in &document.headings {
                                        ui.horizontal(|ui| {
                                            ui.add_space(
                                                ((heading.level.saturating_sub(1)) * 12) as f32,
                                            );
                                            if ui
                                                .add(egui::Button::new(&heading.title).frame(false))
                                                .clicked()
                                            {
                                                selected_heading =
                                                    Some((heading.page_index, heading.id.clone()));
                                            }
                                        });
                                    }
                                } else {
                                    ui.weak("打开文档后显示大纲");
                                }
                            });
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    if ui.button("＋ 打开文件夹").clicked() {
                        self.open_folder_dialog(&context);
                    }
                });
            });

        if let Some(path) = selected_file {
            self.request_document(path, &context);
        }
        if let Some((page_index, id)) = selected_heading {
            self.pending_page_scroll = Some(page_index);
            *self.markdown_cache.scroll_to_id_target_mut() = Some(id);
            context.request_repaint();
        }
    }

    fn show_status_bar(&mut self, root: &mut egui::Ui) {
        egui::Panel::bottom("native-status")
            .exact_size(27.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.weak(&self.status_message);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(document) = &self.document {
                            ui.weak(format!(
                                "{} 字符 · 约 {} 分钟",
                                document.character_count, document.reading_minutes
                            ));
                            if document.mermaid_count > 0 {
                                ui.weak(format!("{} 个原生图表", document.mermaid_count));
                            }
                        }
                    });
                });
            });
    }

    fn show_reader(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new().fill(if context.theme() == egui::Theme::Dark {
                    Color32::from_rgb(31, 32, 30)
                } else {
                    Color32::from_rgb(250, 249, 246)
                }),
            )
            .show(root, |ui| {
                if let Some(document) = &self.document {
                    ui.style_mut().url_in_tooltip = true;
                    let available_width = ui.available_width();
                    let content_width = self
                        .preferences
                        .content_width
                        .min((available_width - 32.0).max(280.0));
                    if (self.last_reader_width - content_width).abs() > 1.0
                        || self.page_heights.len() != document.pages.len()
                    {
                        self.page_heights = document
                            .pages
                            .iter()
                            .map(|page| {
                                initial_page_height(
                                    page.estimated_height,
                                    self.preferences.font_size,
                                    content_width,
                                )
                            })
                            .collect();
                        self.last_reader_width = content_width;
                    }

                    let requested_offset = self.pending_page_scroll.take().map(|page_index| {
                        self.page_heights
                            .iter()
                            .take(page_index)
                            .copied()
                            .sum::<f32>()
                    });
                    let mut scroll = egui::ScrollArea::vertical()
                        .id_salt(("native-reader", &document.path))
                        .auto_shrink([false, false]);
                    if let Some(offset) = requested_offset {
                        scroll = scroll.vertical_scroll_offset(offset);
                    }

                    let pages = &document.pages;
                    let page_heights = &mut self.page_heights;
                    let markdown_cache = &mut self.markdown_cache;
                    let horizontal_margin = ((available_width - content_width) / 2.0).max(12.0);
                    scroll.show_viewport(ui, |ui, viewport| {
                        let preload = viewport.height() * 0.75;
                        let visible_min = (viewport.min.y - preload).max(0.0);
                        let visible_max = viewport.max.y + preload;
                        let mut page_top = 0.0;

                        for (index, page) in pages.iter().enumerate() {
                            let expected_height = page_heights[index];
                            let page_bottom = page_top + expected_height;
                            if page_bottom < visible_min || page_top > visible_max {
                                ui.allocate_space(egui::vec2(available_width, expected_height));
                            } else {
                                let response = ui.horizontal_top(|ui| {
                                    ui.add_space(horizontal_margin);
                                    ui.vertical(|ui| {
                                        ui.set_width(content_width);
                                        CommonMarkViewer::new()
                                            .default_width(Some(content_width as usize))
                                            .max_image_width(Some(content_width as usize))
                                            .enable_scroll_to_heading(true)
                                            .show(ui, markdown_cache, &page.source);
                                    });
                                });
                                let measured = response.response.rect.height().max(24.0);
                                page_heights[index] = measured;
                            }
                            page_top += page_heights[index];
                        }
                    });
                } else {
                    ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
                        ui.add_space((ui.available_height() * 0.22).max(40.0));
                        ui.heading("MD Reader");
                        ui.label("Rust 原生窗口 · GPU 文本绘制 · 无 WebView");
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            ui.add_space((ui.available_width() - 270.0).max(0.0) / 2.0);
                            if ui.button("打开 Markdown").clicked() {
                                self.open_file_dialog(&context);
                            }
                            if ui.button("打开文件夹").clicked() {
                                self.open_folder_dialog(&context);
                            }
                        });
                    });
                }
            });
    }

    fn show_settings(&mut self, context: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let previous_font_size = self.preferences.font_size;
        let previous_content_width = self.preferences.content_width;
        egui::Window::new("阅读设置")
            .open(&mut self.settings_open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::RIGHT_TOP, [-14.0, 58.0])
            .show(context, |ui| {
                ui.label("主题");
                self.preferences.theme.radio_buttons(ui);
                ui.add_space(8.0);
                ui.add(
                    egui::Slider::new(&mut self.preferences.font_size, 14.0..=24.0)
                        .text("正文字号"),
                );
                ui.add(
                    egui::Slider::new(&mut self.preferences.content_width, 580.0..=1200.0)
                        .text("页面宽度"),
                );
            });
        context.set_theme(self.preferences.theme);
        if (previous_font_size - self.preferences.font_size).abs() > f32::EPSILON {
            apply_typography(context, self.preferences.font_size);
            self.last_reader_width = 0.0;
        }
        if (previous_content_width - self.preferences.content_width).abs() > f32::EPSILON {
            self.last_reader_width = 0.0;
        }
    }

    fn show_toast(&mut self, context: &egui::Context) {
        let Some((message, deadline)) = &self.toast else {
            return;
        };
        if Instant::now() >= *deadline {
            self.toast = None;
            return;
        }
        egui::Area::new(Id::new("toast"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -42.0])
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(message));
            });
        context.request_repaint_after(Duration::from_millis(100));
    }

    fn poll_document_changes(&mut self, context: &egui::Context) {
        if self.last_modified_check.elapsed() < Duration::from_secs(1) {
            context.request_repaint_after(Duration::from_millis(250));
            return;
        }
        self.last_modified_check = Instant::now();
        let Some(document) = &self.document else {
            return;
        };
        if self.loading_path.is_some() {
            return;
        }
        let changed = std::fs::metadata(&document.path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .is_some_and(|modified| document.modified.is_none_or(|previous| modified > previous));
        if changed {
            self.request_document(document.path.clone(), context);
        }
    }
}

impl eframe::App for NativeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.process_worker_messages(&context);
        self.handle_shortcuts_and_drop(&context);
        self.show_toolbar(ui);
        self.show_status_bar(ui);
        self.show_sidebar(ui);
        self.show_reader(ui);
        self.show_settings(&context);
        self.show_toast(&context);
        self.poll_document_changes(&context);
        if self.loading_path.is_some() || self.loading_workspace {
            context.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.preferences);
    }
}

fn show_directory_children(
    ui: &mut egui::Ui,
    directory: &DirectoryNode,
    query: &str,
    active_path: Option<&Path>,
    depth: usize,
    selected_file: &mut Option<PathBuf>,
) {
    for child in directory.directories.values() {
        if !child.contains_query(query) {
            continue;
        }
        egui::CollapsingHeader::new(&child.name)
            .id_salt(("directory", &child.path))
            .default_open(
                depth == 0 || active_path.is_some_and(|path| path.starts_with(&child.path)),
            )
            .open((!query.is_empty()).then_some(true))
            .show(ui, |ui| {
                show_directory_children(ui, child, query, active_path, depth + 1, selected_file);
            });
    }
    for file in &directory.files {
        if !query.is_empty() && !file.relative_path.to_ascii_lowercase().contains(query) {
            continue;
        }
        let selected = active_path == Some(file.path.as_path());
        let label = strip_markdown_extension(&file.name);
        let response = ui
            .selectable_label(selected, label)
            .on_hover_text(file.path.display().to_string());
        if response.clicked() {
            *selected_file = Some(file.path.clone());
        }
    }
}

fn first_file(directory: &DirectoryNode) -> Option<&FileEntry> {
    directory
        .files
        .first()
        .or_else(|| directory.directories.values().find_map(first_file))
}

fn strip_markdown_extension(name: &str) -> &str {
    name.rsplit_once('.')
        .filter(|(_, extension)| {
            MARKDOWN_FILTERS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
        .map(|(stem, _)| stem)
        .unwrap_or(name)
}

fn apply_typography(context: &egui::Context, body_size: f32) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut style = (*context.style_of(theme)).clone();
        style.text_styles.insert(
            TextStyle::Body,
            FontId::new(body_size, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new((body_size - 3.0).max(11.0), FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(body_size + 10.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new((body_size - 2.0).max(12.0), FontFamily::Monospace),
        );
        style.spacing.item_spacing.y = 7.0;
        context.set_style_of(theme, style);
    }
}

fn initial_page_height(estimate: f32, font_size: f32, content_width: f32) -> f32 {
    let font_scale = font_size / 17.0;
    let width_scale = (820.0 / content_width.max(280.0)).clamp(0.75, 2.5);
    estimate * font_scale * width_scale
}

fn install_system_font(context: &egui::Context) {
    let mut database = Database::new();
    database.load_system_fonts();
    let families = [
        Family::Name("Microsoft YaHei UI"),
        Family::Name("Microsoft YaHei"),
        Family::Name("Noto Sans CJK SC"),
        Family::Name("Source Han Sans SC"),
        Family::SansSerif,
    ];
    let Some(id) = database.query(&Query {
        families: &families,
        ..Query::default()
    }) else {
        return;
    };
    let Some((bytes, index)) = database.with_face_data(id, |data, index| (data.to_vec(), index))
    else {
        return;
    };
    let mut definitions = FontDefinitions::default();
    definitions.font_data.insert(
        "mdreader-system".into(),
        Arc::new(FontData {
            font: Cow::Owned(bytes),
            index,
            tweak: Default::default(),
        }),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push("mdreader-system".into());
    }
    context.set_fonts(definitions);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_virtual_page_renders_with_the_native_viewer() {
        let markdown = "# Heading\n\nA paragraph with **bold text** and `code`.\n\n".repeat(64);
        let mut cache = CommonMarkCache::default();

        egui::__run_test_ui(|ui| {
            CommonMarkViewer::new().show(ui, &mut cache, &markdown);
        });
    }

    #[test]
    fn strips_supported_extensions_case_insensitively() {
        assert_eq!(strip_markdown_extension("README.MD"), "README");
        assert_eq!(strip_markdown_extension("notes.markdown"), "notes");
        assert_eq!(strip_markdown_extension("archive.zip"), "archive.zip");
    }
}
