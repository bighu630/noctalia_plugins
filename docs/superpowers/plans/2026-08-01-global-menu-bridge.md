# Global Menu 插件（桥接程序 + Luau）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Noctalia v5 上实现 macOS 风格全局菜单：Rust 桥接程序（niri IPC 焦点跟踪 + AT-SPI 菜单读取 + 本地 HTTP 命令服务）通过 runStream stdout NDJSON 向 Luau 插件广播统一菜单模型，插件渲染菜单栏条与子菜单弹出面板，点击经 HTTP 回传桥执行 AT-SPI DoAction。

**Architecture:** 三层：① GTK3 应用经 AT-SPI 总线暴露菜单（本机已验证）；② Rust 桥（单二进制、无状态、纯线程模型）订阅 `niri msg --json event-stream` 焦点事件，解析焦点应用菜单树，经 stdout 逐行 NDJSON 上行，经 `127.0.0.1` 随机端口 HTTP 下行；③ Luau 插件 `service.luau` 托管桥并广播到 `noctalia.state`，`widget.luau` 渲染菜单栏条，`popup.luau` 渲染子菜单并回传点击。

**Tech Stack:** Rust（zbus 4.4 blocking + async-io、serde/serde_json、tiny_http、anyhow、std threads + mpsc，无 tokio）、Luau（Noctalia v5：runStream / state.set/get/watch / http / ui.* / togglePanel）。

**设计文档:** `docs/superpowers/specs/2026-08-01-global-menu-bridge-design.md`
**参考实现:** `/tmp/pi-github-repos/yolo-labz/noctalia-appmenu/bridge/src/`（atspi.rs 2528 行生产验证；niri.rs 外部标签 serde；proxy.rs 会话）
**关键实证:** AT-SPI 角色/状态 wire 常量见 at-spi2-core `atspi-constants.h`；GIMP 菜单树关闭状态可读；DoAction(0) 有效；runStream 单行上限 64KB、无 stdin、无 stop API；`noctalia.togglePanel("author/plugin:panel")`；`noctalia.http({url,method,body,headers}, function(ok,status,body))`；`noctalia.pluginDir()`。

---

## 文件结构

```
global-menu/
├── plugin.toml              # v5 清单：service + widget + panel 三入口
├── service.luau             # 托管桥 + NDJSON 解析 + state 广播 + 健康检查重启
├── widget.luau              # 菜单栏条（ui.row + ui.button）
├── popup.luau               # 子菜单弹出面板
├── bridge/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs          # 组装：总线 → status → http → hello → niri → 事件循环
│       ├── protocol.rs      # 统一菜单模型 + NDJSON 事件类型（serde）
│       ├── niri.rs          # niri IPC：事件流解析 + windows 快照查询
│       ├── atspi.rs         # AT-SPI 客户端：发现/定位/walker/DoAction
│       ├── proxy.rs         # 会话管理 + 焦点解析 + 点击/展开处理
│       ├── http.rs          # tiny_http 本地命令服务（ping/click/open/refresh/shutdown）
│       ├── stdout_io.rs     # NDJSON 上行发射器（行锁）
│       └── status.rs        # org.a11y.Status 拥有者（IsEnabled=true，Qt 预留）
├── scripts/
│   ├── build.sh             # cargo build --release + 拷贝二进制到插件目录
│   └── smoke.sh             # 真实环境冒烟
└── README.md
```

---

## Task 1: 工程骨架（plugin.toml + Cargo 工程）

**Files:**
- Create: `global-menu/plugin.toml`
- Create: `global-menu/bridge/Cargo.toml`
- Create: `global-menu/bridge/src/main.rs`
- Create: `global-menu/scripts/build.sh`

- [ ] **Step 1: 创建 plugin.toml**

```toml
id = "bighu630/global-menu"
name = "Global Menu"
version = "0.1.0"
plugin_api = 14
author = "Bighu630"
license = "MIT"
description = "macOS-style global menu for GTK3 apps on niri via an AT-SPI bridge."
tags = ["bar", "utility"]

[[service]]
id = "global_menu_service"
entry = "service.luau"

[[widget]]
id = "global_menu"
entry = "widget.luau"

[[panel]]
id = "global_menu_popup"
entry = "popup.luau"
placement = "floating"
dismiss_on_outside_click = true
keyboard_focus = "on_demand"
```

- [ ] **Step 2: 创建 Cargo.toml**

```toml
[package]
name = "noctalia-global-menu-bridge"
version = "0.1.0"
edition = "2021"

[dependencies]
zbus = { version = "4.4", default-features = false, features = ["blocking", "async-io"] }
zvariant = "4.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
tiny_http = "0.12"

[profile.release]
opt-level = 2
strip = true
```

- [ ] **Step 3: 创建占位 main.rs（验证编译 + stdout 输出）**

```rust
use std::io::Write;

fn main() {
    // Placeholder: Task 7 组装完整启动流程。先验证工程可编译、stdout 可逐行输出。
    let hello = serde_json::json!({"type": "hello", "port": 0, "pid": std::process::id()});
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{hello}");
    let _ = out.flush();
}
```

- [ ] **Step 4: 构建并验证**

Run: `cd global-menu/bridge && cargo build 2>&1 | tail -3`
Expected: `Finished`，无 error。

Run: `./target/debug/noctalia-global-menu-bridge`
Expected: 输出一行 `{"type":"hello","port":0,"pid":...}` 后退出。

- [ ] **Step 5: 创建 scripts/build.sh（后续所有任务用它分发二进制）**

```bash
#!/usr/bin/env bash
# Build the bridge and copy the binary into the plugin directory.
set -euo pipefail
cd "$(dirname "$0")/../bridge"
cargo build --release
mkdir -p ../bridge-bin
cp target/release/noctalia-global-menu-bridge ../bridge-bin/
echo "bridge installed to global-menu/bridge-bin/"
```

- [ ] **Step 6: Commit**

```bash
git add global-menu/
git commit -m "feat(global-menu): 工程骨架（plugin.toml + Rust bridge 工程）"
```

---

## Task 2: protocol.rs（统一菜单模型 + NDJSON 事件）

**Files:**
- Create: `global-menu/bridge/src/protocol.rs`
- Modify: `global-menu/bridge/src/main.rs`（删占位，改 `mod protocol;` + 最小使用）

- [ ] **Step 1: 写失败测试（序列化断言）**

创建 `global-menu/bridge/tests/protocol_test.rs`：

```rust
use noctalia_global_menu_bridge::protocol::*;

#[test]
fn menu_item_serializes_with_expected_shape() {
    let item = MenuItem {
        id: 3,
        label: "Export…".into(),
        mnemonic: Some("E".into()),
        item_type: MenuItemType::Item,
        enabled: true,
        visible: true,
        checked: false,
        icon: None,
        children: vec![],
    };
    let v: serde_json::Value = serde_json::to_value(&item).unwrap();
    assert_eq!(v["id"], 3);
    assert_eq!(v["label"], "Export…");
    assert_eq!(v["type"], "item");
    assert_eq!(v["checked"], false);
    assert!(v.get("mnemonic").is_some());
    assert!(v.get("icon").is_none()); // None 字段 skip
}

#[test]
fn bridge_event_menu_uses_internal_tag() {
    let ev = BridgeEvent::Menu {
        app: AppInfo { app_id: "gimp".into(), title: "GIMP".into(), pid: 1234 },
        menu: None,
        source: "none",
    };
    let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "menu");
    assert_eq!(v["app"]["app_id"], "gimp");
    assert_eq!(v["menu"], serde_json::Value::Null);
    assert_eq!(v["source"], "none");
}

#[test]
fn checkbox_and_separator_types() {
    assert_eq!(serde_json::to_string(&MenuItemType::Checkbox).unwrap(), "\"checkbox\"");
    assert_eq!(serde_json::to_string(&MenuItemType::Separator).unwrap(), "\"separator\"");
    assert_eq!(serde_json::to_string(&MenuItemType::Submenu).unwrap(), "\"submenu\"");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -5`
Expected: `error[E0432]: unresolved import noctalia_global_menu_bridge`（lib 未建）。

- [ ] **Step 3: 建 lib.rs + protocol.rs**

`global-menu/bridge/src/lib.rs`：

```rust
pub mod protocol;
```

`global-menu/bridge/src/protocol.rs`：

```rust
use serde::Serialize;

/// 统一菜单项模型（桥 → 插件）。
/// id 为桥按 DFS 序分配的会话内稳定序号；path 为相对菜单栏的 child-index 链，
/// 点击/展开时提交 path，桥重解析后按 path 定位（树变化时比序号健壮）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MenuItem {
    pub id: u32,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnemonic: Option<String>,
    #[serde(rename = "type")]
    pub item_type: MenuItemType,
    pub enabled: bool,
    pub visible: bool,
    pub checked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<MenuItem>,
    /// child-index 链（相对 menubar 根），如 [3, 1, 2]。
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub path: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MenuItemType {
    Item,
    Submenu,
    Separator,
    Checkbox,
    Radio,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppInfo {
    pub app_id: String,
    pub title: String,
    pub pid: u32,
}

/// 桥 → 插件（stdout 每行一个）的 NDJSON 事件。
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    Hello { port: u16, pid: u32 },
    Menu {
        app: AppInfo,
        #[serde(skip_serializing_if = "Option::is_none")]
        menu: Option<MenuItem>,
        source: &'static str,
    },
    Heartbeat { ts: u64 },
    Error { msg: String },
}

impl BridgeEvent {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("BridgeEvent serialization cannot fail")
    }
}
```

- [ ] **Step 4: 更新 main.rs 引用 lib（跑测试用）**

`global-menu/bridge/src/main.rs`：

```rust
use noctalia_global_menu_bridge::protocol::BridgeEvent;

fn main() {
    let ev = BridgeEvent::Hello { port: 0, pid: std::process::id() };
    println!("{}", ev.to_line());
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -6`
Expected: `test result: ok. 3 passed`

- [ ] **Step 6: Commit**

```bash
git add global-menu/bridge/
git commit -m "feat(global-menu): protocol.rs 统一菜单模型与 NDJSON 事件"
```

---

## Task 3: niri.rs（IPC 事件流解析 + windows 快照）

**Files:**
- Create: `global-menu/bridge/src/niri.rs`
- Modify: `global-menu/bridge/src/lib.rs`
- Test: `global-menu/bridge/tests/niri_test.rs`

- [ ] **Step 1: 写失败测试（fixture 来自本机实测）**

`global-menu/bridge/tests/niri_test.rs`：

```rust
use noctalia_global_menu_bridge::niri::{NiriEvent, NiriWindow, parse_event_line, focused_window_from_json};

#[test]
fn parses_externally_tagged_focus_event() {
    // niri 26.04 实测线格式
    let line = r#"{"WindowFocusChanged":{"id":30}}"#;
    match parse_event_line(line).unwrap() {
        NiriEvent::WindowFocusChanged { id } => assert_eq!(id, 30),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn unknown_event_falls_through_to_other() {
    // schema 漂移绝不崩溃（ADR-0016 教训）
    let line = r#"{"SomeFutureEvent":{"x":1}}"#;
    assert!(matches!(parse_event_line(line).unwrap(), NiriEvent::Other));
    let line2 = r#"{"WindowFocusChanged":{"id":30},"ExtraKey":1}"#;
    assert!(matches!(parse_event_line(line2).unwrap(), NiriEvent::Other));
    let line3 = r#"not json at all"#;
    assert!(parse_event_line(line3).is_err());
}

#[test]
fn parses_windows_changed_with_full_window_list() {
    let line = r#"{"WindowsChanged":{"windows":[
      {"id":30,"title":"pi-web-access","app_id":"google-chrome-canary","pid":1310790,"workspace_id":2,"is_focused":false,"is_floating":false,"is_urgent":false,"layout":{},"focus_timestamp":{"secs":1,"nanos":2}},
      {"id":34,"title":"PiPlus","app_id":"piplus","pid":1377900,"workspace_id":2,"is_focused":true,"is_floating":false,"is_urgent":false,"layout":{},"focus_timestamp":{"secs":1,"nanos":2}}
    ]}}"#;
    match parse_event_line(line).unwrap() {
        NiriEvent::WindowsChanged { windows } => {
            assert_eq!(windows.len(), 2);
            assert_eq!(windows[1].id, 34);
            assert_eq!(windows[1].pid, 1377900);
            assert!(windows[1].is_focused);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn focused_window_extraction() {
    let windows = vec![
        NiriWindow { id: 1, title: "a".into(), app_id: "x".into(), pid: 10, workspace_id: Some(1), is_focused: false },
        NiriWindow { id: 2, title: "b".into(), app_id: "y".into(), pid: 20, workspace_id: Some(1), is_focused: true },
    ];
    assert_eq!(focused_window_from_json(&windows).unwrap().id, 2);
    assert!(focused_window_from_json(&vec![]).is_none());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -4`
Expected: `error[E0432]` unresolved import `niri`。

- [ ] **Step 3: 实现 niri.rs**

```rust
//! niri IPC 客户端：`niri msg --json event-stream` 事件解析 + windows 快照。
//!
//! niri 26.04 事件为**外部标签** JSON（`{"WindowFocusChanged":{"id":30}}`）。
//! 未知事件/未知字段一律落 `Other`，绝不崩溃（ADR-0016 教训）。

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum NiriEvent {
    WindowFocusChanged { id: u64 },
    WindowFocusTimestampChanged { id: u64 },
    WorkspaceActiveWindowChanged { workspace_id: u64, active_window_id: Option<u64> },
    WindowsChanged { windows: Vec<NiriWindow> },
    Other,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NiriWindow {
    pub id: u64,
    pub title: String,
    #[serde(rename = "app_id")]
    pub app_id: String,
    pub pid: u32,
    #[serde(default)]
    pub workspace_id: Option<u64>,
    #[serde(rename = "is_focused", default)]
    pub is_focused: bool,
}

/// 解析 event-stream 的一行。任何无法精确匹配的输入 → Other，绝不 panic。
pub fn parse_event_line(line: &str) -> Result<NiriEvent> {
    let v: Value = serde_json::from_str(line)?;
    let map = v.as_object().ok_or_else(|| anyhow!("event line is not an object"))?;
    if map.len() != 1 {
        return Ok(NiriEvent::Other);
    }
    let (tag, payload) = map.iter().next().expect("len checked");
    let get = |key: &str| payload.get(key).and_then(Value::as_u64);
    Ok(match tag.as_str() {
        "WindowFocusChanged" => NiriEvent::WindowFocusChanged { id: get("id").unwrap_or(0) },
        "WindowFocusTimestampChanged" => NiriEvent::WindowFocusTimestampChanged { id: get("id").unwrap_or(0) },
        "WorkspaceActiveWindowChanged" => NiriEvent::WorkspaceActiveWindowChanged {
            workspace_id: get("workspace_id").unwrap_or(0),
            active_window_id: payload.get("active_window_id").and_then(Value::as_u64),
        },
        "WindowsChanged" => NiriEvent::WindowsChanged {
            windows: serde_json::from_value(payload.get("windows").cloned().unwrap_or(Value::Array(vec![])))
                .unwrap_or_default(),
        },
        _ => NiriEvent::Other,
    })
}

/// 从窗口列表找当前焦点窗口。
pub fn focused_window_from_json(windows: &[NiriWindow]) -> Option<&NiriWindow> {
    windows.iter().find(|w| w.is_focused)
}

/// 查询 `niri msg --json windows` 全量快照。
/// 失败（niri 不在运行/NIRI_SOCKET 未设）返回 Err，调用方降级处理。
pub fn query_windows() -> Result<Vec<NiriWindow>> {
    let out = Command::new("niri")
        .args(["msg", "--json", "windows"])
        .output()?;
    if !out.status.success() {
        return Err(anyhow!("niri msg failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let windows: Vec<NiriWindow> = serde_json::from_slice(&out.stdout)?;
    Ok(windows)
}
```

- [ ] **Step 4: 更新 lib.rs**

```rust
pub mod niri;
pub mod protocol;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -4`
Expected: `test result: ok. 4 passed`

- [ ] **Step 6: Commit**

```bash
git add global-menu/bridge/
git commit -m "feat(global-menu): niri.rs IPC 事件流解析（外部标签 + Other 兜底）"
```

---

## Task 4: atspi.rs 读取侧（总线发现 + 定位 + walker）

**Files:**
- Create: `global-menu/bridge/src/atspi.rs`
- Modify: `global-menu/bridge/src/lib.rs`
- Test: `global-menu/bridge/tests/atspi_walker_test.rs`

- [ ] **Step 1: 写失败测试（纯 walker 逻辑）**

`global-menu/bridge/tests/atspi_walker_test.rs`：

```rust
use noctalia_global_menu_bridge::atspi::{build_menu_tree, RawNode, ROLE_CHECK_MENU_ITEM, ROLE_MENU, ROLE_MENU_BAR, ROLE_MENU_ITEM, ROLE_SEPARATOR, STATE_CHECKED, STATE_ENABLED, STATE_SENSITIVE, STATE_VISIBLE};
use noctalia_global_menu_bridge::protocol::{MenuItemType};

fn node(role: u32, name: &str, state: (u32, u32), children: Vec<RawNode>) -> RawNode {
    RawNode { role, name: name.into(), state, children, acc: None }
}

#[test]
fn builds_tree_with_types_and_checked_state() {
    // File ▸ New / Open… ; View ─ Show All [checked]
    let root = node(ROLE_MENU_BAR, "", (0, 0), vec![
        node(ROLE_MENU, "File", (0, 0), vec![
            node(ROLE_MENU_ITEM, "New", (1 << STATE_ENABLED | 1 << STATE_SENSITIVE | 1 << STATE_VISIBLE, 0), vec![]),
            node(ROLE_MENU_ITEM, "Open…", (1 << STATE_ENABLED | 1 << STATE_SENSITIVE | 1 << STATE_VISIBLE, 0), vec![]),
        ]),
        node(ROLE_MENU, "View", (0, 0), vec![
            node(ROLE_CHECK_MENU_ITEM, "Show All", (1 << STATE_ENABLED | 1 << STATE_SENSITIVE | 1 << STATE_VISIBLE | 1 << STATE_CHECKED, 0), vec![]),
            node(ROLE_SEPARATOR, "", (1 << STATE_VISIBLE, 0), vec![]),
        ]),
    ]);
    let mut ids = 0u32;
    let tree = build_menu_tree(&root, &mut ids);
    assert_eq!(tree.item_type, MenuItemType::Submenu); // menubar 根按 submenu 承载
    assert_eq!(tree.children.len(), 2);
    assert_eq!(tree.children[0].label, "File");
    assert_eq!(tree.children[0].item_type, MenuItemType::Submenu);
    assert_eq!(tree.children[0].children[1].label, "Open…");
    assert_eq!(tree.children[0].children[1].path, vec![0, 1]);
    assert_eq!(tree.children[1].children[0].label, "Show All");
    assert_eq!(tree.children[1].children[0].item_type, MenuItemType::Checkbox);
    assert!(tree.children[1].children[0].checked);
    assert_eq!(tree.children[1].children[1].item_type, MenuItemType::Separator);
}

#[test]
fn ids_are_dfs_ordered_and_unique() {
    let root = node(ROLE_MENU_BAR, "", (0, 0), vec![
        node(ROLE_MENU, "A", (0, 0), vec![
            node(ROLE_MENU_ITEM, "A1", (1 << STATE_VISIBLE, 0), vec![]),
            node(ROLE_MENU_ITEM, "A2", (1 << STATE_VISIBLE, 0), vec![]),
        ]),
        node(ROLE_MENU, "B", (0, 0), vec![
            node(ROLE_MENU_ITEM, "B1", (1 << STATE_VISIBLE, 0), vec![]),
        ]),
    ]);
    let mut ids = 0u32;
    let tree = build_menu_tree(&root, &mut ids);
    let mut seen = std::collections::HashSet::new();
    fn walk(t: &noctalia_global_menu_bridge::protocol::MenuItem, seen: &mut std::collections::HashSet<u32>) {
        assert!(seen.insert(t.id), "duplicate id {}", t.id);
        for c in &t.children { walk(c, seen); }
    }
    walk(&tree, &mut seen);
    assert_eq!(ids, 6); // 根 + 2 顶层 + 2 + 1 = 6 个节点（separator 也占 id）
}

#[test]
fn invisible_items_are_filtered() {
    let root = node(ROLE_MENU_BAR, "", (0, 0), vec![
        node(ROLE_MENU, "A", (0, 0), vec![
            node(ROLE_MENU_ITEM, "Hidden", (1 << STATE_ENABLED, 0), vec![]), // 无 VISIBLE
            node(ROLE_MENU_ITEM, "Shown", (1 << STATE_VISIBLE, 0), vec![]),
        ]),
    ]);
    let mut ids = 0u32;
    let tree = build_menu_tree(&root, &mut ids);
    assert_eq!(tree.children[0].children.len(), 1);
    assert_eq!(tree.children[0].children[0].label, "Shown");
}

#[test]
fn qt_wrapper_menu_is_flattened() {
    // Qt 每个 MENU_ITEM 的 popup 包在无名 MENU 里 → 扁平化
    let root = node(ROLE_MENU_BAR, "", (0, 0), vec![
        node(ROLE_MENU, "File", (0, 0), vec![
            node(ROLE_MENU_ITEM, "Open", (1 << STATE_VISIBLE, 0), vec![
                node(ROLE_MENU, "", (0, 0), vec![
                    node(ROLE_MENU_ITEM, "Open Recent", (1 << STATE_VISIBLE, 0), vec![]),
                ]),
            ]),
        ]),
    ]);
    let mut ids = 0u32;
    let tree = build_menu_tree(&root, &mut ids);
    let open = &tree.children[0].children[0];
    assert_eq!(open.item_type, MenuItemType::Submenu);
    assert_eq!(open.children.len(), 1);
    assert_eq!(open.children[0].label, "Open Recent");
    assert_eq!(open.children[0].path, vec![0, 0, 0]);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -4`
Expected: unresolved import `atspi`。

- [ ] **Step 3: 实现 atspi.rs（读取侧 + walker）**

```rust
//! AT-SPI 客户端：a11y 总线发现、应用定位、菜单树读取。
//!
//! 角色/状态 wire 常量来自 at-spi2-core `atspi-constants.h`（2026-05 实测核对）。
//! 参考实现：noctalia-appmenu bridge/src/atspi.rs（ADR-0024）。

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::sync::Arc;
use zbus::blocking::Connection;
use zvariant::OwnedObjectPath;

pub const ROLE_CHECK_MENU_ITEM: u32 = 8;
pub const ROLE_FRAME: u32 = 28;
pub const ROLE_MENU: u32 = 33;
pub const ROLE_MENU_BAR: u32 = 34;
pub const ROLE_MENU_ITEM: u32 = 35;
pub const ROLE_RADIO_MENU_ITEM: u32 = 45;
pub const ROLE_SEPARATOR: u32 = 50;
pub const ROLE_WINDOW: u32 = 15;

pub const STATE_ENABLED: u32 = 8;
pub const STATE_SENSITIVE: u32 = 24;
pub const STATE_VISIBLE: u32 = 31;
pub const STATE_CHECKED: u32 = 4;
pub const STATE_SHOWING: u32 = 25;

const IFACE_ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const IFACE_REGISTRY: &str = "org.a11y.atspi.Registry";
const REGISTRY_PATH: &str = "/org/a11y/atspi/registry";
const A11Y_SERVICE: &str = "org.a11y.Bus";
const A11Y_PATH: &str = "/org/a11y/bus";
const MAX_DEPTH: usize = 8;

/// a11y 总线上一个 accessible 的坐标。
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibleRef {
    pub bus: String,
    pub path: OwnedObjectPath,
}

/// walker 的中间表示（与 zbus 调用解耦，便于纯函数测试）。
#[derive(Debug, Clone)]
pub struct RawNode {
    pub role: u32,
    pub name: String,
    pub state: (u32, u32),
    pub children: Vec<RawNode>,
    pub acc: Option<AccessibleRef>,
}

pub struct AtspiClient {
    conn: Connection,
}

impl AtspiClient {
    /// 通过会话总线的 org.a11y.Bus.GetAddress 发现并连接 a11y 总线。
    pub fn connect() -> Result<Self> {
        let session = Connection::session().context("session bus")?;
        let addr: String = session
            .call_method(Some(A11Y_SERVICE), A11Y_PATH, Some("org.a11y.Bus"), "GetAddress", &())
            .context("org.a11y.Bus.GetAddress")?
            .body()
            .deserialize()?;
        let conn = Connection::address(&addr)
            .context("a11y bus address")?
            .build()
            .context("a11y bus connect")?;
        Ok(Self { conn })
    }

    // ── 单节点原语 ──────────────────────────────────────────────

    fn get_role(&self, acc: &AccessibleRef) -> Result<u32> {
        Ok(self
            .conn
            .call_method(Some(&acc.bus), acc.path.as_str(), Some(IFACE_ACCESSIBLE), "GetRole", &())?
            .body()
            .deserialize()?)
    }

    fn get_name(&self, acc: &AccessibleRef) -> Result<String> {
        Ok(self
            .conn
            .call_method(Some(&acc.bus), acc.path.as_str(), Some(IFACE_ACCESSIBLE), "GetName", &())?
            .body()
            .deserialize()?)
    }

    fn get_state(&self, acc: &AccessibleRef) -> Result<(u32, u32)> {
        Ok(self
            .conn
            .call_method(Some(&acc.bus), acc.path.as_str(), Some(IFACE_ACCESSIBLE), "GetState", &())?
            .body()
            .deserialize()?)
    }

    fn child_count(&self, acc: &AccessibleRef) -> Result<i32> {
        Ok(self
            .conn
            .call_method(Some(&acc.bus), acc.path.as_str(), Some(IFACE_ACCESSIBLE), "GetChildCount", &())?
            .body()
            .deserialize()?)
    }

    fn child_at(&self, acc: &AccessibleRef, i: i32) -> Result<Option<AccessibleRef>> {
        #[derive(Deserialize)]
        struct Child((String, OwnedObjectPath));
        let (bus, path): (String, OwnedObjectPath) = self
            .conn
            .call_method(
                Some(&acc.bus),
                acc.path.as_str(),
                Some(IFACE_ACCESSIBLE),
                "GetChildAtIndex",
                &(i,),
            )?
            .body()
            .deserialize()?;
        if bus.is_empty() {
            Ok(None)
        } else {
            Ok(Some(AccessibleRef { bus, path }))
        }
    }

    fn children(&self, acc: &AccessibleRef) -> Vec<AccessibleRef> {
        let mut out = Vec::new();
        if let Ok(n) = self.child_count(acc) {
            for i in 0..n {
                match self.child_at(acc, i) {
                    Ok(Some(c)) => out.push(c),
                    _ => break,
                }
            }
        }
        out
    }

    /// 递归读节点为 RawNode（深度受限，单节点失败跳过该分支）。
    fn read_node(&self, acc: &AccessibleRef, depth: usize) -> Option<RawNode> {
        if depth > MAX_DEPTH {
            return None;
        }
        let role = self.get_role(acc).ok()?;
        let name = self.get_name(acc).unwrap_or_default();
        let state = self.get_state(acc).unwrap_or((0, 0));
        let children = self
            .children(acc)
            .into_iter()
            .filter_map(|c| self.read_node(&c, depth + 1))
            .collect();
        Some(RawNode { role, name, state, children, acc: Some(acc.clone()) })
    }

    // ── 应用定位 ────────────────────────────────────────────────

    /// a11y 总线上所有注册应用（bus name, root accessible path）。
    fn registered_applications(&self) -> Result<Vec<(String, OwnedObjectPath)>> {
        Ok(self
            .conn
            .call_method(
                Some("org.a11y.atspi.Registry"),
                REGISTRY_PATH,
                Some(IFACE_REGISTRY),
                "GetRegisteredApplications",
                &(),
            )?
            .body()
            .deserialize()?)
    }

    /// a11y 总线连接 → PID（与会话总线的 GetConnectionUnixProcessID 不同，a11y 总线是独立 daemon）。
    fn pid_of(&self, bus_name: &str) -> Result<u32> {
        Ok(self
            .conn
            .call_method(
                None::<&str>,
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetConnectionUnixProcessID",
                &(bus_name,),
            )?
            .body()
            .deserialize()?)
    }

    /// 按 PID 找应用根 accessible。
    pub fn find_app_for_pid(&self, pid: u32) -> Result<Option<AccessibleRef>> {
        for (bus, path) in self.registered_applications()? {
            if self.pid_of(&bus).ok() == Some(pid) {
                return Ok(Some(AccessibleRef { bus, path }));
            }
        }
        Ok(None)
    }

    /// 多窗口同 PID：用 niri 焦点窗口 title 精确匹配 frame（ADR-0030：绝不猜）。
    /// 返回 Some(frame) = 从该 frame 找菜单；None = 从 app 根找；Err 仅网络层错误。
    pub fn choose_frame(&self, app: &AccessibleRef, title: &str) -> Result<Option<AccessibleRef>> {
        let frames = self.children(app);
        if frames.len() <= 1 {
            return Ok(None); // 单窗口：app 根
        }
        for f in &frames {
            let role = self.get_role(f).unwrap_or(0);
            if (role == ROLE_FRAME || role == ROLE_WINDOW) && self.get_name(f).ok().as_deref() == Some(title) {
                return Ok(Some(f.clone()));
            }
        }
        // 多窗口但匹配不到 → None 且上层判定"无法识别"，回退占位
        Ok(None)
    }

    /// 从起点 DFS 找 MENU_BAR（深度受限）。
    pub fn find_menubar(&self, root: &AccessibleRef) -> Result<Option<AccessibleRef>> {
        let mut stack = vec![root.clone()];
        for _ in 0..(1 << (MAX_DEPTH + 2)) {
            let Some(acc) = stack.pop() else { break };
            if self.get_role(&acc).unwrap_or(0) == ROLE_MENU_BAR {
                return Ok(Some(acc));
            }
            stack.extend(self.children(&acc).into_iter().rev());
        }
        Ok(None)
    }

    /// 完整链路：pid → app →（title 匹配 frame）→ menubar → RawNode 树。
    pub fn fetch_menubar(&self, pid: u32, title: &str) -> Result<Option<RawNode>> {
        let Some(app) = self.find_app_for_pid(pid)? else { return Ok(None) };
        let scope = match self.choose_frame(&app, title)? {
            Some(frame) => frame,
            None => app,
        };
        let Some(menubar) = self.find_menubar(&scope)? else { return Ok(None) };
        Ok(self.read_node(&menubar, 0))
    }
}

// ── 纯逻辑：RawNode → 统一菜单树 ───────────────────────────────

/// 由 RawNode 构建 MenuItem 树。ids 为 DFS 分配器（会话内连续）。
/// 规则：
/// - role==MENU_BAR/MENU 且 name 非空 → submenu（menubar 根 label 取空）
/// - MENU_ITEM → item；子节点为无名 MENU 时扁平化（Qt wrapper）
/// - CHECK/RADIO_MENU_ITEM → checkbox/radio（checked 取 state 位）
/// - SEPARATOR → separator
/// - 无 STATE_VISIBLE 的项过滤（不占 id）
/// - path = 相对根的 child-index 链（**原始** children 索引，含不可见项；与 locate_by_path 一致）
/// build_item 的公开包装（proxy::open_path 需要与主树同形的子项）。
pub fn build_item_pub(node: &RawNode, path: &[u32], ids: &mut u32) -> crate::protocol::MenuItem {
    build_item(node, path.to_vec(), ids)
}

pub fn build_menu_tree(root: &RawNode, ids: &mut u32) -> crate::protocol::MenuItem {
    use crate::protocol::{MenuItem, MenuItemType};
    let id = { *ids += 1; *ids };
    let path = vec![];
    let mut children = Vec::new();
    for (i, child) in root.children.iter().enumerate() {
        let visible = child.state.0 & (1 << STATE_VISIBLE) != 0
            || child.state.0 & (1 << STATE_SHOWING) != 0;
        if !visible {
            continue;
        }
        let child_path = {
            let mut p = path.clone();
            p.push(i as u32);
            p
        };
        let item = build_item(child, child_path, ids);
        children.push(item);
    }
    MenuItem {
        id,
        label: root.name.clone(),
        mnemonic: None,
        item_type: MenuItemType::Submenu,
        enabled: true,
        visible: true,
        checked: false,
        icon: None,
        children,
        path,
    }
}

fn build_item(node: &RawNode, path: Vec<u32>, ids: &mut u32) -> crate::protocol::MenuItem {
    use crate::protocol::{MenuItem, MenuItemType};
    let id = { *ids += 1; *ids };
    let enabled = node.state.0 & (1 << STATE_ENABLED) != 0
        && node.state.0 & (1 << STATE_SENSITIVE) != 0;
    let checked = node.state.0 & (1 << STATE_CHECKED) != 0;

    let is_submenu_like = node.role == ROLE_MENU_BAR || node.role == ROLE_MENU;
    let mut children = Vec::new();
    if is_submenu_like {
        for (i, child) in node.children.iter().enumerate() {
            let visible = child.state.0 & (1 << STATE_VISIBLE) != 0
                || child.state.0 & (1 << STATE_SHOWING) != 0;
            if !visible {
                continue;
            }
            let mut child_path = path.clone();
            child_path.push(i as u32);
            children.push(build_item(child, child_path, ids));
        }
    } else {
        // item 的 children：Qt 包装的无名 MENU → 扁平化为 submenu
        let mut popup_children: Vec<&RawNode> = Vec::new();
        for child in &node.children {
            if child.role == ROLE_MENU && child.name.is_empty() {
                popup_children.extend(child.children.iter());
            } else {
                popup_children.push(child);
            }
        }
        for (i, child) in popup_children.iter().enumerate() {
            let visible = child.state.0 & (1 << STATE_VISIBLE) != 0
                || child.state.0 & (1 << STATE_SHOWING) != 0;
            if !visible {
                continue;
            }
            let mut child_path = path.clone();
            child_path.push(i as u32);
            children.push(build_item(child, child_path, ids));
        }
    }

    let item_type = if is_submenu_like {
        MenuItemType::Submenu
    } else if node.role == ROLE_SEPARATOR {
        MenuItemType::Separator
    } else if node.role == ROLE_CHECK_MENU_ITEM {
        MenuItemType::Checkbox
    } else if node.role == ROLE_RADIO_MENU_ITEM {
        MenuItemType::Radio
    } else {
        MenuItemType::Item
    };

    MenuItem {
        id,
        label: node.name.clone(),
        mnemonic: None,
        item_type,
        enabled,
        visible: true,
        checked,
        icon: None,
        children,
        path,
    }
}

pub type SharedAtspi = Arc<AtspiClient>;
```

- [ ] **Step 4: 更新 lib.rs**

```rust
pub mod atspi;
pub mod niri;
pub mod protocol;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -4`
Expected: `test result: ok. 4 passed`（atspi walker 4 个 + 之前 7 个）

- [ ] **Step 6: Commit**

```bash
git add global-menu/bridge/
git commit -m "feat(global-menu): atspi.rs 总线发现/应用定位/菜单树 walker"
```

---

## Task 5: atspi.rs 点击侧（DoAction + 按 path 重定位）

**Files:**
- Modify: `global-menu/bridge/src/atspi.rs`（追加方法）
- Test: `global-menu/bridge/tests/atspi_click_test.rs`

- [ ] **Step 1: 写失败测试（path 定位纯逻辑）**

`global-menu/bridge/tests/atspi_click_test.rs`：

```rust
use noctalia_global_menu_bridge::atspi::{locate_by_path, RawNode, ROLE_MENU, ROLE_MENU_BAR, ROLE_MENU_ITEM};

fn node(role: u32, name: &str, children: Vec<RawNode>) -> RawNode {
    RawNode { role, name: name.into(), state: (0, 0), children, acc: None }
}

#[test]
fn locates_by_child_index_path() {
    // menubar [File[New, Open], Edit[Undo]]，path [1, 0] → Undo
    let root = node(ROLE_MENU_BAR, "", vec![
        node(ROLE_MENU, "File", vec![
            node(ROLE_MENU_ITEM, "New", vec![]),
            node(ROLE_MENU_ITEM, "Open", vec![]),
        ]),
        node(ROLE_MENU, "Edit", vec![
            node(ROLE_MENU_ITEM, "Undo", vec![]),
        ]),
    ]);
    let target = locate_by_path(&root, &[1, 0]).expect("found");
    assert_eq!(target.name, "Undo");
}

#[test]
fn missing_path_returns_none() {
    let root = node(ROLE_MENU_BAR, "", vec![node(ROLE_MENU, "A", vec![])]);
    assert!(locate_by_path(&root, &[0, 5]).is_none());
    assert!(locate_by_path(&root, &[3]).is_none());
    assert!(locate_by_path(&root, &[]).is_some()); // 根本身
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -3`
Expected: unresolved `locate_by_path`。

- [ ] **Step 3: 实现 locate_by_path + 点击方法（追加到 atspi.rs）**

```rust
// ── 点击侧 ─────────────────────────────────────────────────────

/// 按 child-index 链定位节点（相对 menubar 根；索引为**全部子节点**索引，
/// 与 build_menu_tree 的 path 语义一致）。
pub fn locate_by_path<'a>(root: &'a RawNode, path: &[u32]) -> Option<&'a RawNode> {
    let mut cur = root;
    for &i in path {
        cur = cur.children.get(i as usize)?;
    }
    Some(cur)
}

impl AtspiClient {
    /// 重新执行完整解析（pid → app → frame → menubar → RawNode），按 path 定位，
    /// 再对目标做 DoAction。返回 (是否找到, 是否执行成功)。
    pub fn click_path(&self, pid: u32, title: &str, path: &[u32]) -> Result<(bool, bool)> {
        let Some(app) = self.find_app_for_pid(pid)? else { return Ok((false, false)) };
        let scope = match self.choose_frame(&app, title)? {
            Some(frame) => frame,
            None => app,
        };
        let Some(menubar) = self.find_menubar(&scope)? else { return Ok((false, false)) };
        let Some(root) = self.read_node(&menubar, 0) else { return Ok((false, false)) };
        let Some(target) = locate_by_path(&root, path) else { return Ok((false, false)) };
        let Some(acc) = &target.acc else { return Ok((false, false)) };
        let ok = self.do_action(acc)?;
        Ok((true, ok))
    }

    /// 重新解析并按 path 读取节点直接子项（懒构建兜底，/open 用）。
    /// 返回 (找到, 子树 RawNode 的 children)。
    pub fn read_children_by_path(
        &self,
        pid: u32,
        title: &str,
        path: &[u32],
    ) -> Result<(bool, Vec<RawNode>)> {
        let Some(app) = self.find_app_for_pid(pid)? else { return Ok((false, vec![])) };
        let scope = match self.choose_frame(&app, title)? {
            Some(frame) => frame,
            None => app,
        };
        let Some(menubar) = self.find_menubar(&scope)? else { return Ok((false, vec![])) };
        let Some(root) = self.read_node(&menubar, 0) else { return Ok((false, vec![])) };
        let Some(target) = locate_by_path(&root, path) else { return Ok((false, vec![])) };
        let Some(acc) = &target.acc else { return Ok((false, vec![])) };
        Ok((true, self.children(acc)))
    }

    /// AT-SPI Action 接口点击。qatspi 约定索引 0 = "click"；
    /// 兜底：枚举动作名匹配 "click"，否则尝试 0。返回执行结果。
    fn do_action(&self, acc: &AccessibleRef) -> Result<bool> {
        let count: i32 = self
            .conn
            .call_method(Some(&acc.bus), acc.path.as_str(), Some("org.a11y.atspi.Action"), "GetActionCount", &())?
            .body()
            .deserialize()?;
        if count <= 0 {
            return Ok(false);
        }
        let mut idx = -1i32;
        for i in 0..count {
            let name: String = self
                .conn
                .call_method(Some(&acc.bus), acc.path.as_str(), Some("org.a11y.atspi.Action"), "GetActionName", &(i,))?
                .body()
                .deserialize()?;
            if name.to_ascii_lowercase().contains("click") {
                idx = i;
                break;
            }
        }
        if idx < 0 {
            idx = 0;
        }
        let ok: bool = self
            .conn
            .call_method(Some(&acc.bus), acc.path.as_str(), Some("org.a11y.atspi.Action"), "DoAction", &(idx,))?
            .body()
            .deserialize()?;
        Ok(ok)
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -3`
Expected: `test result: ok. 2 passed`（新增）

- [ ] **Step 5: Commit**

```bash
git add global-menu/bridge/
git commit -m "feat(global-menu): atspi.rs DoAction 点击 + path 重定位"
```

---

## Task 6: proxy.rs（会话管理 + 事件组装）

**Files:**
- Create: `global-menu/bridge/src/proxy.rs`
- Modify: `global-menu/bridge/src/lib.rs`
- Test: `global-menu/bridge/tests/proxy_test.rs`

- [ ] **Step 1: 写失败测试（菜单事件组装 + 降级）**

`global-menu/bridge/tests/proxy_test.rs`：

```rust
use noctalia_global_menu_bridge::proxy::{FocusInfo, make_menu_event, build_children_response};
use noctalia_global_menu_bridge::protocol::{AppInfo, MenuItem, MenuItemType};

fn item(id: u32, label: &str, item_type: MenuItemType, children: Vec<MenuItem>) -> MenuItem {
    MenuItem { id, label: label.into(), mnemonic: None, item_type, enabled: true, visible: true, checked: false, icon: None, children, path: vec![] }
}

#[test]
fn menu_event_carries_app_and_tree() {
    let focus = FocusInfo { win_id: 5, app_id: "gimp".into(), title: "GIMP".into(), pid: 42 };
    let menu = Some(item(1, "File", MenuItemType::Submenu, vec![item(2, "New", MenuItemType::Item, vec![])]));
    let ev = make_menu_event(&focus, menu, "atspi");
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "menu");
    assert_eq!(v["app"]["pid"], 42);
    assert_eq!(v["app"]["app_id"], "gimp");
    assert_eq!(v["source"], "atspi");
    assert_eq!(v["menu"]["children"][0]["label"], "File");
}

#[test]
fn no_menu_yields_null_menu_with_none_source() {
    let focus = FocusInfo { win_id: 5, app_id: "chrome".into(), title: "Chrome".into(), pid: 7 };
    let ev = make_menu_event(&focus, None, "none");
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["menu"], serde_json::Value::Null);
    assert_eq!(v["source"], "none");
}

#[test]
fn children_response_uses_same_id_space() {
    // /open 响应：children 数组 + ok 标记（id 从解析后的树取）
    let children = vec![item(3, "Zoom", MenuItemType::Item, vec![])];
    let resp = build_children_response(true, children);
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["children"][0]["id"], 3);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -3`
Expected: unresolved import `proxy`。

- [ ] **Step 3: 实现 proxy.rs**

```rust
//! 会话管理：焦点信息 → 菜单树解析 → 事件组装。
//!
//! 桥无状态：每次焦点变化/点击/展开都从 niri + AT-SPI 重新解析。
//! id 语义 = DFS 解析序；path 语义 = 相对 menubar 的 child-index 链（点击/展开提交 path）。

use crate::atspi::{build_menu_tree, AtspiClient};
use crate::protocol::{AppInfo, BridgeEvent, MenuItem, MenuItemType};
use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct FocusInfo {
    pub win_id: u64,
    pub app_id: String,
    pub title: String,
    pub pid: u32,
}

/// 焦点变化处理：解析菜单并产出事件。menu=None 表示无菜单（回退占位）。
pub fn resolve_focus(atspi: &AtspiClient, focus: &FocusInfo) -> Result<Option<MenuItem>> {
    let Some(raw) = atspi.fetch_menubar(focus.pid, &focus.title)? else {
        return Ok(None);
    };
    let mut ids = 0u32;
    Ok(Some(build_menu_tree(&raw, &mut ids)))
}

pub fn make_menu_event(focus: &FocusInfo, menu: Option<MenuItem>, source: &'static str) -> BridgeEvent {
    BridgeEvent::Menu {
        app: AppInfo {
            app_id: focus.app_id.clone(),
            title: focus.title.clone(),
            pid: focus.pid,
        },
        menu,
        source,
    }
}

/// /open 响应体。
#[derive(serde::Serialize)]
pub struct ChildrenResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<MenuItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn build_children_response(ok: bool, children: Vec<MenuItem>) -> ChildrenResponse {
    ChildrenResponse { ok, children, error: None }
}

/// /click 响应体。
#[derive(serde::Serialize)]
pub struct ClickResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 点击：完整重解析 + path 定位 + DoAction（返回是否找到/执行）。
pub fn click_path(atspi: &AtspiClient, focus: &FocusInfo, path: &[u32]) -> Result<(bool, bool)> {
    atspi.click_path(focus.pid, &focus.title, path)
}

/// 展开兜底：重解析 + path 定位 + 读子项，转统一模型（id 空间与一次全新解析一致）。
pub fn open_path(atspi: &AtspiClient, focus: &FocusInfo, path: &[u32]) -> Result<(bool, Vec<MenuItem>)> {
    let (found, raws) = atspi.read_children_by_path(focus.pid, &focus.title, path)?;
    if !found {
        return Ok((false, vec![]));
    }
    let mut ids = 0u32;
    let mut items = Vec::new();
    for raw in raws {
        // 每个子项与主树 children 同形：build_item 而非 build_menu_tree（后者是容器）
        items.push(crate::atspi::build_item_pub(&raw, &[], &mut ids));
    }
    Ok((true, items))
}
```

- [ ] **Step 4: 更新 lib.rs**

```rust
pub mod atspi;
pub mod niri;
pub mod protocol;
pub mod proxy;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -3`
Expected: `test result: ok. 3 passed`（新增）

- [ ] **Step 6: Commit**

```bash
git add global-menu/bridge/
git commit -m "feat(global-menu): proxy.rs 会话解析与事件组装"
```

---

## Task 7: main.rs 组装（http + stdout + status + 事件循环）

**Files:**
- Create: `global-menu/bridge/src/http.rs`
- Create: `global-menu/bridge/src/stdout_io.rs`
- Create: `global-menu/bridge/src/status.rs`
- Rewrite: `global-menu/bridge/src/main.rs`
- Modify: `global-menu/bridge/src/lib.rs`

- [ ] **Step 1: 实现 stdout_io.rs**

```rust
//! NDJSON 上行发射器：逐行、加锁，避免多线程交错。
use crate::protocol::BridgeEvent;
use std::io::Write;
use std::sync::Mutex;

pub struct StdoutSink {
    lock: Mutex<()>,
}

impl StdoutSink {
    pub fn new() -> Self {
        Self { lock: Mutex::new(()) }
    }

    pub fn emit(&self, ev: &BridgeEvent) {
        let _g = self.lock.lock().unwrap();
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", ev.to_line());
        let _ = out.flush();
    }
}
```

- [ ] **Step 2: 实现 status.rs（org.a11y.Status 拥有者）**

```rust
//! 拥有会话总线 org.a11y.Status，置 IsEnabled=true。
//! GTK3 不需要它（自动启用），Qt 需要——为 Qt 预留（设计文档 §2）。
//!
//! zbus 4 的 #[interface] 宏生成接口实现；若宏 API 与 4.4 有出入，
//! 以 zbus 4 docs 为准调整，保持 XML 语义 org.a11y.Status / IsEnabled(b) 不变。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use zbus::blocking::Connection;
use zbus::interface;

pub struct A11yStatus {
    enabled: Arc<AtomicBool>,
}

impl A11yStatus {
    pub fn new() -> Self {
        Self { enabled: Arc::new(AtomicBool::new(true)) }
    }
}

#[interface(name = "org.a11y.Status")]
impl A11yStatus {
    #[zbus(property)]
    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

/// 尝试注册 org.a11y.Status。失败（名字已被占）仅 warn，不致命。
pub fn own_status(conn: &Connection) -> anyhow::Result<()> {
    use zbus::blocking::fdo::DBusProxy;
    let dbus = DBusProxy::new(conn)?;
    // 请求名字：REPLACE_EXISTING 不启用（不抢别人的）；失败静默。
    let flags = zbus::blocking::fdo::RequestNameFlags::empty();
    match dbus.request_name("org.a11y.Status", flags) {
        Ok(reply) if reply == zbus::blocking::fdo::RequestNameReply::PrimaryOwner => {
            let iface = A11yStatus::new();
            let conn2 = conn.clone();
            // zbus 4 blocking ObjectServer 注册（blocking::Connection::object_server）
            conn2.object_server().at("/org/a11y/bus", iface)?;
            Ok(())
        }
        Ok(_) => Err(anyhow::anyhow!("org.a11y.Status already owned by another service")),
        Err(e) => Err(anyhow::anyhow!("request_name failed: {e}")),
    }
}
```

- [ ] **Step 3: 实现 http.rs**

```rust
//! 本地回环 HTTP 命令服务（插件下行通道）。
//! 端点：GET /ping、POST /click、POST /open、POST /refresh、POST /shutdown。

use crate::atspi::SharedAtspi;
use crate::proxy::{build_children_response, click_path, open_path, Shared};
use std::io::Read;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Deserialize)]
struct PathBody {
    path: Vec<u32>,
}

#[derive(Clone)]
pub struct HttpServer {
    pub port: u16,
    shutdown: Arc<Mutex<Option<Sender<()>>>>,
}

pub fn spawn(
    shared: Arc<Mutex<Shared>>,
    atspi: SharedAtspi,
    refresh_tx: Sender<()>,
) -> Result<HttpServer> {
    let server = tiny_http::Server::http("127.0.0.1:0")?;
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
    let shutdown = Arc::new(Mutex::new(Some(shutdown_tx)));

    let server_for_sd = server.clone();
    thread::spawn(move || {
        let _ = shutdown_rx.recv();
        server_for_sd.unblock();
    });
    thread::spawn(move || {
        for request in server.incoming_requests() {
            let shared = shared.clone();
            let atspi = atspi.clone();
            let refresh_tx = refresh_tx.clone();
            let response = handle(&request, shared, atspi, refresh_tx);
            let _ = request.respond(response);
        }
    });

    Ok(HttpServer { port, shutdown })
}

fn handle(
    request: &tiny_http::Request,
    shared: Arc<Mutex<Shared>>,
    atspi: SharedAtspi,
    refresh_tx: Sender<()>,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let path = request.url().split('?').next().unwrap_or("/");
    let method = request.method();
    let json = |code: u16, v: serde_json::Value| {
        tiny_http::Response::from_string(v.to_string())
            .with_status_code(code)
            .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
    };
    let mut body = String::new();
    if let Ok(mut r) = request.as_reader().read_to_string(&mut body) {
        let _ = r;
    }

    let result: serde_json::Value = match (method, path) {
        (_, "/ping") => serde_json::json!({"ok": true}),

        (_, "/shutdown") => {
            std::process::exit(0);
        }

        (_, "/click") => {
            let st = shared.lock().unwrap();
            let Some(session) = &st.session else {
                return json(200, serde_json::json!({"ok": false, "error": "no session"}));
            };
            match serde_json::from_str::<PathBody>(&body) {
                Ok(b) => match click_path(&atspi, &session.focus, &b.path) {
                    Ok((found, clicked)) => serde_json::json!({"ok": found && clicked, "found": found, "clicked": clicked}),
                    Err(e) => serde_json::json!({"ok": false, "error": format!("{e:#}")}),
                },
                Err(e) => serde_json::json!({"ok": false, "error": format!("bad body: {e}")}),
            }
        }

        (_, "/open") => {
            let st = shared.lock().unwrap();
            let Some(session) = &st.session else {
                return json(200, serde_json::json!({"ok": false, "error": "no session"}));
            };
            match serde_json::from_str::<PathBody>(&body) {
                Ok(b) => match open_path(&atspi, &session.focus, &b.path) {
                    Ok((found, items)) => {
                        if found {
                            serde_json::to_value(build_children_response(true, items)).unwrap()
                        } else {
                            serde_json::json!({"ok": false, "error": "path not found"})
                        }
                    }
                    Err(e) => serde_json::json!({"ok": false, "error": format!("{e:#}")}),
                },
                Err(e) => serde_json::json!({"ok": false, "error": format!("bad body: {e}")}),
            }
        }

        (_, "/refresh") => {
            let _ = refresh_tx.send(());
            serde_json::json!({"ok": true})
        }

        _ => return json(404, serde_json::json!({"ok": false, "error": "not found"})),
    };
    json(200, result)
}

impl HttpServer {
    /// 通知 HTTP 线程停止（主循环退出时调用）。
    pub fn stop(&self) {
        if let Some(tx) = self.shutdown.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}
```

- [ ] **Step 4: 实现 main.rs（组装）**

```rust
//! Global Menu bridge 主程序。
//!
//! 启动顺序：会话总线 → org.a11y.Status（尽力）→ a11y 连接（失败退出，
//! 插件 ping 失败会重启桥）→ HTTP → hello → niri 订阅 → 事件循环。
//!
//! 线程：main=事件循环；niri 线程（event-stream 读取）；心跳由事件循环
//! recv_timeout 驱动；HTTP 线程（tiny_http 自管）。

mod atspi;
mod http;
mod niri;
mod protocol;
mod proxy;
mod stdout_io;
mod status;

use crate::atspi::{AtspiClient, SharedAtspi};
use crate::niri::{parse_event_line, NiriEvent, NiriWindow};
use crate::protocol::BridgeEvent;
use crate::proxy::{resolve_focus, FocusInfo, Shared};
use anyhow::Result;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEBOUNCE_MS: u64 = 150;
const HEARTBEAT_SECS: u64 = 5;

enum Ctrl {
    FocusChanged(u64),
    Windows(Vec<NiriWindow>),
    Refresh,
    Quit,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("[global-menu-bridge] fatal: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // 1. 会话总线 + a11y 总线
    let session = zbus::blocking::Connection::session()?;
    match status::own_status(&session) {
        Ok(()) => eprintln!("[global-menu-bridge] org.a11y.Status owned"),
        Err(e) => eprintln!("[global-menu-bridge] org.a11y.Status unavailable (non-fatal): {e:#}"),
    }
    let atspi: SharedAtspi = Arc::new(AtspiClient::connect().map_err(|e| anyhow::anyhow!("AT-SPI unavailable: {e:#}"))?);

    // 2. 共享状态 + HTTP
    let (refresh_tx, refresh_rx) = mpsc::channel::<()>();
    let (focus_tx, focus_rx) = mpsc::channel::<Ctrl>();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let http_server = http::spawn(shared.clone(), atspi.clone(), refresh_tx)?;
    let sink = stdout_io::StdoutSink::new();

    // 3. hello
    sink.emit(&BridgeEvent::Hello { port: http_server.port, pid: std::process::id() });

    // 4. niri 订阅线程
    let focus_tx2 = focus_tx.clone();
    thread::spawn(move || {
        let _ = niri_event_loop(focus_tx2);
    });

    // 5. 初始 windows 快照（缓存由事件流维护，这里兜底）
    if let Ok(windows) = niri::query_windows() {
        let _ = focus_tx.send(Ctrl::Windows(windows));
    }

    // 6. 主循环：去抖 + 心跳 + refresh
    let mut pending_focus: Option<u64> = None;
    let mut window_cache: Vec<NiriWindow> = Vec::new();
    let mut debounce_deadline: Option<std::time::Instant> = None;

    loop {
        let wait = match debounce_deadline {
            Some(d) => d.saturating_duration_since(std::time::Instant::now()),
            None => Duration::from_secs(HEARTBEAT_SECS),
        };
        let msg = focus_rx.recv_timeout(wait.max(Duration::from_millis(20)));
        match msg {
            Ok(Ctrl::FocusChanged(id)) => {
                pending_focus = Some(id);
                debounce_deadline = Some(std::time::Instant::now() + Duration::from_millis(DEBOUNCE_MS));
            }
            Ok(Ctrl::Windows(w)) => window_cache = w,
            Ok(Ctrl::Refresh) => {
                // 立即按当前焦点重解析
                if let Some(focus) = current_focus(&window_cache) {
                    handle_focus(&atspi, &sink, &shared, focus)?;
                }
            }
            Ok(Ctrl::Quit) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 心跳
                sink.emit(&BridgeEvent::Heartbeat { ts: unix_now() });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // 去抖到期 → 处理焦点
        if let Some(d) = debounce_deadline {
            if std::time::Instant::now() >= d {
                if let Some(id) = pending_focus.take() {
                    if let Some(focus) = focus_from_cache(&window_cache, id) {
                        handle_focus(&atspi, &sink, &shared, focus)?;
                    }
                }
                debounce_deadline = None;
            }
        }
    }

    http_server.stop();
    Ok(())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn current_focus(cache: &[NiriWindow]) -> Option<FocusInfo> {
    niri::focused_window_from_json(cache).map(focus_of)
}

fn focus_from_cache(cache: &[NiriWindow], id: u64) -> Option<FocusInfo> {
    cache.iter().find(|w| w.id == id).map(focus_of)
}

fn focus_of(w: &NiriWindow) -> FocusInfo {
    FocusInfo { win_id: w.id, app_id: w.app_id.clone(), title: w.title.clone(), pid: w.pid }
}

fn handle_focus(
    atspi: &AtspiClient,
    sink: &stdout_io::StdoutSink,
    shared: &Arc<Mutex<Shared>>,
    focus: FocusInfo,
) -> Result<()> {
    let menu = match resolve_focus(atspi, &focus) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[global-menu-bridge] resolve failed: {e:#}");
            None
        }
    };
    {
        let mut st = shared.lock().unwrap();
        st.session = Some(proxy::Session { focus: focus.clone(), menu: menu.clone() });
    }
    let source = if menu.is_some() { "atspi" } else { "none" };
    sink.emit(&proxy::make_menu_event(&focus, menu, source));
    Ok(())
}

/// 订阅 niri event-stream；本线程内任何错误（进程退出/schema 变更）记录后结束。
fn niri_event_loop(tx: Sender<Ctrl>) -> Result<()> {
    let mut child = Command::new("niri")
        .args(["msg", "--json", "event-stream"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line?;
        match parse_event_line(&line) {
            Ok(NiriEvent::WindowFocusChanged { id }) => {
                let _ = tx.send(Ctrl::FocusChanged(id));
            }
            Ok(NiriEvent::WindowsChanged { windows }) => {
                let _ = tx.send(Ctrl::Windows(windows));
            }
            Ok(_) => {}
            Err(e) => eprintln!("[global-menu-bridge] niri line parse: {e:#}"),
        }
    }
    // 进程退出（niri 重启/socket 断）→ 让主循环退出
    let _ = child.wait();
    let _ = tx.send(Ctrl::Quit);
    Ok(())
}
```

- [ ] **Step 5: 补 proxy.rs 的 Shared/Session 定义（追加到 proxy.rs）**

```rust
use std::sync::{Arc, Mutex};

/// 跨线程共享状态（proxy 主循环写，HTTP 线程读）。
#[derive(Default)]
pub struct Shared {
    pub session: Option<Session>,
}

#[derive(Clone)]
pub struct Session {
    pub focus: FocusInfo,
    pub menu: Option<MenuItem>,
}
```

（同时在 proxy.rs 顶部补 `use crate::protocol::MenuItem;`）

- [ ] **Step 6: 编译 + 单元测试**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -5`
Expected: `test result: ok. 16 passed`（全部累计），无 error。

- [ ] **Step 7: 手动冒烟（无 niri 环境也应有 hello + 心跳）**

Run: `timeout 3 ./target/debug/noctalia-global-menu-bridge 2>/dev/null | head -3`
Expected: 第一行 `{"type":"hello",...}`，随后 `{"type":"heartbeat",...}`（若无 NIRI_SOCKET 则 stderr 有错误但 hello 已出）。

- [ ] **Step 8: Commit**

```bash
git add global-menu/bridge/
git commit -m "feat(global-menu): 桥完整组装（HTTP/stdout/status/事件循环）"
```

---

## Task 8: 真实环境冒烟（验证桥 ↔ GIMP 全链路）

**Files:**
- Create: `global-menu/scripts/smoke.sh`
- Create: `global-menu/scripts/smoke_manual.md`

- [ ] **Step 1: 创建 smoke.sh（独立跑桥 + 断言 stdout）**

```bash
#!/usr/bin/env bash
# 冒烟：在真实 niri 会话内验证桥的 hello/heartbeat/niri 订阅/菜单解析。
# 需要：NIRI_SOCKET 可用（在 niri 会话内执行）、a11y 总线运行、GIMP 已启动。
# 注意：桥的日志走 stderr，stdout 是协议通道，断言分开。
#!/usr/bin/env bash
set -uo pipefail
BIN="$(dirname "$0")/../bridge/target/debug/noctalia-global-menu-bridge"
[ -x "$BIN" ] || { echo "build bridge first: cd bridge && cargo build"; exit 1; }

"$BIN" > /tmp/gm-out.log 2>/tmp/gm-err.log &
BPID=$!
sleep 2
kill $BPID 2>/dev/null; wait $BPID 2>/dev/null

echo "=== stdout (protocol) ==="
head -5 /tmp/gm-out.log
echo "=== stderr (log) ==="
head -5 /tmp/gm-err.log

PASS=0; FAIL=0
grep -q '"type":"hello"' /tmp/gm-out.log && { echo "PASS hello"; PASS=$((PASS+1)); } || { echo "FAIL hello"; FAIL=$((FAIL+1)); }
grep -q '"type":"heartbeat"' /tmp/gm-out.log && { echo "PASS heartbeat"; PASS=$((PASS+1)); } || { echo "FAIL heartbeat"; FAIL=$((FAIL+1)); }
echo "=== $PASS passed, $FAIL failed ==="
[ $FAIL -eq 0 ]
```

- [ ] **Step 2: 手动验证清单（写入 smoke_manual.md）**

```markdown
# 手动冒烟清单（真实桌面）

前置：在 niri 会话内（ssh 或终端模拟器），`export NIRI_SOCKET=/run/user/$(id -u)/niri.wayland-*.sock`（或用 niri 会话内 shell）。

1. 启动 GIMP（GTK3，无需任何配置）
2. `cd global-menu/bridge && cargo build`（或 release）
3. `./target/debug/noctalia-global-menu-bridge > /tmp/gm-out.log 2>/tmp/gm-err.log &`
4. 焦点切到 GIMP 窗口（`niri msg action focus-window --id <gimp窗口id>` 或鼠标点击）
5. `grep '"type":"menu"' /tmp/gm-out.log | tail -1 | head -c 600`
   预期：`"app":{"app_id":"gimp"...},"menu":{"label":"","type":"submenu","children":[{"label":"File"...`
6. 若 menu 为 null：检查 `cat /tmp/gm-err.log`（a11y 连接失败？gimp 未注册？）
7. 测试点击（手动）：`curl -s -X POST -d '{"path":[3]}' http://127.0.0.1:<port>/click`
   （port 从 hello 行取；path [3] = View 菜单——预期触发 GIMP 打开 View 相关动作或不报错）
8. 全链路插件验证：在 Noctalia 中启用插件（见 README），观察菜单栏条 + 点击 Fullscreen
```

- [ ] **Step 3: 执行冒烟（本机真实会话）**

Run: `bash global-menu/scripts/smoke.sh`（需在 niri 会话内；若当前 shell 无 NIRI_SOCKET，先 `export NIRI_SOCKET=$(ls /run/user/1000/niri.wayland-*.sock | head -1)`）
Expected: `PASS hello`、`PASS heartbeat`；若有 niri 会话则 stderr 无连接错误。

- [ ] **Step 4: 手动验证菜单解析（可选，需 GIMP 前台）**

按 smoke_manual.md 第 1-6 步执行，确认 `"menu"` 事件含 GIMP 的 File/Edit/View 顶层项。

- [ ] **Step 5: Commit**

```bash
git add global-menu/scripts/
git commit -m "test(global-menu): 冒烟脚本与手动验证清单"
```

---

## Task 9: Luau service.luau + widget.luau（托管桥 + 菜单栏条）

**Files:**
- Create: `global-menu/service.luau`
- Create: `global-menu/widget.luau`

- [ ] **Step 1: 实现 service.luau**

```lua
--!nonstrict
-- Global Menu bridge host: 托管桥进程、解析 NDJSON 事件、经 state 广播。
-- 桥由本 service 入口独占托管（widget/panel 只读 state）。

local state = noctalia.state
local bridgePath = noctalia.pluginDir() .. "/bridge-bin/noctalia-global-menu-bridge"
local bridgePort = nil
local bridgeStartedAt = 0
local restartDelayUntil = 0
local RESTART_COOLDOWN_MS = 10000

local function nowMs()
  return os.clock() * 1000
end

local function decode(line)
  local ok, v = pcall(noctalia.json.decode, line)
  if ok and type(v) == "table" then return v end
  return nil
end

local function httpGet(port, path, cb)
  noctalia.http({ url = "http://127.0.0.1:" .. tostring(port) .. path, method = "GET" }, cb)
end

local function httpPost(port, path, bodyTable, cb)
  noctalia.http({
    url = "http://127.0.0.1:" .. tostring(port) .. path,
    method = "POST",
    body = noctalia.json.encode(bodyTable),
  }, cb)
end

local function handleLine(line)
  local ev = decode(line)
  if not ev then return end
  if ev.type == "hello" then
    bridgePort = ev.port
    state.set("bridge", noctalia.json.encode({ port = ev.port, alive = true }))
    noctalia.log("GlobalMenu bridge up on port " .. tostring(ev.port))
  elseif ev.type == "menu" then
    state.set("menu", noctalia.json.encode(ev))
  elseif ev.type == "error" then
    noctalia.log("GlobalMenu bridge: " .. tostring(ev.msg))
  end
end

local function startBridge()
  if nowMs() < restartDelayUntil then return end
  local ok = noctalia.runStream(bridgePath, handleLine)
  if ok then
    bridgeStartedAt = nowMs()
  else
    noctalia.log("GlobalMenu: failed to start bridge, will retry")
    restartDelayUntil = nowMs() + RESTART_COOLDOWN_MS
  end
end

local function pingAndRestart()
  if not bridgePort then return end
  httpGet(bridgePort, "/ping", function(ok2)
    if not ok2 then
      -- 桥已死：标记 down 并重启（旧进程若存活由 /shutdown 清场）
      httpPost(bridgePort, "/shutdown", {}, function() end)
      bridgePort = nil
      state.set("bridge", noctalia.json.encode({ port = 0, alive = false }))
      restartDelayUntil = nowMs() + RESTART_COOLDOWN_MS
    end
  end)
end

function update()
  noctalia.setUpdateInterval(5000)
  if bridgePort then
    pingAndRestart()
  else
    startBridge()
  end
end

startBridge()
```

- [ ] **Step 2: 实现 widget.luau**

```lua
--!nonstrict
-- Global Menu 菜单栏条：渲染焦点应用顶层菜单；空时占位。

local state = noctalia.state
local menuData = nil -- {app=..., menu=..., source=...}
local bridgeInfo = nil

state.watch("menu", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  menuData = ok and v or nil
end)

state.watch("bridge", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  bridgeInfo = ok and v or nil
end)

local function httpPost(port, path, bodyTable, cb)
  if not port or port == 0 then return end
  noctalia.http({
    url = "http://127.0.0.1:" .. tostring(port) .. path,
    method = "POST",
    body = noctalia.json.encode(bodyTable),
  }, cb)
end

-- 点击顶层菜单项：拉最新子项 + 打开 popup 面板
local function onTopClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/open", { path = item.path }, function(ok2, status, body)
    if not ok2 then return end
    local ok3, resp = pcall(noctalia.json.decode, body or "{}")
    if not ok3 or not resp or not resp.ok then return end
    state.set("popup", noctalia.json.encode({
      title = item.label,
      parentPath = item.path,
      items = resp.children or {},
    }))
    noctalia.togglePanel("bighu630/global-menu:global_menu_popup")
  end)
end

function render()
  if not menuData or not menuData.menu then
    local appName = menuData and menuData.app and menuData.app.app_id or ""
    if appName == "" then appName = noctalia.tr("global_menu.placeholder") end
    return ui.row({ gap = 6, paddingH = 8, paddingV = 2 }, {
      ui.label({ text = appName, color = "on_surface_variant" }),
    })
  end
  local buttons = {}
  for _, item in ipairs(menuData.menu.children or {}) do
    if item.type == "separator" then
      buttons[#buttons + 1] = ui.separator({ orientation = "vertical" })
    elseif item.type == "submenu" or item.type == "item" then
      buttons[#buttons + 1] = ui.button({
        text = item.label,
        enabled = item.enabled,
        onClick = function() onTopClick(item) end,
      })
    end
  end
  return ui.row({ gap = 2, paddingH = 8, paddingV = 2 }, buttons)
end
```

- [ ] **Step 3: 添加 i18n 占位文案（translations/en.json + zh-CN.json）**

`global-menu/translations/en.json`：

```json
{
  "global_menu.placeholder": "Global Menu",
  "global_menu.no_menu": "No menu"
}
```

`global-menu/translations/zh-CN.json`：

```json
{
  "global_menu.placeholder": "全局菜单",
  "global_menu.no_menu": "无菜单"
}
```

- [ ] **Step 4: 构建桥并安装二进制到插件目录**

Run: `bash global-menu/scripts/build.sh`
Expected: `bridge installed to global-menu/bridge-bin/`

- [ ] **Step 5: 语法检查 Luau（用 Noctalia 插件 lint 或 luau 工具；至少人工检查）**

Run: `grep -n "function render" global-menu/widget.luau global-menu/popup.luau`（确认入口函数存在）
Expected: 两文件各一行（service 无 UI 入口，不检查）。

- [ ] **Step 6: Commit**

```bash
git add global-menu/
git commit -m "feat(global-menu): service.luau 托管桥 + widget.luau 菜单栏条"
```

---

## Task 10: popup.luau（子菜单弹出面板 + 点击回传）

**Files:**
- Create: `global-menu/popup.luau`

- [ ] **Step 1: 实现 popup.luau**

```lua
--!nonstrict
-- Global Menu 子菜单弹出面板：渲染子项、点击回传、Esc 关闭。

local state = noctalia.state
local popupData = nil -- {title=..., parentPath=..., items=...}
local bridgeInfo = nil

state.watch("popup", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  popupData = ok and v or nil
end)

state.watch("bridge", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  bridgeInfo = ok and v or nil
end)

local function httpPost(port, path, bodyTable, cb)
  if not port or port == 0 then return end
  noctalia.http({
    url = "http://127.0.0.1:" .. tostring(port) .. path,
    method = "POST",
    body = noctalia.json.encode(bodyTable),
  }, cb)
end

local function closePopup()
  noctalia.togglePanel("bighu630/global-menu:global_menu_popup")
end

-- 点击叶子项：POST /click {path}; 成功后桥自动重拉 → menu 事件 → 状态刷新
local function onItemClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/click", { path = item.path }, function(ok2, status, body)
    closePopup()
  end)
end

-- 点击带子菜单的项：递归展开（同面板替换）
local function onSubmenuClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/open", { path = item.path }, function(ok2, status, body)
    if not ok2 then return end
    local ok3, resp = pcall(noctalia.json.decode, body or "{}")
    if not ok3 or not resp or not resp.ok then return end
    state.set("popup", noctalia.json.encode({
      title = item.label,
      parentPath = item.path,
      items = resp.children or {},
    }))
  end)
end

function render()
  if not popupData then
    return ui.column({ padding = 8 }, {
      ui.label({ text = noctalia.tr("global_menu.no_menu"), color = "on_surface_variant" }),
    })
  end
  local rows = {}
  for _, item in ipairs(popupData.items or {}) do
    if item.type == "separator" then
      rows[#rows + 1] = ui.separator({})
    else
      local label = item.label
      if item.type == "checkbox" or item.type == "radio" then
        local mark = item.checked and "✓ " or "   "
        label = mark .. label
      end
      local hasChildren = item.type == "submenu" or (item.children and #item.children > 0)
      if hasChildren then
        label = label .. "  ▸"
      end
      rows[#rows + 1] = ui.button({
        text = label,
        enabled = item.enabled,
        onClick = function()
          if hasChildren then
            onSubmenuClick(item)
          else
            onItemClick(item)
          end
        end,
      })
    end
  end
  return ui.column({ gap = 2, paddingH = 6, paddingV = 6 }, rows)
end
```

- [ ] **Step 2: 人工检查 + 语法**

Run: `grep -n "function render" global-menu/popup.luau`
Expected: 一行。

- [ ] **Step 3: Commit**

```bash
git add global-menu/
git commit -m "feat(global-menu): popup.luau 子菜单面板与点击回传"
```

---

## Task 11: 端到端验证 + 打包

**Files:**
- Create: `global-menu/README.md`
- Modify: `catalog.toml`、`registry.json`（插件目录条目，格式参考现有条目）

- [ ] **Step 1: 端到端手动验证（真实桌面，按 smoke_manual.md 第 8 步）**

- Noctalia 重新加载插件（Noctalia 设置里启用或重启 Noctalia）
- 启动 GIMP → 焦点切到 GIMP → 菜单栏条出现 File/Edit/View/...
- 点击 File → 弹出面板显示 New/Open/... → 点击一项 → 应用执行
- 点击 View → Show All 勾选状态同步
- 杀掉桥进程（`pkill -f noctalia-global-menu-bridge`）→ ≤10s 后插件自动重启桥并恢复菜单
- Noctalia reload 后插件自愈

- [ ] **Step 2: 写 README.md**

```markdown
# Noctalia Global Menu

macOS 风格全局菜单：焦点应用（GTK3）的菜单栏显示在 Noctalia 顶栏，点击展开子菜单、点击项触发应用真实动作。

## 架构

- `bridge/` — Rust 桥接程序：niri IPC 焦点跟踪 + AT-SPI 菜单读取 + 本地 HTTP 命令服务（设计文档：`docs/superpowers/specs/2026-08-01-global-menu-bridge-design.md`）
- `service.luau` — 托管桥进程，NDJSON 事件 → `noctalia.state` 广播
- `widget.luau` — 菜单栏条
- `popup.luau` — 子菜单弹出面板

## 依赖

- niri（焦点事件来源）
- at-spi2-core（a11y 总线；GTK3 应用自动接入，无需配置）
- Rust ≥ 1.81（构建桥）
- GTK3 应用（如 GIMP）——Qt6/Chromium/Electron 本机暂不支持（见设计文档 §9）

## 构建与安装

```bash
bash global-menu/scripts/build.sh   # cargo build --release → global-menu/bridge-bin/
```

然后在 Noctalia 设置中启用 "Global Menu" 插件（或重启 Noctalia）。

## 使用

启动任意 GTK3 应用并聚焦 → 菜单栏出现在顶栏。点击顶层项展开子菜单，点击项执行。

## 故障排查

- 菜单不出现：`systemctl --user status at-spi-dbus-bus.service`（a11y 总线须运行）；
  确认应用是 GTK3（`ldd $(which gimp) | grep gtk-3`）
- 桥日志：插件日志（Noctalia 日志）或手动运行
  `NIRI_SOCKET=/run/user/1000/niri.wayland-*.sock ./bridge/target/release/noctalia-global-menu-bridge`
```

- [ ] **Step 3: 更新 catalog.toml（追加条目，保持格式）**

在 `[[plugin]]` 列表追加：

```toml
[[plugin]]
id = "bighu630/global-menu"
name = "Global Menu"
version = "0.1.0"
updated_at = <当前 unix 时间戳>
added_at = <当前 unix 时间戳>
author = "Bighu630"
license = "MIT"
description = "macOS-style global menu for GTK3 apps on niri via an AT-SPI bridge."
plugin_api = 14
tags = ["bar", "utility"]
```

- [ ] **Step 4: 更新 registry.json（追加条目）**

```json
{
  "id": "bighu630/global-menu",
  "name": "Global Menu",
  "version": "0.1.0",
  "official": false,
  "author": "Bighu630",
  "description": "macOS-style global menu for GTK3 apps on niri via an AT-SPI bridge.",
  "repository": "https://github.com/bighu630/noctalia_plugins",
  "license": "MIT",
  "plugin_api": 14,
  "tags": ["Bar", "Utility"],
  "lastUpdated": "2026-08-01T00:00:00+08:00"
}
```

- [ ] **Step 5: 最终全量测试**

Run: `cd global-menu/bridge && cargo test 2>&1 | tail -3`
Expected: 全部 passed，无警告（可允许 dead_code 警告，不阻塞）。

- [ ] **Step 6: Commit**

```bash
git add global-menu/ catalog.toml registry.json
git commit -m "feat(global-menu): 打包（README + catalog/registry 条目）"
```

---

## 收尾

- 合并 worktree 分支到 main（finishing-a-development-branch 技能）
- 更新 `docs/global-menu-research.md`：修正"niri 支持 org_kde_kwin_appmenu"（实证：niri 26.04 不支持），补充 AT-SPI 实证结论
