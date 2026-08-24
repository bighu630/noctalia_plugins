# Noctalia v5 插件系统调研报告 —— nc-screen-recorder 迁移可行性

> 调研日期：2026-02
> 信息来源：
> - 官方文档 v5：https://docs.noctalia.dev/noctalia/plugins/development/{manifest,entries,declarative-ui,runtime-api,workflow,plugin-api}/ 及 https://docs.noctalia.dev/noctalia/ipc/plugins/
> - community-plugins 仓库：https://github.com/noctalia-dev/community-plugins（v5 Luau 插件目录，每插件一个顶层目录 + catalog.toml，CI 索引）
> - 本地 v5 实例参考：`text-sync/`（bar widget）、`shell-command/`（launcher provider）、`global-menu/`（widget + panel + service 三条目联动，最佳参照）
> - 迁移对象：`nc-screen-recorder/`（v4 QML：Main.qml 411 行、BarWidget.qml、Panel.qml、Settings.qml、RegionSelector.qml）

---

## 一、v5 插件模型总览

- 插件 = 目录 + `plugin.toml` 清单 + 若干 **entry**（`.luau` 脚本），受信任、非沙箱（可直接读写文件、起进程、联网）。
- Entry 类型（对应 v4 概念）：

| plugin.toml 表 | 类型 | 对应 v4 |
|---|---|---|
| `[[widget]]` | 状态栏 widget（命令式或 ui.* 声明式渲染） | BarWidget.qml |
| `[[panel]]` | 弹出面板（ui.* 声明式） | Panel.qml |
| `[[setting]]` / `[[widget.setting]]` / `[[panel.setting]]` | 设置项 schema，宿主 GUI 编辑并持久化 | Settings.qml + settings.json |
| `[[service]]` | 无 UI 后台服务 | Main.qml 中的逻辑/IpcHandler |
| `[[shortcut]]` / `[[launcher_provider]]` / `[[desktop_widget]]` | 控制中心磁贴 / 启动器 provider / 桌面组件 | — |

- `plugin_api` 必填，当前支持 3–28（每级能力见 plugin-api 文档；如 24=argv 形式 runAsync、28=面板原生右键菜单）。
- 多条目之间不共享 VM 内存，用 `noctalia.state.set/get/watch` 通道交换普通值；持久化写 `noctalia.pluginDataDir()`。
- 外部依赖在 manifest `dependencies = ["gpu-screen-recorder", "slurp"]` 中声明。

## 二、核心能力逐项对照

### 1. 状态栏 Bar Widget ✅ 可直接实现

- 注册：`[[widget]] id="recorder" entry="widget.luau"`，用户把 `<author>/<plugin>:recorder` 放到 bar 上。
- 刷新：`function update()` + `noctalia.setUpdateInterval(1000)`（计时器场景官方 timer 插件即此模式）；秒级 tick 足够录制计时。
- 显示更新（命令式 API）：`barWidget.setText/setGlyph/setGlyphColor/setColor/setTooltip/clearTooltip/setFont/setVisible/isVertical/outputName/render`。计时文本、录音图标、红色录制态颜色均可直接设置。
- 复合内容：`barWidget.render(ui.row({...},{ui.glyph(...), ui.label(...), ...}))` 声明式树（红点+计时+麦克风图标组合可用此实现）；约束：横条上保持单行高度，bar 内无键盘控件。
- 点击：`onClick()` 打开面板（见 §2）；右键 `onRightClick()`（配合 §4 的 `panel.openContextMenu` 或直接 togglePanel）；中键默认打开插件设置（可用 manifest `[widget.actions] middle="none"` 释放）；滚轮 `onScroll(axis, steps, startsGesture)` 可做"滚动调音量类"快捷操作（如滚轮切换音频源）。
- 注意：用户可在设置里把某手势绑定为 shell action，绑定优先于脚本回调（与 v4 无冲突）。

### 2. Panel / 下拉菜单 ✅ 可直接实现

- 定义：`[[panel]] id="main" entry="panel.luau" width=380 height=... placement/floating position/open_near_click/dismiss_on_outside_click/keyboard_focus/persistent`。
- 渲染：`onOpen(context)` 里 `panel.render(ui.* 树)`，状态变化后重新 render 即可 diff 更新；`panel.close()` 关闭。
- 与 widget 联动：widget 的 `onClick()` 里调用 `noctalia.togglePanel("<author>/<plugin>:<panel-id>")`；也可 IPC `noctalia msg panel-toggle <id>`。宿主为每个 panel 自动注入 placement/position/**open_near_click**（面板出现在触发按钮附近，等价 v4 `pluginApi.openPanel(screen, root)`）三个标准设置项。
  - ⚠️ 已知坑（本项目 global-menu 实测）：部分版本 `togglePanel` 缺少点击锚点信息导致 open-near-click 不跟随，本地已有补丁 `docs/noctalia-togglepanel-anchor.patch`；迁移时需在目标版本上验证。
- 面板内交互控件齐全：`ui.button(onClick/onRightClick)、ui.toggle、ui.slider、ui.select、ui.input、ui.scroll、ui.progress、ui.graph` 等——v4 Panel.qml 的开始/停止按钮、音频源 NComboBox（→ `ui.select`）、保存目录输入均可一一映射。
- 右键上下文菜单：`panel.openContextMenu({items={header/item/separator}, onActivate="cb"})`（plugin_api 28，原生主题化菜单）→ 替代 v4 `NPopupContextMenu`。
- 计时显示：面板内 `panel.setWantsSecondTicks(true)` 让 `update()` 每秒触发，重渲染计时文本。
- 尺寸宿主所有（width/height 固定值或 `"fill"`），无运行时 setSize；v4 的 contentPreferredHeight 自适应需改为固定合理尺寸。

### 3. 设置页 ✅ 可直接实现（且比 v4 更省事）

- 声明式 schema，宿主自动生成 Settings GUI 并持久化：
  - 类型：string / string_list / string_map / bool / int / double / select(options) / file / folder / glyph / color；
  - 支持 default/min/max/visible_when（条件显隐）/advanced；label 走 `translations/en.json` 的 key。
- nc-screen-recorder settings.json 的各项（保存目录 file/folder、音频源 select、帧率 int、质量 select 等）全部有对应类型。
- 读取：`noctalia.getConfig(key)`；设置变更时 widget/panel 重建、service 收 `onConfigChanged()`。
- ⚠️ 限制：插件**只能读不能写**配置。v4 用 `pluginApi.pluginSettings.isRecording=...; saveSettings()` 在条目间共享运行时状态的做法必须改用 `noctalia.state.*`（内存）+ `pluginDataDir()` 文件持久化（global-menu 已示范该模式）。

### 4. 进程执行 ✅ 可实现（停止进程需变通）

- 启动+捕获：`noctalia.runAsync(cmd字符串或argv数组 [, cb])` → cb 收 `{exitCode, stdout, stderr, timedOut}`；argv 形式免 shell 转义（plugin_api 24）。适合 slurp 选区、mkdir、通知等一次性命令。
- 长驻流式：`noctalia.runStream(cmd, onLine)` 逐行回调 stdout，适合解析 gpu-screen-recorder 日志/进度；脚本重载、条目移除、插件停止时宿主自动终止流。
- 辅助：`commandExists(name)`（检测 gpu-screen-recorder 是否安装）、`processMatches(cb, needles)`（检测进程存活）、`getenv`、`expandPath`、`runInTerminal`。
- ❌ **没有进程句柄 API**：无法对已启动进程发送信号（v4 `Process.signal(2)` 无对应物）。
  - 变通：`noctalia.runAsync("pkill -INT -f gpu-screen-recorder")`（SIGINT 触发其优雅收尾保存文件），或启动时用包装 shell 记录 `$!` 到 pluginDataDir 再 `kill -INT $(cat pidfile)`。功能可达，仅多一步。

### 5. UI 组件库 ✅ 基本齐备

ui.label / glyph / image / markdown / box / separator / spacer / progress / button(variant: primary/destructive/ghost…, tooltip) / graph / toggle / slider / select / input(含 password/multiline) / scroll(stickToBottom) / dragSource / dropZone；容器 row/column 支持 onClick/onHover。颜色用主题 token（primary/error/surface_variant…，可带 alpha 如 `"error"`、`"primary/0.6"`）或 hex。

与 v4 差异：
- `NComboBox` → `ui.select`（纯字符串 options + selectedIndex，无自定义行模板）——音频源下拉够用。
- 无 QML 动画系统（Behavior/SequentialAnimation）：录制红点呼吸闪烁在 bar 上只能靠 ~500ms update interval 重渲染切换透明度/颜色来近似（面板内可用 `setNeedsFrameTick` + onFrameTick 平滑动画）。
- hover 高亮：imperative API 无 hover 回调；声明式树上 row/button 有 `onHover`，可部分还原。

### 6. IPC 与通知 ✅ 直接实现

- 每个 entry 可定义 `onIpc(event, payload)`；外部触发：`noctalia msg plugin <author/plugin:entry> <focused|DP-1|all> <event> [payload]`（v4 IpcHandler 的 startRecording/stopRecording/toggle 一一对应，文档示例甚至用了 `noctalia/screen_recorder:service`）。
- 通知：`noctalia.notify(title, body)` / `notifyError`（v4 的带按钮通知 Process 变通可简化为普通通知或仍 shell 出去）。
- 其他可用：`copyToClipboard`、`formatTime`、`http/download`、`json.encode/decode`、`tr/trp` i18n（translations/en.json、zh-CN.json 可沿用）。

### 7. 不支持 / 无需调研

- RegionSelector 屏幕蒙版：v5 无自绘覆盖层能力——但用户已确认用 slurp 等外部工具选区，无需实现。
- Bar 内键盘输入控件（ui.input/select 在 bar 被跳过）：本需求不需要。

## 三、结论清单

**可直接实现**
1. 状态栏 widget 注册、图标/文本/颜色/tooltip 更新、每秒计时（update + setUpdateInterval）
2. 左键 togglePanel 打开面板（open_near_click 跟随按钮，需版本验证锚点补丁）
3. 右键菜单（onRightClick + panel.openContextMenu，plugin_api≥28）
4. 面板 UI：开始/停止按钮、录制状态+计时、音频源 ui.select、目录选择（settings 的 file/folder）
5. 设置 schema 声明式持久化（替代 Settings.qml + settings.json）
6. gpu-screen-recorder 启动与 stdout 流式读取（runStream/runAsync argv）
7. slurp 选区、mkdir、通知、剪贴板、IPC 外部触发、i18n

**需变通实现**
1. 停止录制：无进程句柄/信号 API → `pkill -INT -f gpu-screen-recorder` 或 pidfile 包装脚本
2. 条目间运行时状态共享（isRecording/开始时间）：v4 写 pluginSettings → `noctalia.state.*` + `pluginDataDir()` JSON 持久化
3. 录制红点闪烁动画：bar 无 frame tick/QML 动画 → 高频 update 重渲染近似（面板内可用 frame tick）
4. 音频源 NComboBox 的富样式 → ui.select 纯文本下拉
5. 面板高度自适应 → 固定 width/height 或 "fill"
6. hover 态胶囊变色 → ui 树 onHover 近似或放弃

**不支持**
1. 自绘屏幕蒙版/区域选择器（已被 slurp 方案取代，无需支持）
2. 向子进程发送任意信号 / stdin 写入（变通见上）

**总体结论：迁移完全可行。** v5 的 widget+panel+service 三条目模型能完整承载 nc-screen-recorder 全部核心功能；工作量主要在 QML→Luau/ui.* 重写与状态管理方式改造，无阻塞性缺口。建议结构：`[[service]]` 管 recorder 进程与状态机（runStream+pkill、state 广播），`[[widget]]` 显示+开面板，`[[panel]]` 操作 UI，`dependencies=["gpu-screen-recorder","slurp"]`，`plugin_api ≥ 24`（建议 26+ 以覆盖 argv、context menu 等）。
