# 开源全局菜单（Global Menu）实现调研报告

> 目的：为 Noctalia shell 全局菜单插件提供架构参考。
> 调研时间：2026-08（基于 2026 年可获取的源码/文档快照）。

---

## 0. 背景：全局菜单的标准架构范式

所有成熟的全局菜单实现都遵循同一个三层模型（Unity/Ayatana 时代确立的 `com.canonical` 系列 D-Bus 协议）：

```
┌─────────────┐  RegisterWindow(winId, menuPath)   ┌──────────────────┐
│ 应用 (客户端) │ ─────────────────────────────────▶ │  Registrar 服务    │
│ 导出 DBusMenu │                                   │ winId→(bus,path) │
└──────┬──────┘                                   └────────┬─────────┘
       │  com.canonical.dbusmenu 接口                      │ GetMenuForWindow
       │  (GetLayout/AboutToShow/Event + 信号)              ▼
       │                                   ┌─────────────────────────────┐
       └───────────────────────────────────│ 显示端 (面板 applet/插件)      │
                                           │ 监听窗口切换 → 订阅活动窗口菜单  │
                                           └─────────────────────────────┘
```

- **客户端（应用侧）**：GTK 用 `unity-gtk-module`/`appmenu-gtk-module`（GtkMenuShell → DBusMenu 导出），Qt 用 `appmenu-qt`/Qt ≥5.7 内置的 `QDBusMenuBar`（qtbase `src/gui/platform/unix/dbusmenu/`），Java 用 jayatana。
- **Registrar**：提供 `com.canonical.AppMenu.Registrar` 接口（`/com/canonical/AppMenu/Registrar`），方法 `RegisterWindow` / `UnregisterWindow` / `GetMenuForWindow`，维护 winId ↔ (D-Bus service, object path) 映射。
- **显示端**：监听窗口管理器/桌面的活动窗口变化 → 查 Registrar 拿到菜单地址 → 作为 `com.canonical.dbusmenu` 的客户端（Importer）订阅菜单 → 渲染菜单栏。

DBusMenu 核心方法/信号（各项目通用的机制）：

| 方向 | 成员 | 用途 |
|---|---|---|
| 方法 | `GetLayout(parentId, recursionDepth, propertyNames)` | 拉取菜单树布局 |
| 方法 | `AboutToShow(id)` | 弹出子菜单前询问应用刷新 |
| 方法 | `Event(id, eventId, data, timestamp)` | 发送交互事件（`clicked`/`opened`/`closed`/`hovered`） |
| 方法 | `GetProperty` / `GetGroupProperties` | 查询单项/批量属性 |
| 信号 | `LayoutUpdated(revision, parent)` | 布局变化，需重新 GetLayout |
| 信号 | `ItemsPropertiesUpdated(updated, removed)` | 局部属性增量更新 |
| 信号 | `ItemActivationRequested(id, timestamp)` | 应用内快捷键触发菜单项，通知显示端弹出对应子菜单 |

参考综述：[Linux 桌面应用全局菜單实现](https://64mb.org/2022/04/05/linux-desktop-global-menu/)（中文，含完整工作流程与 dbus-monitor 调试示例）、[helloSystem 文档](https://hellosystem.github.io/docs/developer/menu.html)。

---

## 1. KDE Plasma 全局菜单（appmenu）

**仓库**：[KDE/plasma-workspace](https://github.com/KDE/plasma-workspace/tree/master/appmenu)（服务端 KDED 模块 `appmenu/`）、[applet `applets/appmenu/`](https://github.com/KDE/plasma-workspace/tree/master/applets/appmenu)（显示端）、[`libdbusmenuqt/`](https://phabricator.kde.org/source/plasma-workspace/browse/master/libdbusmenuqt)（DBusMenu Qt 客户端库）。

**技术栈**：C++ / Qt6 / QML（显示端）；Qt Wayland 客户端私有 API（服务端）。

### 1.1 服务端（KDED 模块 `appmenu/`）— 连接应用与 WM

- `appmenu.cpp`（`AppMenuModule`）：仅在显示端服务 `org.kde.kappmenuview` 注册后才激活 `MenuImporter`（QDBusServiceWatcher 监听注册/注销）。
- `menuimporter.cpp`（`MenuImporter`）：实现 `com.canonical.AppMenu.Registrar`（注册 `RegisterWindow`/`UnregisterWindow` 等），收到注册后：
  - 过滤弹窗类型（X11 上跳过 `NET::Menu`/`DropdownMenu`/`PopupMenu` 类型窗口）；
  - **X11**：把 `serviceName` 和 `menuObjectPath` 写入窗口 X11 属性 `_KDE_NET_WM_APPMENU_SERVICE_NAME` / `_KDE_NET_WM_APPMENU_OBJECT_PATH`（KWin/任务管理器据此把菜单关联到窗口）；
  - 维护 winId→(service,path) 映射，`QDBusServiceWatcher` 跟踪菜单服务退出。
- `appmenu_dbus.cpp`（`AppmenuDBus`）：注册 `org.kde.kappmenu`（路径 `/KAppMenu`），转发 `showRequest`/`menuHidden`/`menuShown` 信号给 KWin。
- **KWin 侧**（[kwin-x11 5.0/appmenu.cpp](https://github.com/KDE/kwin-x11/blob/Plasma/5.0/appmenu.cpp)，现并入 KWin）：`ApplicationMenu` 单例连接 `org.kde.kappmenu`，收到 `showRequest(service, path, actionId)` 后在窗口位置弹出菜单。
- `ItemActivationRequested`（应用内按快捷键激活菜单项）→ `showRequest` → KWin 弹出 `VerticalMenu`（[verticalmenu.cpp](https://github.com/KDE/plasma-workspace/blob/master/appmenu/verticalmenu.cpp)，基于 QMenu，Wayland 下以不可见 ToplevelWindow + xdg_popup 弹出）。
- **Wayland 菜单定位协议**：`org_kde_kwin_appmenu_manager`（[协议定义](https://wayland.app/protocols/kde-appmenu)）允许客户端把 `com.canonical.dbusmenu` 的 D-Bus 地址 attach 到 wl_surface。KWin 正在将该私有协议移植到标准 `xdg-dbus-annotation`（[KWin MR !5064](https://invent.kde.org/plasma/kwin/-/merge_requests/5064)）；Firefox 已从 dbus-annotation 切换到 kde-appmenu 协议（[Bug 1956707](https://bugzilla.mozilla.org/show_bug.cgi?id=1956707)）。

### 1.2 显示端（applet `applets/appmenu/`）— 订阅与渲染

- **活动窗口跟踪**：`AppMenuModel`（QAbstractListModel）直接用 KWin 的 `TaskManager::TasksModel`，监听 `activeTaskChanged` 及 `dataChanged`（ApplicationMenuObjectPath / ApplicationMenuServiceName roles）——**不与窗口管理器直接交互，而是通过 KWin 的窗口数据模型获取活动窗口的菜单地址**。同时监听 virtualDesktopChanged / activityChanged / screenGeometryChanged。
- **DBusMenu 客户端**：`KDBusMenuImporter : DBusMenuImporter`（libdbusmenuqt），关键实现（[dbusmenuimporter.cpp](https://phabricator.kde.org/source/plasma-workspace/browse/master/libdbusmenuqt/dbusmenuimporter.cpp)）：
  - 初始：`GetLayout(0, 1, [])` 异步拉取顶层；每个子菜单在弹出前才 `updateMenu(menu)`（懒加载）；
  - 弹出前：`AboutToShow(id)` + 发送 `opened` 事件（注释明确说明："Firefox deliberately ignores aboutToShow whereas Qt ignores opened, so we'll just send both"）；
  - 更新：`LayoutUpdated` 信号 → 去抖定时器合并 → 重新 GetLayout；`ItemsPropertiesUpdated/removed` → 局部增量更新；
  - 点击：QAction::triggered → `Event(id, "clicked", ...)`；菜单 aboutToHide → 发 `closed`；
  - 应用请求：`ItemActivationRequested` → 查找对应 action → 请求显示端激活；
  - 服务崩溃：`QDBusServiceWatcher::serviceUnregistered` → 关闭菜单、标记不可用。
- **渲染**：QML。`main.qml` 中 Repeater + `MenuDelegate.qml`（AbstractButton，KSvg FrameSvgItem 背景，Kirigami 助记符下划线在按 Alt 时显示）。点击按钮 → `Plasmoid.trigger(index)` → C++ `AppMenuApplet::trigger()` 弹出原生 QMenu（importer 构建的 QAction 树），hover 时通过 `requestActivateIndex` 切换子菜单（QML 端过滤 QEvent::MouseMove）。
- **键盘导航**：Alt 键助记符 + 搜索菜单（Wayland 下提供"Search"动作，QLineEdit 过滤扁平化 action 列表）。

**参考来源**：
- https://github.com/KDE/plasma-workspace/tree/master/appmenu
- https://lxr.kde.org/source/plasma/plasma-workspace/applets/appmenu/plugin/appmenumodel.h
- https://blog.broulik.de/2016/10/global-menus-returning/（Kai Uwe Broulik：Qt 5.7 内置 DBusMenu 导出 + KDE 5.9 全局菜单回归）
- https://blog.broulik.de/2018/03/gtk-global-menu/（gmenu-dbusmenu-proxy：GMenu ↔ DBusMenu 代理，让 GTK 应用进 KDE 全局菜单）
- https://codebrowser.dev/qt6/qtbase/src/gui/platform/unix/dbusmenu/qdbusmenubar.cpp.html（Qt 客户端导出实现）

---

## 2. vala-panel-appmenu

**仓库**：[vala-panel-project/vala-panel-appmenu](https://gitlab.com/vala-panel-project/vala-panel-appmenu)（GitHub 镜像：[rilian-la-te/vala-panel-appmenu](https://github.com/rilian-la-te/vala-panel-appmenu)）

**技术栈**：Vala + GTK3 + Meson；依赖 GLib≥2.50、GTK≥3.22、libwnck≥3.4、libdbusmenu-glib（DBusMenu Importer 绑定）。支持 Vala Panel / XFCE / MATE / Budgie 四种面板插件（`applets/` 下各自一个 .vala 文件）。

### 2.1 组件结构（lib/）

| 文件 | 职责 |
|---|---|
| `registrar.vala` | 实现 `com.canonical.AppMenu.Registrar` 服务（独立可执行 `appmenu-registrar`，GApplication service 模式，注册 `com.canonical.AppMenu.Registrar` 总线名） |
| `appmenu-wnck.vala` | 基于 libwnck 跟踪活动窗口（wnck screen 的 active_window_changed 等信号），拿 winId 查 Registrar 得到菜单地址 |
| `helper-dbusmenu.vala` | `DBusMenu.Importer`（libdbusmenu-glib）：`notify::model` 回调中 `w.insert_action_group("dbusmenu", importer.action_group)` + `w.set_menubar(importer.model)` —— **把 DBusMenu 树转换为 GMenuModel + GActionGroup，直接喂给 GTK 菜单部件** |
| `helper-menumodel.vala` | 支持 GTK 原生 `org.gtk.Menus` 协议（GNOME 系应用） |
| `helper-desktop.vala` | 无菜单应用时显示 .desktop 文件动作的回退菜单 |
| `menu-widget.vala` | GtkMenuBar（经典模式）渲染与交互（悬停打开、滚动等） |
| `matcher.c` | 窗口匹配（WM_CLASS/桌面文件关联，pid 查找等） |
| `launcher.c` | 通过 GDesktopAppInfo 启动应用 |

### 2.2 事件/交互机制

- 菜单点击：GTK ActionGroup 的 `activate` 信号 → DBusMenu 客户端库发送 `Event(id, "clicked")`。
- 动态更新：libdbusmenu-glib 的 Importer 内部订阅 `LayoutUpdated`/`ItemsPropertiesUpdated` 信号并自动刷新 model。
- GTK 应用菜单导出由 `unity-gtk-module` / `appmenu-gtk-module`（仓库 subprojects 内）完成；Qt 应用用 appmenu-qt / Qt 5.7+ 内置支持；Java 用 jayatana（实验性）。
- 隐藏应用内菜单栏：设置 GSettings `Gtk/ShellShowsMenubar`、`Gtk/ShellShowsAppmenu`（XFCE 用 xfconf，MATE 用 org.mate.interface，Budgie 用 gsettings overrides）。

**参考来源**：
- https://gitlab.com/vala-panel-project/vala-panel-appmenu
- https://github.com/rilian-la-te/vala-panel-appmenu（源码树结构见上）
- https://packages.ubuntu.com/en/resolute/mate-applet-appmenu（打包依赖：appmenu-registrar、libwnck、libmate-panel-applet）

---

## 3. UKUI globalmenu 组件

**仓库**：[ukui/ukui-kwin](https://github.com/ukui/ukui-kwin)（appmenu.cpp 为 KWin fork 遗留）、[ukui/ukui-panel](https://github.com/ukui/ukui-panel)、[ukui/ukui-menu](https://github.com/ukui/ukui-menu)（开始菜单，易混淆）

**技术栈**：C++ / Qt5（KWin fork 的 ApplicationMenu 部分）。

### 3.1 实际状况（重要结论）

- UKUI 的"全局菜单"基础设施 = **KWin 窗口管理器的 `ApplicationMenu` 单例**（fork 自 kwin-x11，见 [ukui-kwin/appmenu.cpp](https://github.com/ukui/ukui-kwin/blob/master/appmenu.cpp)）：连接 `org.ukui.kappmenu`（路径 `/KAppMenu`）总线接口，处理 `showRequest`/`menuAvailable`/`menuHidden` 信号，在窗口处弹出菜单。
- **UKUI 没有自己的全局菜单显示端**：ukui-panel 的插件列表只有 startmenu / quicklaunch / taskbar / tray / calendar / nightmode / showdesktop（[ukui-panel README](https://github.com/ukui/ukui-panel)），**无 appmenu 插件**。
- `ukui-menu` 是**开始菜单（应用启动器）**，不是全局菜单；UKUI 4.x 用 QML 重写并引入 QtPlugin 插件机制（`MenuExtensionPlugin`，见 [UKUI 开始菜单插件开发指南](https://www.ukui.org/news/137-cn.html)），与全局菜单无关。
- 结论：UKUI 保留了 KWin 的菜单弹出基础设施（面向有全局菜单显示端的场景，如配合 KDE appmenu applet 或第三方实现），自身未落地完整全局菜单。对 Noctalia 的参考价值有限，仅证明"KWin fork 保留 appmenu 基础设施"这一迁移路径。

**参考来源**：
- https://github.com/ukui/ukui-kwin/blob/master/appmenu.cpp
- https://github.com/ukui/ukui-panel
- https://gitee.com/openkylin/ukui-panel（UKUI 4.x panel，基于 ukui-quick-framework 插件机制，含顶栏 org.ukui.panelTopBar）

---

## 4. MATE 全局菜单

**状况**：MATE 官方无内置全局菜单，社区方案为主：

1. **mate-applet-appmenu**（当前主流，即 vala-panel-appmenu 的 MATE 插件）：基于 Unity 协议与库（DBusMenu），打包依赖 `appmenu-registrar`、libgtk-3、libmate-panel-applet、libwnck（[Debian 包页](https://packages.debian.org/sid/x11/mate-applet-appmenu)）。MATE 需设置 `org.mate.interface gtk-shell-shows-menubar true` 隐藏窗口内菜单栏。
2. **tamer-hassan/mate-globalmenu**（历史）：gnome2-globalmenu 的 fork（X11 applet 时代，已过时）：https://github.com/tamer-hassan/mate-globalmenu
3. **KeremSoke/mate-appmenu**（2025-11 新 fork，含 HUD）：https://github.com/KeremSoke/mate-appmenu

架构与 vala-panel-appmenu 相同（见第 2 节），不再赘述。

---

## 5. GNOME 生态实现（扩展类）

### 5.1 Fildem（活跃维护，GNOME 45–50）

**仓库**：[gonzaarcr/Fildem](https://github.com/gonzaarcr/Fildem)（原版）、[InledGroup/Fildem](https://github.com/InledGroup/Fildem)（2025-2026 维护版，ESM 迁移 + D-Bus 更新）

**架构**：GJS GNOME Shell 扩展（面板 UI）+ **Python companion 守护进程**（菜单提取 + HUD）：
- Python 侧（`fildem/appmenu.py`、`fildem/menu_model/`）：通过 **Bamf**（`org.ayatana.bamf.matcher`，ActiveWindowChanged 信号）监听活动窗口；用 libdbusmenu 读取 DBusMenu；构建自己的 MenuModel 树；
- 与扩展的通信：自定义 D-Bus（companion 服务）；
- 扩展侧：把菜单渲染为面板上的按钮（GJS St/PopupMenu），点击回传 companion 发送 `Event("clicked")`；
- GTK 菜单导出仍依赖 `appmenu-gtk2-module` / `appmenu-gtk3-module`（`~/.gtkrc-2.0`、`settings.ini` 配置）；
- HUD：rofi 风格的菜单项模糊搜索（`fildem/handlers/rofi.py`、`fildem/utils/fuzzy.py`），X11 用 keybinder 全局快捷键，Wayland 下由用户配置自定义快捷键执行 `fildem-hud`。

### 5.2 Gnome-Global-AppMenu（已停止开发）

**仓库**：[lestcape/Gnome-Global-AppMenu](https://gitlab.com/lestcape/Gnome-Global-AppMenu)（[停止公告 issue #116](https://gitlab.com/lestcape/Gnome-Global-AppMenu/-/issues/116)）

基于 GNOME Shell patch（Giovanni Campagna 的 [bugzilla 652122](https://bugzilla.gnome.org/show_bug.cgi?id=652122)）+ AppIndicator 扩展同源思路。**GTK4 不再支持加载外置 GTK 模块，导致 GTK4 应用无法导出菜单**，是停止开发的主因（见 [64mb.org 综述](https://64mb.org/2022/04/05/linux-desktop-global-menu/)）。只能读取标准 DBusMenu 结构，无法修补不导出菜单的应用（[shemgp 镜像 README](https://github.com/shemgp/Gnome-Global-AppMenu)）。

### 5.3 AppMenu（ChathurangaBW，GNOME 45–50，2026 年新项目）

**仓库**：https://github.com/ChathurangaBW/AppMenu

**亮点**：纯 GJS **零依赖、无外部守护进程**的 macOS 风格全局菜单栏。
**混合策略（对本项目最有借鉴价值）**：
1. 应用导出 Canonical dbusmenu → 直接读取并触发真实菜单项；
2. 现代 GTK/libadwaita 应用暴露 `org.gtk.Actions` → 构建原生 action 菜单；
3. 两者皆无 → **合成动作回退**：用跨应用通用动作/快捷键（通过 GNOME Shell 虚拟键盘事件模拟按键，避免 X11 专属的菜单抓取）。

### 5.4 gnome-shell-extension-appindicator（GJS DBusMenu 参考实现）

**仓库**：https://github.com/ubuntu/gnome-shell-extension-appindicator（[Menu System 文档](https://deepwiki.com/ubuntu/gnome-shell-extension-appindicator/6-menu-system)）

GJS 中最完整的 DBusMenu 实现参考：`DBusClient`（Gio.DBusProxy 子类，异步 GetLayout）、`DbusMenuItem` 树、PopupMenu 渲染；关键细节：**菜单关闭时不更新菜单项**（[commit f57dbd5](https://github.com/ubuntu/gnome-shell-extension-appindicator/commit/f57dbd5792a93fb28b6145d629e527b194fb6ab3)）、LayoutUpdated 对空菜单的处理（[commit fdc774c](https://github.com/ubuntu/gnome-shell-extension-appindicator/commit/fdc774c9138e08928967dfa79f6564f1f810e0ef)）。

**参考来源**：https://extensions.gnome.org/extension/4114/fildem-global-menu/、https://github.com/gonzaarcr/Fildem、https://gitlab.com/lestcape/Gnome-Global-AppMenu、https://github.com/ChathurangaBW/AppMenu、https://deepwiki.com/ubuntu/gnome-shell-extension-appindicator/6-menu-system

---

## 6. 2024–2026 新项目与前沿动向

### 6.1 Wayland 侧协议进展（直接影响 Noctalia 的集成方式）

| 协议/项目 | 说明 | 来源 |
|---|---|---|
| `org_kde_kwin_appmenu_manager` | KDE 私有协议：客户端将 DBusMenu 的 (service, path) attach 到 wl_surface；Qt Wayland 平台插件内置 | https://wayland.app/protocols/kde-appmenu |
| `xdg-dbus-annotation`（草案） | KDE 推动的标准化替代（KWin MR !5064），Firefox 曾用后被 kde-appmenu 取代 | https://invent.kde.org/plasma/kwin/-/merge_requests/5064、https://bugzilla.mozilla.org/show_bug.cgi?id=1956707 |
| GTK shell 协议 `global_menu_bar` | GTK 应用通过 gtk-shell 告知 DBus 地址导出全局菜单 | https://github-wiki-see.page/m/WayfireWM/wayfire/wiki/Configuration |
| Wayfire 0.11（2026-07） | 同时集成 KDE AppMenu + GTK global-menu，shell 可将应用菜单关联到 Wayland 窗口 | https://wayfire.org/2026/07/24/Wayfire-0-11.html |
| dkondor/gtk_global_menu | Wayfire 全局菜单演示：Python/GTK3 按钮版 + C++/GTK4 layer-shell 菜单栏版 | https://github.com/dkondor/gtk_global_menu |

### 6.2 新项目

- **qtile-extras GlobalMenu widget**（Python）：qtile 栏的全局菜单 widget。`qtile_extras.resources.dbusmenu.DBusMenu`（dbus-fast 异步实现 DBusMenu 客户端）+ `global_menu.registrar`（Registrar 客户端），`hook.subscribe.focus_change` 监听焦点，点击后 `get_menu(root=item.id)` 展开子菜单。**最小可读的 Python DBusMenu 参考**：https://github.com/elparaguayo/qtile-extras/blob/main/qtile_extras/widget/globalmenu.py
- **CuarzoSoftware/Heaven**（C++20，2025）：**不用 DBusMenu 协议**的自有全局菜单栈：三个库 `cz-heaven-bar`（显示端，镜像所有客户端菜单）/ `cz-heaven-client`（客户端导出，对象树 + 批量 `Commit` 模型）/ `cz-heaven-compositor`（Wayland token 认证 + 决定活动客户端）。显示端 ↔ 客户端通过 D-Bus 会话总线，`NameOwnerChanged` 驱动重连，客户端重连后全量重发状态。对"自定义协议 vs DBusMenu 互操作"的取舍有参考价值：https://github.com/CuarzoSoftware/Heaven
- **NovaBar**（Vala+GTK3，2025-12）：macOS 风格面板，**同时支持 `org.gtk.Menus` 与 `com.canonical.dbusmenu` 两种协议**（X11 全局菜单、Wayland 显示窗口标题）：https://github.com/novik133/NovaBar
- **helloSystem/Menu**（Qt）：FreeBSD helloSystem 的 Qt 全局菜单栏（KDEPlasmaPlatformTheme 导出，依赖 KF5 最少化）：https://github.com/helloSystem/Menu
- **noctalia-appmenu**（Rust + Quickshell QML，2026-05）—— **与 Noctalia shell 直接相关的先行项目**：https://github.com/yolo-labz/noctalia-appmenu
  - v0.1：**DBusMenu/Registrar 管线**（ADR-0001 决定复用 vala-panel-appmenu-registrar；ADR-0022 bridge 自己持有 registrar；ADR-0023 焦点变化时拉取菜单）。
  - v1.0：转向 **AT-SPI 无障碍总线**（ADR-0024）——原因：Quickshell 的 `DBusMenuHandle` 是 `QML_UNCREATABLE`，无法在 QML 里绑定任意 (busName, objectPath)（ADR-0007），于是 Rust sidecar bridge 订阅 niri IPC 焦点事件 → 遍历 AT-SPI 树提取菜单栏 → 写 `~/.cache/…/active.json` 快照 + 固定 D-Bus 地址，QML widget 订阅快照渲染；点击经 bridge 用 AT-SPI `DoAction` 回传。
  - 架构决策记录（ADR）完整记录了每个取舍（为什么 niri-only、为什么用固定代理地址、去抖策略、Firefox lazy menubar 的揭示锁定问题等），**强烈建议阅读**：`docs/adr/` 与 `docs/research/noctalia-and-appmenu-sota-2026-05-30.md`。

---

## 7. 对 Noctalia shell 插件设计的综合建议（调研结论）

### 7.1 菜单订阅（显示端如何拿到数据）

| 方案 | 代表实现 | 适用性 |
|---|---|---|
| **D-Bus 总线广播**：监听 `com.canonical.AppMenu.Registrar`（应用主动 RegisterWindow）+ 自己维护 winId→menu 映射 | KDE MenuImporter（X11 属性写入）、vala-panel-appmenu registrar、qtile-extras | 需要 WM 提供活动窗口 ID（X11 容易；Wayland 需要 compositor 集成） |
| **WM/Compositor 数据通道**：从 compositor 的窗口模型直接取 (service, path) | KDE AppMenuModel + TaskManager（KWin 实现 roles）；KWin `org_kde_kwin_appmenu_manager` 协议 | Wayland 唯一标准路径，但要求 compositor 实现协议；niri 目前没有（noctalia-appmenu 因此走 AT-SPI） |
| **AT-SPI 无障碍树**（无需 DBusMenu）：遍历活动应用的可访问性树提取菜单 | noctalia-appmenu v1.0 | 与 WM 协议无关、覆盖 Qt/GTK，但依赖 `QT_ACCESSIBILITY=1`/GTK a11y，Firefox/Electron 有局限，点击用 DoAction 语义较弱 |
| **窗口管理器钩子**（WM 内部）：活动窗口变化由 WM 直接通知插件 | KDE applet 的 activeTaskChanged；qtile focus_change | 插件内建在 shell 中时最自然（Noctalia 若是自家 compositor 则优先此路径） |

### 7.2 渲染与交互的关键工程点

1. **菜单模型缓存**：顶层菜单栏 GetLayout(0,1) 拉取；子菜单**弹出时才拉取**（懒加载，KDE importer 对每个 QMenu::aboutToShow 调 AboutToShow+重新 GetLayout；qtile `get_menu(root=id)` 同思路）。
2. **事件流**：弹出前发 `AboutToShow` + `opened`（两者都发以兼容 Qt/Firefox 差异）；关闭发 `closed`；点击发 `clicked`（含 id）；hover 切换子菜单可选 `hovered`。
3. **增量更新**：`LayoutUpdated` 去抖后整树重拉；`ItemsPropertiesUpdated` 局部更新（勾选、禁用、标签变化实时反映）。
4. **异常处理**：菜单服务消失（NameOwnerChanged/serviceUnregistered）→ 关闭菜单、回退占位；空菜单（"naughty apps"）→ 隐藏；X11 弹窗类型过滤；注册后未主动注销的窗口需要按需清理（KDE 靠窗口关闭事件 + QDBusServiceWatcher）。
5. **UI 渲染**：成熟实现都复用工具包原生菜单（QMenu / GtkMenuBar / PopupMenu / St.PopupMenu）来获得键盘导航、助记符、hover 展开、屏幕边缘行为——不建议完全自绘；Noctalia 若用 QML，参考 KDE applet 的"QML 按钮条 + C++ 原生弹出菜单"组合。
6. **Alt 助记符与键盘导航**：KDE 用 Kirigami.MnemonicData + 修饰键状态；全局菜单的 Alt 键聚焦在 X11 上是痛点（[KDE bug 376726](https://bugs.kde.org/show_bug.cgi?format=multiple&id=376726)）。
7. **回退策略**：无导出菜单的应用显示 .desktop 动作（vala-panel-appmenu helper-desktop）或合成通用动作（AppMenu 扩展）；GTK4 应用普遍无菜单导出（GTK4 移除模块加载），需 `org.gtk.Actions` 或回退。
8. **Wayland 窗口定位协议缺失时的出路**：noctalia-appmenu 的教训——compositor 不实现 appmenu 协议时，DBusMenu 管线只能靠 Registrar + PID/窗口匹配（脆弱），AT-SPI 是可行的替代基座。

### 7.3 最值得精读的源码清单

1. KDE `libdbusmenuqt/dbusmenuimporter.cpp` — DBusMenu 客户端最完整实现（Qt）
2. KDE `applets/appmenu/appmenumodel.cpp` + `appmenuapplet.cpp` — 显示端模型与弹出交互
3. vala-panel-appmenu `lib/helper-dbusmenu.vala` + `menu-widget.vala` — DBusMenu→GMenuModel 转换（Vala）
4. qtile-extras `resources/dbusmenu.py` + `widget/globalmenu.py` — 简洁的异步 DBusMenu 实现（Python）
5. gnome-shell-extension-appindicator `dbusMenu.js` — GJS DBusMenu 实现
6. noctalia-appmenu `docs/adr/` — 为 Noctalia 场景（Quickshell/QML + niri 类 compositor）定制的架构决策记录
7. Heaven `src/` — 自研协议全局菜单栈设计（对照参考）

---

## 8. 实证勘误与补充（2026-08-01，global-menu 插件实现期间）

以下结论经本机（Archcraft/Arch，niri 26.04 8ed0da4，at-spi2-core 2.60.5）实测验证，修正上文部分推测：

1. **niri 26.04 未实现 `org_kde_kwin_appmenu_manager`，也未实现 gtk-shell**（`grep -a` 二进制确认 0 命中）。§6.1 中"niri 支持此协议"的说法不成立——Qt/Chromium/Firefox 的 Wayland 菜单导出通道在本机不存在。
2. **appmenu-gtk-module（vala-panel fork）在 Wayland 下不调用 RegisterWindow**：菜单导出为 `org.gtk.Menus`（GMenuModel，`/org/appmenu/gtk/window/menus/menubar/<id>`，应用唯一名），地址仅经 gtk-shell `global_menu_bar` 传递（niri 丢弃）；X11 下写 X11 窗口属性 `_GTK_UNIQUE_BUS_NAME`/`_UNITY_OBJECT_PATH`。模块惰性 watch Registrar 名字出现。
3. **本机 at-spi2 桥与标准 at-spi2-core 的差异**（GIMP 3.2/GTK3 探针实测）：`GetRegisteredApplications`/`GetChildCount`/`GetName`/`GetActionCount`/`GetActionName` 方法缺失（用 `org.freedesktop.DBus.Properties.Get` 读 `ChildCount`/`Name`/`NActions` + `GetName(i)` 兜底）；`GetState` 返回 `au`（双 u32 位词）。
4. **AT-SPI 权威 role/state wire 常量**（atspi-constants.h 2.60.5）：FRAME=23、WINDOW=69、MENU=33、MENU_BAR=34、MENU_ITEM=35、CHECK_MENU_ITEM=8、RADIO_MENU_ITEM=45、SEPARATOR=50；STATE_VISIBLE=30（31 是 MANAGES_DESCENDANTS，易混淆）、SHOWING=25、CHECKED=4、SENSITIVE=24、ENABLED=8。
5. **GIMP 3.2 自绘菜单栏（无 GtkMenuBar）**：AT-SPI 树中无 MENU_BAR 节点——不适合作为 GTK3 全局菜单的验证对象（但恰好验证"无菜单回退"路径）。
6. **niri 对 XWayland 窗口的 pid 不可靠**：xwayland-satellite 场景下报卫星进程 pid（实测 2540），a11y 总线按真实 pid 匹配会失败——XWayland 应用需 pid 兜底策略（app_id/名称匹配，P1）。
7. **niri event-stream 多订阅者行为**：实测同时存在多个 `niri msg event-stream` 连接时事件只送达最新订阅者（手动测试连接会"抢走"桥的事件）——诊断时注意勿留多余的 event-stream 连接。
