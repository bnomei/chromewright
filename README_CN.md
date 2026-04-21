# browser-use

一个轻量级的 Rust 浏览器自动化库，基于 Chrome DevTools Protocol (CDP) 实现。

## ✨ 特性亮点

- **无需 Node.js 依赖** - 纯 Rust 实现，通过 CDP 直接控制浏览器
- **轻量快速** - 无需沉重的运行时，开销极小
- **MCP 集成** - 内置 Model Context Protocol 服务器，支持 AI 驱动的自动化
- **简洁 API** - 提供面向文档交互的高级工具集

## 安装

在你的 `Cargo.toml` 中添加：

```toml
[dependencies]
browser-use = "0.1.0"
```

## 快速开始

```rust
use browser_use::browser::BrowserSession;

// 启动浏览器并导航
let session = BrowserSession::launch(Default::default())?;
session.navigate("https://example.com")?;

// 提取 DOM，并读取当前文档修订版本
let dom = session.extract_dom()?;
println!("当前 revision: {}", dom.document.revision);
```

## MCP 服务器

运行内置的 MCP 服务器，实现 AI 驱动的自动化：

```bash
# 无头模式
cargo run --bin mcp-server

# 可视化浏览器
cargo run --bin mcp-server -- --headed
```

在 macOS 上，当前更推荐先手动启动一个可视的、专用 profile 的 Chrome 实例，再让本仓库通过 CDP 连接它：

```bash
open -na "Google Chrome" --args \
  --remote-debugging-port=9222 \
  --user-data-dir="$HOME/.browser-use-agent-profile"
```

这样可以避免附着到你日常使用的个人 Chrome profile。Chrome 启动后，再把 MCP 服务器指向 `9222` 端口暴露出来的 DevTools WebSocket。

## 功能

- 默认提供面向 agent 的高级文档工具：`snapshot`、`navigate`、`click`、`input`、`wait`、标签页与内容提取
- DOM 提取包含 revision 级别的 `node_ref` 与 iframe 元数据
- 支持使用 CSS 选择器、数字索引或 `node_ref` 定位元素
- 原始 JavaScript 执行与基于文件路径的截图属于显式启用的操作员工具
- 线程安全的浏览器会话管理

## 工具分层

默认的 `ToolRegistry` 与 MCP 服务器只暴露高级文档交互契约。
`evaluate` 与基于路径的 `screenshot` 不再属于默认工具面，而是需要显式注册：

```rust
use browser_use::tools::ToolRegistry;

let mut registry = ToolRegistry::with_defaults();
registry.register_operator_tools();
```

## 环境要求

- Rust 1.70+
- 已安装 Chrome 或 Chromium 浏览器

## 致谢

本项目灵感来源于 [agent-infra/mcp-server-browser](https://github.com/bytedance/UI-TARS-desktop/tree/main/packages/agent-infra/mcp-servers/browser) 并参考了其实现。

## 许可证

MIT
