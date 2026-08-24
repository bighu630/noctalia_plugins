--!nonstrict
-- nc-screen-recorder v5 状态栏 widget：两态胶囊渲染。
--
-- 渲染机制（Noctalia v5）：命令式渲染——调用 barWidget.render(ui树) 推送 UI 树，
-- state watch 变化或定时 tick 时重建并推送（global-menu 模式）。
--
-- 两态：
--   idle      普通胶囊（不设背景色）+ circle glyph + tooltip「屏幕录制」；
--             低频 setUpdateInterval(5000) 仅作兜底重渲染。
--   recording 背景 error 色；静态白点（用户决策：不做闪烁动画）+
--             白色粗体计时文本（由 now - rec_state.startTime 计算）+
--             音频开启时的麦克风 glyph；
--             setUpdateInterval(1000) 驱动秒级计时刷新。

local state = noctalia.state

local PLUGIN_ID = "bighu630/nc-screen-recorder"
local PANEL_ID = PLUGIN_ID .. ":main"

local IDLE_INTERVAL_MS = 5000
local RECORDING_INTERVAL_MS = 1000
local WHITE = "#ffffff"

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

local function buildTree()
  if isRecording() then
    local children = {
      -- 静态白点（明确不做闪烁）：小号实心圆字符即可，避免引入帧动画
      ui.label({ text = "●", color = WHITE, fontSize = 13 }),
      ui.label({ text = formatElapsed(elapsedSeconds()), color = WHITE, fontWeight = "bold" }),
    }
    local a = audioSource()
    if a ~= "" and a ~= "none" then
      children[#children + 1] = ui.glyph({ name = "microphone", color = WHITE, size = 14 })
    end
    -- 录制态：error 色背景胶囊（ui.row 支持 fill 主题 token）
    return ui.row({ gap = 6, paddingH = 10, align = "center", fill = "error" }, children)
  end

  -- idle 态：普通胶囊（不设背景色，跟随主题默认）
  return ui.row({ gap = 6, paddingH = 8, align = "center" }, {
    ui.glyph({ name = "circle", size = 16 }),
  })
end

local function refresh()
  barWidget.render(buildTree())
  -- 说明：一旦调用 render()，命令式 setText/setGlyph 即失效；tooltip 为独立属性仍可设置。
  -- 若宿主版本在 render 后忽略 setTooltip，仅悬停提示消失，不影响功能。
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

function onRightClick()
  -- M1：优先调用宿主的 openSettings()（打开本插件设置页，plugin_api >= 15）；
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
