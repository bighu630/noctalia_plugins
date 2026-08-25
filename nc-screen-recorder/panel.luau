--!nonstrict
-- nc-screen-recorder v5 下拉面板：状态行 / 开始停止大按钮 / 6 个下拉 / 设置入口。
--
-- 渲染机制：onOpen(context) 里 panel.render(ui树) 推送全树；watch rec_state/rec_options
-- 变化即重渲染；录制中 panel.setWantsSecondTicks(true) 让 update() 每秒触发刷新计时，
-- 面板关闭时 tick 自动停止（onClose 再显式取消一次）。
--
-- 契约要点：
--   * rec_cmd 的值必须是 JSON 字符串（noctalia.json.encode 后 state.set）。
--   * ui.select 为值驱动（每次渲染传 selectedIndex）；onChange 回调收到 0 基索引
--     （官方 community-plugins/audio-switcher 同款映射：values[index + 1]）。
--   * 显示器动态列表：runtime-api 文档确认 noctalia.outputs() 返回已连接输出
--     {name,description,width,height,x,y,scale,focused}，取 name 作为 -w 参数。

local state = noctalia.state

local ERROR_COLOR = "error"

-- 进程内状态快照（rec_state/rec_options 均为 JSON 字符串，契约 §1）
local recState = nil
local recOptions = nil

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

-- <1h → m:ss；≥1h → h:mm:ss（与 widget 一致）
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

local function elapsedSeconds()
  local st = tonumber(recState and recState.startTime or 0) or 0
  if st <= 0 then return 0 end
  return math.max(0, math.floor((nowMs() - st) / 1000))
end

-- 当前生效选项：优先 rec_options 快照（service 广播），兜底读配置
local function optionValue(key, fallback)
  if recOptions and recOptions[key] ~= nil and tostring(recOptions[key]) ~= "" then
    return tostring(recOptions[key])
  end
  local cfg = noctalia.getConfig(key)
  if cfg ~= nil and tostring(cfg) ~= "" then return tostring(cfg) end
  return fallback
end

local function sendCmd(cmdTable)
  -- rec_cmd 值必须是 JSON 字符串（契约 §1；service 解码后消费并复位为 "null"）
  state.set("rec_cmd", noctalia.json.encode(cmdTable))
end

-- ---------------------------------------------------------------- 控件构建

-- 构建表单行：标签在上方、下拉占满整行（wl-screen-mirror 的 section 惯用模式）。
-- 不用「固定宽标签 + 横排」——label 的 width 属性不可靠且易溢出面板宽度、
-- 各行标签自然宽度不一导致下拉无法对齐。
-- props: { label=..., values={...}, labels={...}, current=..., enabled=bool,
--          onPick=function(value) end }
-- onChange 收到 (index, label)，index 为 0 基（官方示例映射 values[index + 1]）；
-- ui.select 支持 enabled 参数（declarative-ui 文档确认），录制中禁用。
local function selectRow(props)
  local selectedIndex = 0
  for i, v in ipairs(props.values) do
    if v == props.current then
      selectedIndex = i - 1
      break
    end
  end
  return ui.column({ gap = 4 }, {
    ui.label({ text = props.label, fontSize = 11, color = "on_surface_variant" }),
    ui.select({
      options = props.labels,
      selectedIndex = selectedIndex,
      enabled = props.enabled,
      controlSize = "md",
      flexGrow = 1, -- 占满面板内容宽，所有下拉左右边缘对齐
      onChange = function(index, _label)
        local v = props.values[(math.floor(tonumber(index) or -1)) + 1]
        if v ~= nil then props.onPick(v) end
      end,
    }),
  })
end

-- 录制目标下拉：区域选择 / 全屏 / 动态显示器列表（noctalia.outputs()，官方 API）
local function targetRow(enabled)
  local values = { "region", "fullscreen" }
  local labels = {
    noctalia.tr("panel.targetRegion"),
    noctalia.tr("panel.targetFullscreen"),
  }
  local ok, outs = pcall(noctalia.outputs)
  if ok and type(outs) == "table" then
    for _, o in ipairs(outs) do
      if type(o) == "table" and type(o.name) == "string" and o.name ~= "" then
        values[#values + 1] = o.name
        labels[#labels + 1] = o.name
      end
    end
  end
  return selectRow({
    label = noctalia.tr("panel.recordingTarget"),
    values = values,
    labels = labels,
    current = optionValue("target", "fullscreen"),
    enabled = enabled,
    onPick = function(v)
      sendCmd({ op = "setOption", key = "target", value = v })
    end,
  })
end

-- 推流到下拉：首项「不推流」（""）+ 设置里 streamDestinations(string_map) 的键名
local function streamRow(enabled)
  local values = { "" }
  local labels = { noctalia.tr("panel.noStream") }
  local dests = noctalia.getConfig("streamDestinations")
  local names = {}
  if type(dests) == "table" then
    for name in pairs(dests) do
      names[#names + 1] = tostring(name)
    end
    table.sort(names)
    for _, name in ipairs(names) do
      values[#values + 1] = name
      labels[#labels + 1] = name
    end
  end
  return selectRow({
    label = noctalia.tr("panel.streamDestination"),
    values = values,
    labels = labels,
    current = optionValue("streamDestination", ""),
    enabled = enabled,
    onPick = function(v)
      sendCmd({ op = "setOption", key = "streamDestination", value = v })
    end,
  })
end

-- ---------------------------------------------------------------- 树构建

-- 底部「设置」按钮：打开本插件设置页（noctalia.openSettings() 会自动关闭已开面板）
local function settingsButton()
  return ui.button({
    text = noctalia.tr("panel.settings"),
    glyph = "settings",
    variant = "outline",
    contentAlign = "center",
    flexGrow = 1,
    onClick = function()
      -- M1：与 widget.onRightClick 相同的回退策略；区别在于面板场景下回退动作是
      -- 关闭面板（panel.close()）而非 togglePanel（避免把刚关掉的面板又打开）。
      local ok = false
      if noctalia.openSettings then ok = pcall(noctalia.openSettings) end
      if not ok then panel.close() end
    end,
  })
end

-- 六个下拉（录制中禁用）：音频源 / 录制目标 / 编码器 / 质量 / 帧率 / 推流到
local function buildSelects(recording)
  local rows = {}

  rows[#rows + 1] = selectRow({
    label = noctalia.tr("panel.audio"),
    values = { "none", "mic", "desktop", "both" },
    labels = {
      noctalia.tr("panel.audioNone"),
      noctalia.tr("panel.audioMic"),
      noctalia.tr("panel.audioDesktop"),
      noctalia.tr("panel.audioBoth"),
    },
    current = optionValue("audioSource", "none"),
    enabled = not recording,
    onPick = function(v)
      sendCmd({ op = "setOption", key = "audioSource", value = v })
    end,
  })

  rows[#rows + 1] = targetRow(not recording)

  rows[#rows + 1] = selectRow({
    label = noctalia.tr("panel.codec"),
    values = { "auto", "h264", "hevc", "av1", "vp9" },
    labels = {
      noctalia.tr("panel.codecAuto"),
      noctalia.tr("settings.codec.options.h264"),
      noctalia.tr("settings.codec.options.hevc"),
      noctalia.tr("settings.codec.options.av1"),
      noctalia.tr("settings.codec.options.vp9"),
    },
    current = optionValue("codec", "auto"),
    enabled = not recording,
    onPick = function(v)
      sendCmd({ op = "setOption", key = "codec", value = v })
    end,
  })

  rows[#rows + 1] = selectRow({
    label = noctalia.tr("panel.quality"),
    values = { "medium", "high", "very_high", "ultra" },
    labels = {
      noctalia.tr("settings.quality.options.medium"),
      noctalia.tr("settings.quality.options.high"),
      noctalia.tr("settings.quality.options.very_high"),
      noctalia.tr("settings.quality.options.ultra"),
    },
    current = optionValue("quality", "high"),
    enabled = not recording,
    onPick = function(v)
      sendCmd({ op = "setOption", key = "quality", value = v })
    end,
  })

  rows[#rows + 1] = selectRow({
    label = noctalia.tr("panel.framerate"),
    values = { "30", "60", "120", "144" },
    labels = {
      noctalia.tr("settings.framerate.options.30"),
      noctalia.tr("settings.framerate.options.60"),
      noctalia.tr("settings.framerate.options.120"),
      noctalia.tr("settings.framerate.options.144"),
    },
    current = optionValue("framerate", "60"),
    enabled = not recording,
    onPick = function(v)
      sendCmd({ op = "setOption", key = "framerate", value = v })
    end,
  })

  rows[#rows + 1] = streamRow(not recording)

  return ui.column({ gap = 10 }, rows)
end

local function buildTree()
  local recording = isRecording()
  local selects = buildSelects(recording)
  local settingsBtn = settingsButton()

  -- 状态行（仅录制中显示）：红点(error 色) + 「录制中」+ 计时 + 音频图标(mic/speaker)
  if recording then
    local a = optionValue("audioSource", "none")
    local statusChildren = {
      ui.label({ text = "●", color = ERROR_COLOR, fontSize = 14 }),
      ui.label({ text = noctalia.tr("panel.recording"), color = ERROR_COLOR, fontWeight = "bold" }),
      ui.label({ text = formatElapsed(elapsedSeconds()), color = ERROR_COLOR }),
    }
    if a == "mic" or a == "both" then
      statusChildren[#statusChildren + 1] = ui.glyph({ name = "microphone", color = ERROR_COLOR, size = 14 })
    elseif a == "desktop" then
      statusChildren[#statusChildren + 1] = ui.glyph({ name = "speaker", color = ERROR_COLOR, size = 14 })
    end

    -- 大按钮：录制中 = destructive「停止录制」
    return ui.column({ padding = 16, gap = 12 }, {
      ui.row({ gap = 10, align = "center" }, statusChildren),
      ui.button({
        text = noctalia.tr("panel.stopRecording"),
        variant = "destructive",
        contentAlign = "center",
        flexGrow = 1,
        onClick = function()
          sendCmd({ op = "stop" })
        end,
      }),
      selects,
      settingsBtn,
    })
  end

  -- 未录制：隐藏状态行；大按钮 = primary「开始录制」（target 由 service 解析，含 slurp 流程）
  return ui.column({ padding = 16, gap = 12 }, {
    ui.button({
      text = noctalia.tr("panel.startRecording"),
      variant = "primary",
      contentAlign = "center",
      flexGrow = 1,
      onClick = function()
        sendCmd({ op = "start" })
      end,
    }),
    selects,
    settingsBtn,
  })
end

-- ---------------------------------------------------------------- 渲染调度

local function refresh()
  panel.render(buildTree())
  -- 录制中请求秒级 tick 驱动 update() 刷新计时（面板关闭时宿主自动停止，onClose 再显式关一次）
  panel.setWantsSecondTicks(isRecording())
end

function update()
  -- 仅在录制中被 setWantsSecondTicks 触发；重渲染即可更新计时文本
  refresh()
end

function onOpen(context)
  refresh()
end

function onClose()
  panel.setWantsSecondTicks(false)
end

-- ---------------------------------------------------------------- state 订阅与初始化

state.watch("rec_state", function(json)
  recState = decodeSnapshot(json or "null")
  refresh()
end)

state.watch("rec_options", function(json)
  recOptions = decodeSnapshot(json or "null")
  refresh()
end)

-- 初始化恢复：watch 只响应变化，用同步 get 兜底（global-menu 模式）
local okS, vS = pcall(state.get, "rec_state")
if okS then recState = decodeSnapshot(vS) end
local okO, vO = pcall(state.get, "rec_options")
if okO then recOptions = decodeSnapshot(vO) end

refresh()
