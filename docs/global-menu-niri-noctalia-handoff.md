# 全局菜单（Global Menu）niri + Noctalia 原生实现——交接文档

> 交接日期：2026-08-03
> 背景：macOS 风格全局菜单。原方案（Luau 插件 + Rust 桥）已跑通（GTK/Qt/Electron 菜单显示+点击），
> 现改为**参考 Plasma 6 架构的完全原生方案**：niri（compositor）收集菜单地址，Noctalia（shell）原生拉取渲染。

## 1. Git 目录

| 项目 | 位置 | 状态 |
|---|---|---|
| **niri** | `/data/code/c++/niri` | **已含全部 appmenu 改动**（从 /tmp/niri-latest 复制，含 release 构建产物 target/） |
| **noctalia** | `/data/code/c++/noctalia` | **fresh clone，零改动**（原方案改 /tmp/noc-src，未迁移） |
| 插件仓库（原方案，作废但可参考） | `/data/code/c++/noctalia_plugins` | Rust 桥 + Luau 插件（功能验证过） |
| KDE 调研报告 | `/data/code/c++/noctalia_plugins/kde-global-menu/` | 3 份：Plasma6 架构 / KWin 协议实现 / Plasma6 变化与问题 |
| Naxdy PR #46 参考 diff | `/tmp/naxdy-appmenu.diff` | 505 行完整实现（niri 旧版） |

## 2. 目标架构（对齐 Plasma 6）

```
Qt ≥6.10 / Chromium 137+ / Firefox 138+（Wayland）
  └─ org_kde_kwin_appmenu 协议：set_address(service, path) attach 到 wl_surface
        ↓
niri（compositor）——已实现 ✅
  ├─ 协议收集每窗口的 (service, path)
  ├─ com.canonical.AppMenu.Registrar.GetMenuForWindow(niri窗口id) 查询接口
  └─ niri msg --json windows → 每窗口带 appmenu_service/appmenu_object_path 字段
        ↓
Noctalia（shell，原生 C++）——待实现
  ├─ NiriWorkspaceBackend 解析 windows JSON（含新字段）→ 焦点窗口的菜单地址
  ├─ DBusMenu 客户端（sdbus-c++）：GetLayout/AboutToShow/Event/LayoutUpdated
  ├─ AT-SPI 客户端（GTK 应用）：菜单树 + DoAction 点击
  ├─ bar 原生 widget（菜单栏条）
  └─ ContextMenuPopup（子菜单弹出）
```

- **不依赖 Luau 插件、不需要 Rust 桥进程**（插件仓库的方案退役）
- 点击：DBusMenu `Event(id,"clicked")`（Qt/Chromium 官方支持）/ AT-SPI `DoAction`（GTK）
- GTK 归属待定（见 §5）

## 3. niri 已写代码（全部已编译通过；最后增量构建被中断需重跑验证）

参考：Naxdy/niri PR #46（https://github.com/Naxdy/niri/pull/46），适配 niri 26.4 API（smithay git、zbus 5、async-channel 替代 tokio）。

| 文件 | 改动 |
|---|---|
| `Cargo.toml` | + `wayland-protocols-plasma = { version = "0.3.12", features = ["server"] }` |
| `src/protocols/kde_appmenu.rs`（新，~150 行） | `org_kde_kwin_appmenu_manager` v2 协议：`create`+`set_address` → `AppmenuPath{service_name, path}`；`OrgKdeKwinAppmenuManagerHandler` trait + `delegate_org_kde_kwin_appmenu!` 宏 |
| `src/dbus/canonical_dbusmenu.rs`（新，~60 行） | 拥有 `com.canonical.AppMenu.Registrar`；`GetMenuForWindow(window_id)` 按 **niri 窗口 id** 查询（async_channel 回传）；RegisterWindow/UnregisterWindow 返回"xwayland not supported" |
| `src/dbus/mod.rs` | + 模块声明 + `DBusServers.conn_canonical_dbusmenu` + start() 分支（calloop channel → `layout.windows().find(id)` → `get_appmenu()`） |
| `src/handlers/mod.rs` | `impl OrgKdeKwinAppmenuManagerHandler for State`（unmapped_windows / layout.find_window_and_output_mut 存地址）+ delegate |
| `src/handlers/compositor.rs` | `Unmapped` 解构加 `appmenu`；`Mapped::new(..., appmenu)` |
| `src/window/mapped.rs` | + `appmenu: Option<AppmenuPath>` 字段 + `set_appmenu/get_appmenu`（`#[cfg(feature = "dbus")]`） |
| `src/window/unmapped.rs` | + `appmenu` 字段（默认 None） |
| `src/niri.rs` | + `org_kde_kwin_appmenu_manager_state` 字段/初始化/构造列表 |
| `src/protocols/mod.rs` | + `pub mod kde_appmenu`（dbus feature） |
| `niri-ipc/src/lib.rs` | `Window` + `appmenu_service`/`appmenu_object_path`（Option，skip_serializing_if none） |
| `src/ipc/server.rs` | `make_ipc_window` 填两个新字段（cfg dbus；WindowsChanged 事件与 windows 响应都复用此函数，字段自动带上） |

**待办**：`cargo build --release` 重跑确认（上次被中断，之前完整构建 3m43s 通过）；安装替换 `/usr/bin/niri` + 注销重登（用户操作）。

## 4. Noctalia 待实现（大工程，未开始）

位置：`/data/code/c++/noctalia`（fresh clone）。依赖现状：sdbus-c++（meson 已依赖）、nlohmann-json、ContextMenuPopup 原生控件（`src/ui/controls/context_menu_popup.*`）齐备。

### 4.1 地址接入（~30 行）
- `src/compositors/niri/niri_workspace_backend.cpp`：`updateFocusedWindowIdFromWindowsJson` 解析 windows JSON 处，顺带解析 `appmenu_service`/`appmenu_object_path` 存入 `WindowState`（新增字段）
- 焦点跟踪已有：`focusedWindowId()`（`m_focusedWindowId`）

### 4.2 DBusMenu 客户端（新模块 `src/shell/global_menu/dbusmenu_client.*`，~400 行）
参考：插件桥的 `global-menu/bridge/src/dbusmenu.rs`（Rust 已验证的生产逻辑）：
- `GetLayout(0,-1,[])` 全量拉树（顶层下发；子菜单懒加载：`AboutToShow(id)` + `GetLayout(id,-1,[])`——**Qt/Chromium 子菜单必须这么拉**）
- `Event(id,"clicked",...)` 点击
- `LayoutUpdated`/`ItemsPropertiesUpdated` 信号订阅（sdbus-c++ signal）
- 菜单模型：label/type/enabled/visible/checked/toggle + children；**助记符 `_` 处理**

### 4.3 AT-SPI 客户端（GTK 应用，`atspi_client.*`，~400 行）
参考：`global-menu/bridge/src/atspi.rs`。关键经验：
- 角色 wire 值：MENU_BAR=34、MENU=33、POPUP_MENU=41、MENU_ITEM=35、SEPARATOR=50、CHECK_MENU_ITEM=8、RADIO_MENU_ITEM=45、FRAME=23
- 菜单结构：MENU_ITEM → 无名弹出壳（GTK 用 role 33，Qt 用 role 41）→ 实际子项；**读子菜单必须展开壳**
- 本机 at-spi2 桥缺失方法：用 Properties.Get 读 ChildCount/Name/NActions 兜底；GetState 返回 `au`（双 u32 位词）
- **Qt 菜单项 DoAction 不可用**（qtatspi 对象瞬态、无 Action 接口）→ **Qt 必须走 DBusMenu**（niri 协议路径）
- **前置**：`org.a11y.Bus` 服务的 `org.a11y.Status.IsEnabled=true`（readwrite 属性，Set 即可）——否则 Qt/Chromium 不注册 a11y（GNOME 由 settings-daemon 设，niri 会话需自行设置）
- 启动时注册事件监听（RegisterEvent）可帮助对象稳定（可选）

### 4.4 管理器（`global_menu_manager.*`，~200 行）
- 焦点变化（workspace backend 信号）→ 取地址/AT-SPI → 解析菜单 → 缓存（按窗口 id）
- 点击路由：DBusMenu Event 或 AT-SPI DoAction（点击时重新解析，不缓存路径）
- 菜单变化刷新（LayoutUpdated / 焦点切换）

### 4.5 bar widget + 弹出菜单（~300 行）
- `src/shell/bar/widgets/global_menu_widget.*`（参考其他 bar widget 的注册方式——`widget_factory.cpp`）
- 菜单栏条渲染（按钮行）+ 子菜单用 ContextMenuPopup（`src/ui/controls/context_menu_popup.*`，已有 C++ 控件——检查是否可直接用/需扩展）
- 锚定：点击位置（参考已合入的 noctalia togglePanel 锚点机制——`docs/noctalia-togglepanel-anchor.patch` 已实现 bar 点击位置记录）

## 5. 待决策：GTK 应用归属

GIMP 等 GTK3 应用 Wayland 下只有 AT-SPI（gtk-shell 私有协议 KWin/niri 都不实现）：
- **A**：GTK 跑 XWayland + appmenu-gtk-module X11 属性 → niri 读 X11 属性（KWin `x11window.cpp` 做法，~200 行）→ 统一 DBusMenu
- **B**：Noctalia 原生实现 AT-SPI 客户端（GTK 保持 Wayland 原生）——**推荐**（体验好，AT-SPI 逻辑已在插件桥验证）
- **C**：暂不支持 GTK

## 6. 关键经验速查（来自插件方案的实际调试，省大量弯路）

1. **Qt Wayland 应用**（Konsole/Dolphin/kwrite）：QDBusMenuBar 导出 + niri 协议 attach（Qt ≥6.10 自动，本机 6.11 ✅）；**点击必须走 DBusMenu Event**（AT-SPI 对 Qt 无效）
2. **Chromium/Electron**：137+ 支持 appmenu 协议（Wayland 模式）；Electron 老版本（Typora = Chromium 91）**只有 X11 注册 Registrar** 一条路
3. **GTK3**：AT-SPI 全通（菜单 eager、DoAction 有效）；**注意** appmenu-gtk-module 在 Wayland 下不注册（gtk-shell 被忽略）
4. **niri IPC**：event-stream 是外部标签 JSON（`{"WindowFocusChanged":{"id":30}}`）；多订阅者**单播**（调试时勿留多余连接）
5. **niri 对 XWayland 窗口的 pid 不可靠**（xwayland-satellite 场景报卫星 pid）——按 niri 窗口 id 查询（新方案天然规避）
6. **at-spi-bus-launcher 重启**会把 IsEnabled 重置为 false → 桥/Noctalia 启动时要 Set 回 true
7. 插件时代教训（原生实现已规避）：runStream 64KB 单行限制、Lua 词法作用域、state watch 时序

## 7. 参考文件清单

| 文件 | 用途 |
|---|---|
| `/data/code/c++/noctalia_plugins/kde-global-menu/plasma-workspace-appmenu-research.md` | Plasma6 完整架构（kded 模块/applet/KDBusMenuImporter/gmenu-proxy） |
| `/data/code/c++/noctalia_plugins/kde-global-menu/kwin-global-menu-protocol-implementation.md` | KWin 协议实现细节（niri 实现的直接参考） |
| `/data/code/c++/noctalia_plugins/kde-global-menu/plasma6-vs-plasma5-global-menu-changes-and-known-issues.md` | Qt 版本时间线、已知问题 |
| `/tmp/naxdy-appmenu.diff` | Naxdy PR #46 完整 diff（niri 旧版参考） |
| `/data/code/c++/noctalia_plugins/global-menu/bridge/src/dbusmenu.rs` | 已验证的 DBusMenu 客户端逻辑（Rust → 移植 C++） |
| `/data/code/c++/noctalia_plugins/global-menu/bridge/src/atspi.rs` | 已验证的 AT-SPI 客户端逻辑（Rust → 移植 C++） |
| `/data/code/c++/noctalia_plugins/docs/noctalia-togglepanel-anchor.patch` | Noctalia bar 点击位置→面板锚点（已构建进用户 Noctalia，全局菜单原生 widget 可直接用） |
| KWin 源码（在线） | `src/wayland/appmenu.cpp`、`src/x11window.cpp`（X11 属性方案 A 参考） |
