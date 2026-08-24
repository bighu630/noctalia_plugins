# nc-screen-recorder v5 迁移实施计划

> 状态：已与用户对齐（2026-06），原地重写 `/data/code/c++/noctalia_plugins/nc-screen-recorder/`
> 参照范例：`text-sync/widget.luau`、`global-menu/{widget,service,popup}.luau`

## 用户决策记录

| 项 | 决策 |
|---|---|
| 红点脉冲 | 静态红点，不做闪烁动画 |
| 计时格式 | `m:ss`（<1h）/ `h:mm:ss`，秒级刷新 |
| 右键行为 | 直接打开插件设置（若宿主无此 API 则回退为开面板） |
| 面板布局 | 与 v4 一致：状态行 / 开始停止大按钮 / 6 个下拉 / 设置入口 |
| 区域选择 | slurp 外部工具；**无需坐标换算**——man 页确认 `-region WxH+X+Y` 与 slurp 兼容，直接 `slurp -f "%wx%h+%x+%y"` 输出喂给 `-w` |
| slurp 失败 | Esc/空输出 → toast「已取消区域选择」，非错误 |
| 停止录制 | pidfile 方案：包装脚本记录 `$!`，停止时 `kill -INT $(cat pidfile)` |
| 重启恢复 | 启动时以 pid 存活性为准：pid 活→接管为录制中（沿用原 startTime）；pid 死→清场回 idle。**绝不**凭持久化字段直接显示录制中 |
| 设置 | 声明式 schema（见下）；面板内改动经 service 写 overrides（见接口契约 §3） |

## 架构

```
[[service]] service.luau   进程与状态机（唯一写状态者）
[[widget]]  widget.luau    状态栏渲染 + 左键 togglePanel + 右键设置
[[panel]]   panel.luau     操作 UI（attached 锚定图标下方）
```

- `plugin_api = 26`（argv runAsync 等）
- `dependencies = ["gpu-screen-recorder", "slurp"]`
- id：`bighu630/nc-screen-recorder`，version 2.0.0
- panel：`placement="attached"` `position="auto"` `open_near_click=true` `dismiss_on_outside_click=true`，width≈380 height≈600（fixed）

## 接口契约（三个条目共同遵守，不得偏离）

### 1. state 通道（值一律 JSON 字符串）

| key | 方向 | 载荷 |
|---|---|---|
| `rec_state` | service → widget/panel | `{isRecording:bool, startTime:number(ms,0=idle), file:string, streaming:bool}` |
| `rec_options` | service → widget/panel | `{audioSource,target,codec,quality,framerate,streamDestination}`（生效值快照）|
| `rec_cmd` | widget/panel → service | `{op:"start"\|"stop"\|"setOption", key?, value?}` |

- watch 回调里 `pcall(json.decode)`；初始化用同步 `state.get` 兜底（global-menu 模式）。
- service 收到 cmd 后必须把 `rec_cmd` 复位为 `"null"` 或递增 seq 防重复消费（约定：处理完 set 回 `"null"`；消费方忽略 `"null"`）。

### 2. 录制流程（service 独占）

启动（target 解析）：
- `region` → `runAsync({"slurp","-f","%wx%h+%x+%y"})`：exitCode==0 且输出非空 → 几何串直接用；否则 toast 取消。
- `fullscreen` → `-w screen`
- 其他值 → 视为显示器名 `-w <name>`

命令构建（argv 免转义优先，含用户字符串的路径/URL 用 bash 包装并单引号转义）：
```
mkdir -p <dir>  （文件模式先执行）
gpu-screen-recorder [-o FILE | -o URL -c flv] [-w W|-w screen] [-f FPS]
  [-k CODEC](非auto) [-q Q] [-a default_input|default_output|'default_output|default_input']
```
包装脚本捕获 pid：
```bash
gpu-screen-recorder <args> & pid=$!; echo $pid > '<pluginDataDir>/recorder.pid'; wait $pid; echo "__GSR_EXIT__:$?"
```
以 `runStream(["bash","-c",wrapper], onLine)` 运行；onLine 解析 `__GSR_EXIT__:` 前缀取退出码收尾；其余行仅 log。

退出码处理：`0` 或 `255`（SIGINT 优雅结束）→ 成功：文件模式且非流式 → `notify` 带 文件路径；流式 → 「推流已停止」。其他 → `notifyError` 失败。

停止：
```
kill -INT $(cat recorder.pid)；pidfile 不存在或进程已死 → 清理 pidfile 并提示未在录制
```

### 3. 选项解析与持久化

v5 插件只读配置，不能写。因此：
- 生效选项 = `pluginDataDir()/overrides.json` 的键 ?? `noctalia.getConfig(key)` ?? 默认值。
- panel 改选项 → `rec_cmd {op="setOption",key,value}` → service 写 overrides.json → 重广播 `rec_options`。
- service 启动即广播一次 rec_state + rec_options。

### 4. 会话持久化（重启恢复依据）

`pluginDataDir()/session.json`：`{pid,startTime,file,streaming}`。
- 开始录制成功后写入；正常结束后删除。
- service 启动：读 session.json → pid 存活（`kill -0`）→ 恢复录制态（isRecording=true，startTime 沿用）；否则删除 session.json/pidfile，广播 idle。

### 5. widget 渲染

- idle：普通胶囊色 + glyph `circle`；tooltip「屏幕录制」。
- recording：背景 `error` 色，render 树 = 白点(glyph 或 label ●)+白色计时文本+麦克风 glyph(音频开启时)；静态不闪烁；`setUpdateInterval(1000)` 刷新计时，idle 时降频。
- `onClick()` → `noctalia.togglePanel("bighu630/nc-screen-recorder:main")`。
- `onRightClick()` → 打开插件设置：先查 runtime-api 是否有对应函数；无则回退 togglePanel。

### 6. panel 渲染

- `onOpen(context)` render 全树；watch rec_state/rec_options 重渲染；录制中 `setWantsSecondTicks(true)`。
- 内容自上而下：状态行（红点+「录制中」+计时+音频图标）/ 大按钮（开始=primary、停止=destructive）/ 6 个 `ui.select`（录制中禁用）：音频源、录制目标(区域选择/全屏/显示器列表)、编码器、质量、帧率、推流到（首项「不推流」+ streamDestinations 配置项遍历）/ 底部 outlined 设置按钮。
- 显示器动态列表：查 runtime-api 是否有枚举 output/monitor 的 API；没有则目标下拉仅「区域选择/全屏」+ 说明。

## 设置 schema（[[setting]]，label_key 走 translations）

| key | type | default |
|---|---|---|
| saveDirectory | folder | ~/Videos |
| filePattern | string | recording_{datetime} |
| videoFormat | select mp4/mkv/webm | mp4 |
| codec | select auto/h264/hevc/av1/vp9 | auto |
| quality | select medium/high/very_high/ultra | high |
| framerate | select 30/60/120/144 | 60 |
| audioSource | select none/mic/desktop/both | none |
| streamDestinations | string_map | {} |
| streamDestination | select（静态 schema 无法动态列出，声明为 string 默认""；实际列表在 panel 动态渲染） | "" |

## 文件清单

新增：`plugin.toml`、`service.luau`、`widget.luau`、`panel.luau`、`translations/en.json`、`translations/zh-CN.json`
删除（v4 遗留）：`Main.qml` `BarWidget.qml` `Panel.qml` `Settings.qml` `RegionSelector.qml` `dimming.frag` `dimming.frag.qsb` `select_region.sh` `manifest.json` `settings.json` `i18n/` `.codex`
更新：`README.md`、仓库根 `catalog.toml`/`registry.json` 条目

## 验收标准

1. `plugin.toml` 语法有效，三条目齐全，dependencies/api 正确。
2. 三个 luau 无语法错误（luau 或 lua 解析器校验通过），契约中的 state key/id/常量完全一致。
3. 代码走查覆盖：slurp 取消路径、pid 死活恢复路径、流式/文件双模式、setOption 持久化、i18n key 与 translations 文件一一对应。
4. 安装后人工验证项（写入 README）：胶囊两态显示、左键面板出现在图标下方、区域录制出片、SIGINT 停止保存、重启无残留录制态。
