--!nonstrict
-- nc-screen-recorder v5 状态栏 widget：两态胶囊渲染。
--
-- 渲染机制：**纯命令式 API**（setGlyph/setText/setColor…）。录制态背景色经
-- barWidget.setColor("error") 直接作用于宿主胶囊本体（圆角造型由宿主负责），
-- 不用 ui.row{fill=...}（那是内容后面的方形底块，不是胶囊）。
-- 停止后以 setColor("") 恢复默认胶囊配色（udiskie 插件同款用法）。
--
-- 两态：
--   idle      circle 图标（on_surface 前景）+ 无文本 + 默认胶囊；低频兜底重渲染。
--   recording error 色胶囊 + 计时文本（on_error 前景）+ 图标区分：mic/both→microphone、desktop→speaker、none→circle；
--             setUpdateInterval(1000) 驱动秒级计时刷新。

local state = noctalia.state

local PLUGIN_ID = "bighu630/nc-screen-recorder"
local PANEL_ID = PLUGIN_ID .. ":main"

local IDLE_INTERVAL_MS = 5000
local RECORDING_INTERVAL_MS = 1000

-- 进程内状态快照（rec_state/rec_options 均为 JSON 字符串，契约 §1）
local recState = nil   -- {isRecording,startTime,file,streaming}
local recOptions = nil -- {audioSource,target,codec,quality,framerate,streamDestination}

-- ---------------------------------------------------------------- 工具函数

local function decodeSnapshot(jsonStr)
  if type(jsonStr) ~= "string" or jsonStr == "" or jsonStr == "null" then return nil end
  local ok, v = pcall(noctalia.json.decode, jsonStr)
  if ok and type(v) == "table" then return v end
  return nil
end

local function nowMs()
  return os.time() * 1000
end

local function isRecording()
  return recState ~= nil and recState.isRecording == true
end

-- <1h → m:ss；≥1h → h:mm:ss（用户决策：秒级刷新，分不补零到小时位）
local function formatElapsed(sec)
  sec = math.max(0, math.floor(tonumber(sec) or 0))
  local h = math.floor(sec / 3600)
  local m = math.floor((sec % 3600) / 60)
  local s = sec % 60
  if h > 0 then
    return string.format("%d:%02d:%02d", h, m, s)
  end
  return string.format("%d:%02d", m, s)
end

-- 当前生效音频源：优先 rec_options 快照，兜底读配置（service 未就绪时）
local function audioSource()
  if recOptions and recOptions.audioSource ~= nil then
    return tostring(recOptions.audioSource)
  end
  return tostring(noctalia.getConfig("audioSource") or "none")
end

local function elapsedSeconds()
  local st = tonumber(recState and recState.startTime or 0) or 0
  if st <= 0 then return 0 end
  return math.max(0, math.floor((nowMs() - st) / 1000))
end

-- ---------------------------------------------------------------- 渲染

-- 纯命令式渲染：颜色作用于胶囊本体，圆角/内边距造型全部由宿主管理。
local function refresh()
  if isRecording() then
    local a = audioSource()
    -- 与 panel 保持一致的音频图标语义：mic/both→microphone、desktop→speaker、none→circle
    -- 避免桌面音频录制时误显示麦克风图标
    local glyphName = "circle"
    if a == "mic" or a == "both" then
      glyphName = "microphone"
    elseif a == "desktop" then
      glyphName = "speaker"
    end
    barWidget.setGlyph(glyphName)
    barWidget.setGlyphColor("#000000") -- 固定黑色图标（亮/暗主题一致），红底胶囊上保证可见
    barWidget.setText(formatElapsed(elapsedSeconds()))
    barWidget.setColor("error") -- 录制态：红色胶囊本体（error）
  else
    barWidget.setGlyph("circle")
    barWidget.setGlyphColor("on_surface") -- idle 胶囊为默认 surface 背景，用 on_surface 比 on_surface_variant 对比度更高，明/暗主题均可见
    barWidget.setText("")
    barWidget.setColor("") -- 停止后恢复默认胶囊配色
  end
  barWidget.setTooltip(isRecording()
    and noctalia.tr("widget.tooltipRecording")
    or noctalia.tr("widget.tooltip"))
end

-- 定时兜底：录制中 1s 刷新计时；idle 低频重渲染（状态主要靠 watch 推送）
function update()
  noctalia.setUpdateInterval(isRecording() and RECORDING_INTERVAL_MS or IDLE_INTERVAL_MS)
  refresh()
end

-- ---------------------------------------------------------------- 交互

function onClick()
  noctalia.togglePanel(PANEL_ID)
end

-- 与 panel 同款命令通道：widget 直发停止命令（契约 §1，值必须为 JSON 字符串）
local function sendStop()
  state.set("rec_cmd", noctalia.json.encode({ op = "stop" }))
end

function onRightClick()
  -- 录制中 → 停止录制（SIGINT 优雅保存；插件无「暂停」语义，停止即收尾出片）
  if isRecording() then
    sendStop()
    return
  end
  -- 未录制 → 打开设置：优先宿主 openSettings()（plugin_api >= 15），
  -- API 不存在或调用失败时回退为 togglePanel，保证右键始终有响应。
  local ok = false
  if noctalia.openSettings then ok = pcall(noctalia.openSettings) end
  if not ok then noctalia.togglePanel(PANEL_ID) end
end

-- ---------------------------------------------------------------- state 订阅与初始化

state.watch("rec_state", function(json)
  recState = decodeSnapshot(json or "null")
  update()
end)

state.watch("rec_options", function(json)
  recOptions = decodeSnapshot(json or "null")
  refresh()
end)

-- 初始化恢复：watch 只响应变化（热重载/新实例拿不到当前值），用同步 get 兜底
local okS, vS = pcall(state.get, "rec_state")
if okS then recState = decodeSnapshot(vS) end
local okO, vO = pcall(state.get, "rec_options")
if okO then recOptions = decodeSnapshot(vO) end

noctalia.setUpdateInterval(isRecording() and RECORDING_INTERVAL_MS or IDLE_INTERVAL_MS)
refresh()
