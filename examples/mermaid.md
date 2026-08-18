# Mermaid 图表示例

MD Reader 只在图表接近视口时加载 Mermaid，普通 Markdown 的打开速度不受影响。

## 流程图

```mermaid
flowchart LR
    A[读取 Markdown] --> B[Worker 解析]
    B --> C{内容类型}
    C -->|文字与表格| D[立即排版]
    C -->|代码块| E[进入视口后高亮]
    C -->|Mermaid| F[进入视口后绘图]
```

## 时序图

```mermaid
sequenceDiagram
    participant U as 用户
    participant R as MD Reader
    participant W as 渲染线程
    U->>R: 打开文档
    R->>W: 发送 Markdown
    W-->>R: 返回 HTML
    R-->>U: 显示排版结果
```

## 普通代码块

```typescript
export function fastRender(markdown: string): Promise<string> {
  return renderer.render(markdown);
}
```
