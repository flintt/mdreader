# Changelog

## 0.2.2 — 2026-08-19

- Windows 启动不再递归扫描系统及用户字体目录，只读取固定的系统中文字体候选文件
- Release 构建恢复 panic unwind，使 Mermaid 渲染异常能够被隔离而不是终止整个进程
- 图形窗口初始化失败或主线程异常时显示原生错误对话框
- 启动错误写入 `%LOCALAPPDATA%\\MD Reader\\startup-error.log`，避免无窗口、无提示退出

## 0.2.1 — 2026-08-18

- 重做阅读区宽度计算：自动模式保持 5% 边距，宽屏限制为 1040px 并精确居中
- 固定滚动条占位和虚拟分页左边线，消除窗口缩放与滚动时的横向跳动
- 加入自动/手动页面宽度切换，并兼容已有设置
- Mermaid 默认以 1:1 矢量原图显示，复杂图使用独立横向滚动条，不再压缩成不可读小图
- Mermaid 加入“清晰原图 / 适应页面”即时切换
- 提高 Mermaid 字号、线条对比度和密集节点最低间距，同时保留快速布局策略

## 0.2.0 — 2026-08-18

- 完全移除 Tauri、WebView、HTML、JavaScript 和 Node.js 运行链路
- 使用 Rust + egui + wgpu 重写为 GPU 绘制的原生桌面应用
- 使用原生滚动视口，仅布局和绘制视口附近的 Markdown 内容
- 文件读取、目录扫描和 Mermaid 排版移至后台线程
- 使用稳定路径 ID 的递归文件树，切换文档后保持展开状态
- 加入纯 Rust Mermaid SVG 渲染及内存/磁盘缓存
- 修复 Mermaid SVG 中文与英文文字显示
- 加入原生 NSIS、MSI 与便携版 EXE 的 Windows 发布流程
- Windows 版静态链接 MSVC CRT，便携版无需额外运行库

## 0.1.0 — 2026-08-18

- 首个公开版本
- Tauri 2 + Rust 文件读取与目录扫描
- Web Worker Markdown 解析和渲染缓存
- GFM、代码按需高亮、本地图片和相对文档链接
- Mermaid 图表按视口渲染，支持深浅主题
- 文件树、文档大纲、最近文档和阅读设置
- Windows 文件关联以及 NSIS / MSI 构建配置
