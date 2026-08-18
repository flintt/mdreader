# MD Reader

一款为 Windows 设计的轻量 Markdown 阅读器。交互参考 Typora 的克制阅读体验，核心目标是启动快、打开快、渲染时界面不卡顿。

本项目是独立开源实现，与 Typora 及其开发者不存在关联，也不包含 Typora 的源码或品牌素材。

## 已实现

- Tauri 2 + Rust 原生文件读取，小体积、低内存
- Web Worker 后台解析 Markdown，长文档渲染不阻塞窗口
- 代码块接近视口时才动态加载并高亮，超长文档无需预处理全部代码
- GFM、表格、任务列表、代码高亮和本地图片
- Mermaid 流程图、时序图等图表按视口动态加载，并自动适配深浅主题
- 拖放打开、文件夹扫描、文件筛选和文档大纲
- 浅色/深色/跟随系统、字号和版心设置
- 最近文档、相对 Markdown 链接、Windows 文件关联
- HTML 清理与本地图片类型/大小限制

## 本地开发

需要 Node.js 20+、Rust 1.77+，以及 Windows 上的 WebView2 和 Microsoft C++ Build Tools。

```bash
npm install
npm run tauri dev
```

仅预览界面（不能读取本地文件）：

```bash
npm run dev
```

## 构建 Windows 安装包

请在 Windows x64 环境运行：

```powershell
npm install
npm run tauri build
```

NSIS 和 MSI 安装包会生成到 `src-tauri/target/release/bundle/`。

仓库还包含 `Build Windows` GitHub Actions 工作流：手动触发，或推送 `v*` 标签后，可直接下载构建好的 x64 NSIS / MSI artifact。

## 快捷键

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl+O` | 打开 Markdown |
| `Ctrl+Shift+O` | 打开文件夹 |
| `Ctrl+B` | 显示/隐藏侧边栏 |
| `Ctrl+K` | 聚焦文件筛选 |

## Mermaid

使用标准 fenced code block 即可显示图表：

````markdown
```mermaid
flowchart LR
    A[打开文档] --> B[快速渲染]
```
````

Mermaid 采用严格安全模式，并在图表接近视口时才动态加载。完整示例见 [`examples/mermaid.md`](examples/mermaid.md)。

## 性能取舍

第一版定位为阅读器而不是编辑器：没有引入富文本编辑状态、撤销栈和双向 Markdown AST，因此冷启动和首次排版链路更短。解析结果按文件修改时间缓存，重复切换文档无需再次解析。

运行 `npm run bench` 可在本机复测 100 KB / 1 MB 合成文档的纯解析耗时，以及首屏 20 个代码块的高亮耗时。

## License

MIT
