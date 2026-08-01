# Noctalia v5 全局菜单（Global Menu）插件设计

> 状态：已与用户确认（2026-08-01）
> 前置调研：`docs/global-menu-research.md`（含本机环境结论）
> 先行项目：`github.com/yolo-labz/noctalia-appmenu`（v0.1 DBusMenu 管线 → v1.0 AT-SPI 基座，`docs/adr/` 完整决策记录）

## 1. 背景与目标

在 Noctalia v5（运行于 niri compositor 之上）实现 macOS 风格全局菜单：焦点应用（GTK3 起步）的菜单栏（File/Edit/View/…）显示在 shell 菜单栏条中，点击顶层项展开子菜单，点击叶子项触发应用真实动作。

**已确认的架构方向**：
1. 本机 Noctalia v5 运行在 niri 之上，依靠 niri IPC 捕捉活跃窗口；
2. Luau 插件与各种应用完全隔离——所有复杂度（协议差异、窗口跟踪、DBus 交互）由中间桥接程序处理，插件只面对统一化的菜单模型。

## 2. 实证调查结论（2026-08-01 本机验证）

| 项目 | 结论 |
|---|---|
| niri 版本 | 26.04 (8ed0da4)。**未实现** `org_kde_kwin_appmenu_manager`、**未实现** gtk-shell（二进制 grep 0 命中）——修正调研文档中"niri 支持此协议"的说法 |
| niri IPC | `niri msg --json event-stream` 可用：`{"WindowFocusChanged":{"id":30}}`（外部标签形式）、`WindowsChanged`（含 pid/title/app_id/is_focused）、`WorkspaceActiveWindowChanged`。`niri msg --json windows` 无 x11 id、无 appmenu 字段 |
| GTK3 + appmenu-gtk-module 25.04 | Wayland 后端：导出 `org.gtk.Menus` 到会话总线 `/org/appmenu/gtk/window/menus/menubar/<id>`，地址仅经 gtk-shell 传递（niri 丢弃）→ 不可发现；**不调用 RegisterWindow**。XWayland 后端：写 X11 窗口属性 `_GTK_UNIQUE_BUS_NAME`/`_UNITY_OBJECT_PATH`。注册行为惰性：watch Registrar 名字出现 |
| AT-SPI 基座 | **本机可用**：a11y 总线运行中；GTK3 应用零配置自动接入（GIMP 实测完整菜单树 + 子菜单关闭状态可读 + `DoAction(0)` 点击返回 True） |
| Arch qt6-base 6.11.1 | **未编译 AT-SPI 桥插件**（无 accessibility/ 插件目录、无 at-spi2-core 依赖，kwrite + QT_ACCESSIBILITY=1 实测未连 a11y 总线）→ 本机 Qt6 应用无任何菜单导出通道 |
| 用户应用生态 | Chrome Canary（Chromium）、PiPlus（Electron）为主——均无菜单可导出；GIMP（GTK3 ✅）、kwrite/okular/qutebrowser（Qt6 ❌）、WPS（Qt ❌） |
| Noctalia v5 runStream | `/bin/sh -c` 启动，stdout 逐行回调（`\n` 分割，去 `\r`），**无 stdin API**，不自动重启，reload 时杀全部子进程，单行上限 64KB（kMaxStreamLineBytes） |

### 基座决策
- **AT-SPI 主线**（GTK3）：与 noctalia-appmenu v1.0 最终选择一致，本机实测全链路可用
- **不做 DBusMenu/Registrar 主线**：Wayland 下 GTK3 不可发现（gtk-shell 被忽略）；Qt/Chromium 通道被 niri 缺失协议封死
- 桥额外拥有 `org.a11y.Status`（IsEnabled=true）：GTK3 不需要（实测自动启用），为 Qt 未来修复预留

## 3. 架构

```
┌────────────────────────────────────────────────────────────┐
│ Noctalia v5（运行在 niri 之上）                              │
│                                                            │
│  Luau 插件（隔离 VM，plugin.toml + .luau）                  │
│  ├─ [[widget]] 菜单栏条 ── ui.row + ui.button（顶层项）     │
│  ├─ [[panel]] 子菜单弹出 ── floating popup + capture_keys   │
│  └─ runStream 托管桥                                        │
│       │ stdout：NDJSON 逐行事件（上行）                      │
│       ▼                                                     │
│  bridge（Rust 单二进制，随插件分发，无状态可重启）           │
│  ├─ niri IPC   焦点跟踪：event-stream + windows 查询        │
│  ├─ AT-SPI     菜单树读取 + DoAction 点击 + org.a11y.Status │
│  └─ HTTP 127.0.0.1:随机端口  ← noctalia.http POST（下行）    │
│       │                                                     │
│  ─────┴────────────────────────────────────────             │
│  会话总线 / a11y 总线                                        │
│  GTK3 应用（GIMP…）AT-SPI 自动接入（本机已实测）             │
└────────────────────────────────────────────────────────────┘
```

**桥内部模块**（单进程多线程，Rust）：
- `niri.rs` — 事件流订阅。serde **外部标签**枚举 + 手动 Deserialize 带 `Other` 兜底（ADR-0016 教训：schema 漂移 warn-and-skip，绝不崩溃）
- `atspi.rs` — a11y 总线发现（`org.a11y.Bus.GetAddress`）、应用枚举、按 (pid, title) 定位 MENU_BAR、树 walker（角色/状态枚举映射、Qt 无名 MENU wrapper 扁平化）、`DoAction` 点击
- `proxy.rs` — 焦点事件 → eager fetch 菜单树 → 去抖 → 输出事件
- `stdout.rs` — NDJSON 发射器（逐行加锁写 stdout）
- `http.rs` — 本地回环 HTTP 命令服务
- `status.rs` — 拥有 `org.a11y.Status`（IsEnabled=true）

**窗口 → 菜单解析**（ADR-0030 教训）：
- niri 焦点窗口 (id, pid, title) → a11y 总线按 PID 找应用
- 单窗口应用：直接取应用根
- 多窗口同 PID：AT-SPI frame Name **精确匹配** niri 窗口 title；匹配失败 → menu:null 占位（**绝不猜**）

## 4. 通信协议

### 上行（桥 → 插件，stdout 每行一个 JSON）
| 消息 | 时机 | 内容 |
|---|---|---|
| `hello` | 桥启动 | `{"type":"hello","port":<随机端口>,"pid":<桥pid>}` |
| `menu` | 焦点变化/菜单变化 | `{"type":"menu","app":{"app_id","title","pid"},"menu":{...}\|null,"source":"atspi"\|"none"}` |
| `heartbeat` | 每 5s | `{"type":"heartbeat","ts":...}` |
| `error` | 非致命错误 | `{"type":"error","msg":"..."}` |

### 下行（插件 → 桥，HTTP 回环）
| 端点 | 用途 | 返回 |
|---|---|---|
| `POST /click` | `{"id":N}` 点击菜单项 | `{"ok":bool,"error"?}`；成功后桥自动重拉并补发 `menu` 事件 |
| `POST /open` | `{"id":N}` 展开子菜单前拉最新子树（懒构建兜底） | 子树 JSON（id 与主树同一会话空间，插件可原地替换） |
| `POST /refresh` | 强制重拉 | 补发 `menu` 事件 |
| `GET /ping` | 健康检查 | `{"ok":true}` |

### 菜单 id 会话语义
- 桥在每次解析时按 DFS 分配 1..N，**当前解析会话内稳定**（焦点变化 → 会话重建 → 旧 id 全部失效；插件收到新 `menu` 事件时丢弃旧树）
- 点击时桥**重新走完整解析链再 DoAction**（accessible path 会被回收，绝不跨会话缓存路径）

## 5. 统一菜单 JSON schema

```json
{
  "id": 3,                 // 桥按 DFS 分配，当前解析会话内稳定
  "label": "Export…",      // 已去助记符下划线
  "mnemonic": "E",         // 可空
  "type": "item",          // submenu|item|separator|checkbox|radio
  "enabled": true,
  "visible": true,
  "checked": false,        // checkbox/radio
  "icon": null,            // AT-SPI 无图标，字段保留
  "children": []           // 仅 submenu 非空
}
```

- 顶层 = `menu.children`（菜单栏各项）
- 树全量下发（GIMP 级 ~10KB，远低于 runStream 64KB 单行上限）
- 超 64KB 巨型菜单降级：只发顶层 + 子菜单走 `/open`（P1）

## 6. 点击链路（两段式）

1. **展开子菜单 = 纯本地渲染**：AT-SPI 无 `AboutToShow`/`opened` 机制（DBusMenu 特有），GTK3 菜单树应用启动即构建（实测 GIMP 关闭状态下 View 菜单 39 子项可读）。插件用已下发的子树渲染弹出面板，**零应用交互、零网络往返**
2. **点击叶子项 = 回传应用**：插件 `POST /click {id}` → 桥重解析当前焦点应用（pid+title）→ 按会话 child-index 链定位 → `DoAction(0)`（qtatspi 约定 0=click；动作索引非 0 时枚举动作名匹配 "click" 兜底）→ 成功后自动重拉并发新 `menu` 事件（勾选/禁用状态实时同步）
3. **懒构建兜底**：展开时本地子树为空 → `POST /open {id}` → 桥在**当前会话内**重新遍历到该节点读取其子节点（保持其他节点 id 不变，只替换该节点 children），返回 children 数组（id 与主树同一空间，插件原地替换）

## 7. 异常与重启处理

| 故障 | 检测 | 处理 |
|---|---|---|
| 桥进程崩溃 | 插件 update() 每 5s `GET /ping` 超时 + runStream 流断 | 插件重新 `runStream` 拉起桥（桥无状态，从 niri/a11y 全量重建） |
| Noctalia reload | 系统行为：`stopAllStreams` 杀桥 | 插件重启时自动重新拉起 |
| a11y 总线重启（罕见） | `org.a11y.Bus` 消失 | 桥检测后重连（GetAddress + 重建连接），期间 menu:null |
| niri IPC schema 漂移 | serde 解析失败 | `Other` 兜底 + warn 日志，绝不崩溃 |
| 应用中途退出/崩溃 | 焦点事件 / a11y 树消失 | 单次解析失败 → menu:null + 占位；下个焦点事件自然恢复 |
| 菜单解析连续失败 | 错误计数 | 退避重试（最多 2s 一次），不刷屏 |
| stdout 单行超 64KB | 序列化前检查 | 巨型菜单降级：只发顶层，子菜单走 `/open`（P1） |
| 桥启动失败 | hello 事件超时 | 插件显示占位 + `noctalia.log` 错误信息 |

## 8. 插件侧 UI（MVP）

```
[[widget]] global_menu（菜单栏条）
  └─ ui.row: [应用名] File Edit View …（顶层按钮，超出宽度截断）
      └─ 空菜单时：显示应用名 + 淡化提示（绝不空白）

[[panel]] global_menu_popup（子菜单弹出）
  └─ floating + dismiss_on_outside_click=true + keyboard_focus=on_demand
  └─ ui.column: 子项列表（label / separator / checkbox 勾选 / 子菜单 ▸ 指示）
      └─ hover 高亮、点击叶子项 → POST /click
      └─ 点击带 ▸ 的项 → 同面板内替换为下一级（MVP 简化）
```

- MVP 交互：**点击展开**（hover 自动展开为 P1）、鼠标操作为主
- `capture_keys`：MVP 可只做 Esc 关闭（dismiss_on_outside_click 已覆盖大部分）；完整键盘导航（方向键/Enter）为 P1

## 9. MVP 边界与里程碑

**MVP 入/出**：
- ✅ 入：GTK3 真实菜单（AT-SPI 全链路）、占位回退（应用名）、`org.a11y.Status` 所有权（Qt 预留）
- ⏭ P1：.desktop 动作回退、hover 自动展开、完整键盘导航、Alt 助记符、超大树降级、Firefox/GTK4/Electron 策略文档化
- ⏭ P2：Qt6（需重建 qt6-base 带 atspi）、多显示器菜单定位

**里程碑**：
| M | 内容 | 验证 |
|---|---|---|
| M1 | 桥骨架：niri 焦点跟踪 + hello/heartbeat + menu:null + HTTP ping | 焦点切到 GIMP 时 stdout 出事件 |
| M2 | AT-SPI 菜单读取 → 真实菜单栏渲染 | GIMP 菜单栏显示 |
| M3 | 点击回传 + 子菜单展开 | 点 GIMP "Fullscreen" 应用进入全屏 |
| M4 | 异常打磨 + UI 完善 + 打包（catalog.toml/registry.json + 构建脚本） | 桥崩溃自动恢复、reload 后自愈 |

## 10. 关键技术风险与对策

| 风险 | 对策 |
|---|---|
| AT-SPI 协议手写量大（zbus 无官方绑定） | 参考 noctalia-appmenu `bridge/src/atspi.rs`（生产验证）；角色/状态枚举用 at-spi2-core 的 `atspi-constants.h` 权威值 |
| DoAction 对个别应用无效 | 动作名枚举兜底 + 失败返回 error 事件，插件显示提示 |
| 焦点事件风暴（快速切换窗口） | 去抖（~150ms）+ 会话合并 |
| runStream 64KB 限制 | 全量下发前检查大小，超限降级 |
| a11y 树解析延迟（首次） | 桥预连接 a11y 总线；菜单拉取在焦点事件后异步进行，先发 app 信息再发菜单 |
