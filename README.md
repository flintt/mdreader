# MD Reader

一款为 Windows 设计的快速、纯原生 Markdown 阅读器。界面使用 Rust + egui，文本由 GPU 直接绘制；运行时不包含 WebView、Chromium、HTML、JavaScript 或 Node.js。

本项目是独立开源实现，与 Typora 及其开发者不存在关联，也不包含 Typora 的源码或品牌素材。

## 原生架构

- eframe/egui 原生窗口与 wgpu GPU 渲染
- Markdown 仅绘制当前可见区域，长文可直接使用原生滚轮和滚动条
- 文件读取、目录扫描和 Mermaid 排版均在后台线程执行，窗口不会等待 I/O
- 目录采用稳定路径 ID，切换文章不会重建或丢失展开状态
- Mermaid 由纯 Rust 直接生成 SVG，并按内容哈希缓存；无需浏览器或外部进程
- 本地图片、表格、任务列表、代码块、大纲和自动刷新
- 拖放打开、最近文档、文件筛选、深浅主题、字号和版心设置
- Windows `.md` / `.markdown` 文件关联

## 运行与测试

需要 Rust 1.95+。Windows 构建还需要标准的 MSVC Rust 工具链。

```bash
cargo run --release -- README.md
cargo test --locked
```

## 构建 Windows 安装包

在 Windows x64 环境执行：

```powershell
cargo install cargo-packager --version 0.11.8 --locked
cargo packager --release
```

NSIS、MSI 安装包会生成到 `dist/`。仓库的 `Build native Windows` 工作流也会在推送 `v*` 标签时测试、构建并发布安装包和便携版 EXE。

## 快捷键

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl+O` | 打开 Markdown |
| `Ctrl+Shift+O` | 打开文件夹 |
| `Ctrl+B` | 显示/隐藏侧边栏 |
| `Ctrl+K` | 聚焦文件筛选 |

## Mermaid

使用标准 fenced code block：

````markdown
```mermaid
flowchart LR
    A[打开文档] --> B[原生排版] --> C[GPU 显示]
```
````

当前纯 Rust 渲染器支持流程图、时序图、类图、状态图、ER 图、饼图、甘特图、时间线、用户旅程、思维导图、Git Graph 等常见类型。它并非调用 Mermaid.js，因此极少数 Mermaid.js 扩展语法可能存在差异。完整示例见 [`examples/mermaid.md`](examples/mermaid.md)。

## 性能设计

MD Reader 定位为阅读器，不维护富文本编辑状态和撤销栈。打开文件和目录后立即返回 UI，Markdown 预处理在工作线程完成；Mermaid SVG 使用内存与磁盘两级缓存；正文滚动只布局和绘制视口附近内容。底部状态栏会显示当前文档的后台加载耗时，便于在实际机器上验证。

## License

MIT
