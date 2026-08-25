--!nonstrict
-- nc-screen-recorder v5 service：进程与状态机核心（唯一写状态者）。
--
-- 职责：
--   * 启动恢复：读 session.json，按 pid 存活性决定接管录制态或清场回 idle。
--   * 选项解析与持久化：overrides.json ?? noctalia.getConfig(key) ?? 默认值，
--     经 rec_cmd {op="setOption"} 写 overrides.json 并重广播 rec_options。
--   * 命令消费：op=start/stop/setOption；region 目标先跑 slurp 选区。
--   * 录制进程托管：bash 包装脚本记录 pid 到 recorder.pid，
--     经 runStream 运行并解析 __GSR_EXIT__: 前缀收尾。
--   * 状态广播：rec_state / rec_options（JSON 字符串）。

local state = noctalia.state

local dataDir = noctalia.pluginDataDir()
local PID_FILE = dataDir .. "/recorder.pid"
local SESSION_FILE = dataDir .. "/session.json"
local OVERRIDES_FILE = dataDir .. "/overrides.json"

-- 生效选项的兜底默认值（与 plugin.toml 设置 schema 的 default 一致）
local DEFAULTS = {
  audioSource = "none",
  target = "fullscreen",       -- region | fullscreen | <显示器名>
  codec = "auto",              -- auto | h264 | hevc | av1 | vp9
  quality = "high",            -- medium | high | very_high | ultra
  framerate = "60",
  streamDestination = "",
  videoFormat = "mp4",
  -- 文件模式相关（不进 rec_options 快照，但同样走三级解析）
  saveDirectory = "~/Videos",
  filePattern = "recording_{datetime}",
}

-- rec_options 快照广播的键（契约 §1 固定集合，含 videoFormat 以同步面板显式选择）
local OPTION_KEYS = { "audioSource", "target", "codec", "quality", "framerate", "streamDestination", "videoFormat" }

-- 进程内状态
local recActive = false          -- 已确认 pid、处于录制中
local prelaunch = false          -- 已受理 start、正在 slurp/mkdir，尚未拉起 gsr
local starting = false           -- 已发起启动（runStream 成功）、尚未确认 pid
local pendingStop = false        -- starting 态下收到停止请求、pidfile 尚未落地（M2）
local launchTime = 0             -- 启动时间戳（用于 pid 确认超时）
local PID_CONFIRM_TIMEOUT_MS = 10000
local session = { startTime = 0, file = "", streaming = false }
-- GIF 后处理挂起项：fmt=="gif" 时先录 <name>.mp4，结束后经 ffmpeg 调色板管线
-- 转成 <name>.gif。{ tmp=中间mp4路径, final=最终gif路径 }；仅文件模式非空。
local gifPending = nil

-- ---------------------------------------------------------------- 工具函数

local function nowMs()
  return os.time() * 1000
end

local function trim(s)
  return noctalia.string.trim(tostring(s or ""))
end

local function decodeJson(line)
  if type(line) ~= "string" or line == "" or line == "null" then return nil end
  local ok, v = pcall(noctalia.json.decode, line)
  if ok and type(v) == "table" then return v end
  return nil
end

-- 单引号包装 + 转义（用于 bash -c 包装脚本中的用户字符串）
local function shQuote(s)
  return "'" .. tostring(s or ""):gsub("'", "'\\''") .. "'"
end

local function expandTilde(p)
  if type(p) ~= "string" then return "" end
  return p:gsub("^~", noctalia.getenv("HOME") or "/")
end

local function removeFilesAsync(...)
  noctalia.runAsync({ "rm", "-f", ... })
end

-- ---------------------------------------------------------------- 选项解析

local function loadOverrides()
  local raw = noctalia.readFile(OVERRIDES_FILE)
  if type(raw) ~= "string" or raw == "" then return {} end
  local v = decodeJson(raw)
  return v or {}
end

-- 生效值 = overrides.json ?? noctalia.getConfig(key) ?? 默认值（§3）
local function effective(key)
  local o = loadOverrides()
  if o[key] ~= nil then return o[key] end
  local cfg = noctalia.getConfig(key)
  if cfg ~= nil then return cfg end
  return DEFAULTS[key]
end

local function broadcastOptions()
  local snap = {}
  for _, k in ipairs(OPTION_KEYS) do
    snap[k] = tostring(effective(k) or "")
  end
  state.set("rec_options", noctalia.json.encode(snap))
end

local function persistOption(key, value)
  local o = loadOverrides()
  o[key] = value
  noctalia.writeFile(OVERRIDES_FILE, noctalia.json.encode(o))
  broadcastOptions()
end

-- ---------------------------------------------------------------- 状态广播

local function broadcastState(isRecording, startTime, file, streaming)
  state.set("rec_state", noctalia.json.encode({
    isRecording = isRecording,
    startTime = startTime or 0,
    file = file or "",
    streaming = streaming or false,
  }))
end

local function broadcastIdle()
  broadcastState(false, 0, "", false)
end

local function clearSessionFiles()
  removeFilesAsync(SESSION_FILE, PID_FILE)
end

-- ---------------------------------------------------------------- 收尾

-- GIF 两遍调色板转换（后处理，不阻塞状态复位）：成功删中间 mp4 并发保存通知，
-- 失败保留 mp4 以免丢录像。
local function convertToGif(pending)
  noctalia.notify(noctalia.tr("notification.convertingToGif"))
  local cmd = "ffmpeg -y -v error -i " .. shQuote(pending.tmp)
    .. " -vf \"fps=12,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse\" -loop 0 "
    .. shQuote(pending.final)
  -- 长视频转换可能耗时较久：显式给 600s 超时（默认超时会中途杀掉 ffmpeg）
  noctalia.runAsync("bash -c " .. shQuote(cmd), function(result)
    if result and result.exitCode == 0 and not result.timedOut then
      removeFilesAsync(pending.tmp)
      noctalia.notify(noctalia.tr("notification.recordingSavedWithPath", { file = pending.final }))
    else
      noctalia.notifyError(noctalia.tr("notification.gifConvertFailed"))
    end
  end, 600000)
end

-- 退出码 0 / 255（SIGINT 优雅结束）视为成功；否则失败通知。
local function finishRecording(exitCode)
  local wasStreaming = session.streaming
  local filePath = session.file
  local pendingGif = gifPending
  starting = false
  recActive = false
  prelaunch = false -- 防泄漏（正常路径下已为 false）
  pendingStop = false
  session = { startTime = 0, file = "", streaming = false }
  clearSessionFiles()
  broadcastIdle()

  if exitCode == 0 or exitCode == 255 then
    if wasStreaming then
      noctalia.notify(noctalia.tr("notification.streamingStopped"))
    elseif filePath ~= "" then
      if pendingGif and pendingGif.tmp and pendingGif.final then
        convertToGif(pendingGif)
      else
        noctalia.notify(noctalia.tr("notification.recordingSavedWithPath", { file = filePath }))
      end
    end
  else
    noctalia.notifyError(noctalia.tr("notification.recordingFailed", { code = tostring(exitCode) }))
  end
  -- 无论成败均复位挂起项（转换回调持有局部引用，不受影响）
  gifPending = nil
end

local function handleGsrLine(line)
  if type(line) ~= "string" then return end
  local codeStr = line:match("^__GSR_EXIT__:(%-?%d+)")
  if codeStr then
    finishRecording(tonumber(codeStr) or -1)
  else
    noctalia.log("gsr: " .. line)
  end
end

-- ---------------------------------------------------------------- 启动录制

local function buildFilePath()
  local dir = expandTilde(tostring(effective("saveDirectory") or ""))
  if dir == "" then dir = expandTilde(DEFAULTS.saveDirectory) end
  local pattern = tostring(effective("filePattern") or DEFAULTS.filePattern)
  local name = pattern:gsub("{datetime}", os.date("%Y-%m-%d_%H-%M-%S"))
  local fmt = tostring(effective("videoFormat") or DEFAULTS.videoFormat)
  if fmt == "" then fmt = DEFAULTS.videoFormat end
  return dir .. "/" .. name .. "." .. fmt
end

-- 组装 gpu-screen-recorder 参数（argv 数组，随后经 shQuote 进入 bash 包装）
local function buildGsrArgs(windowArg, streaming, filePath, streamUrl)
  local args = { "gpu-screen-recorder" }
  if streaming then
    args[#args + 1] = "-o"
    args[#args + 1] = streamUrl
    args[#args + 1] = "-c"
    args[#args + 1] = "flv"
  else
    args[#args + 1] = "-o"
    args[#args + 1] = filePath
  end
  args[#args + 1] = "-w"
  args[#args + 1] = windowArg
  args[#args + 1] = "-f"
  args[#args + 1] = tostring(effective("framerate") or DEFAULTS.framerate)
  local codec = tostring(effective("codec") or DEFAULTS.codec)
  if codec ~= "" and codec ~= "auto" then
    args[#args + 1] = "-k"
    args[#args + 1] = codec
  end
  args[#args + 1] = "-q"
  args[#args + 1] = tostring(effective("quality") or DEFAULTS.quality)
  local audio = tostring(effective("audioSource") or DEFAULTS.audioSource)
  if audio == "mic" then
    args[#args + 1] = "-a"
    args[#args + 1] = "default_input"
  elseif audio == "desktop" then
    args[#args + 1] = "-a"
    args[#args + 1] = "default_output"
  elseif audio == "both" then
    args[#args + 1] = "-a"
    args[#args + 1] = "default_output|default_input"
  end
  return args
end

-- 包装脚本严格按 §2：后台运行 gsr → 记录 pid → wait → 输出退出码哨兵行
local function buildWrapper(args)
  local parts = {}
  for _, a in ipairs(args) do
    parts[#parts + 1] = shQuote(a)
  end
  return table.concat(parts, " ")
    .. " & pid=$!; echo $pid > " .. shQuote(PID_FILE)
    .. "; wait $pid; echo \"__GSR_EXIT__:$?\""
end

local function launchRecorder(windowArg)
  -- B1：streamDestination 存的是目标名（key），真实推流 URL 在
  -- streamDestinations(string_map) 中。key 为空或 map 无该 key → url=""。
  local destKey = trim(effective("streamDestination") or "")
  local streamUrl = ""
  if destKey ~= "" then
    local map = effective("streamDestinations")
    if type(map) == "table" and type(map[destKey]) == "string" then
      streamUrl = trim(map[destKey])
    end
  end
  -- B2：由解出的 streamUrl 是否非空自动决定推流/文件模式（service 侧推断），
  -- 不再依赖 cmd.streaming，杜绝「选了推流目标仍走文件模式」。
  local streaming = streamUrl ~= ""
  local filePath = streaming and "" or buildFilePath()
  -- GIF 模式：gsr 不支持 gif 容器，先录到同名 .mp4 临时文件，收尾后再转 .gif。
  -- session/broadcast 始终面向最终 gif 路径，仅传给 gsr 的输出换成临时 mp4。
  gifPending = nil
  local finalFile = filePath
  if not streaming and tostring(effective("videoFormat") or DEFAULTS.videoFormat) == "gif" then
    local tmpFile = filePath:gsub("%.gif$", ".mp4")
    gifPending = { tmp = tmpFile, final = filePath }
    finalFile = filePath
    filePath = tmpFile
  end
  local args = buildGsrArgs(windowArg, streaming, filePath, streamUrl)

  -- 文件模式先确保目录存在（§2）
  local function doLaunch()
    -- runStream 仅接受字符串命令（argv 表形式仅 runAsync 支持）；
    -- wrapper 含用户字符串，经 shQuote 安全嵌入 bash -c
    local ok = noctalia.runStream("bash -c " .. shQuote(buildWrapper(args)), handleGsrLine)
    if not ok then
      prelaunch = false
      starting = false
      gifPending = nil
      noctalia.notifyError(noctalia.tr("notification.startFailed"))
      return
    end
    -- pid 由包装脚本异步写入，等待 update() 心跳确认后再写 session.json
    prelaunch = false
    starting = true
    launchTime = nowMs()
    session = {
      startTime = nowMs(),
      file = streaming and "" or finalFile,
      streaming = streaming,
    }
  end

  if not streaming then
    local dir = filePath:match("^(.*)/[^/]+$") or "."
    noctalia.runAsync({ "mkdir", "-p", dir }, function(result)
      if not result or result.exitCode ~= 0 then
        prelaunch = false
        starting = false
        pendingStop = false
        gifPending = nil
        noctalia.notifyError(noctalia.tr("notification.failedToCreateDirectory"))
        return
      end
      doLaunch()
    end)
  else
    doLaunch()
  end
end

local function startRecording(cmd)
  if recActive or starting or prelaunch then return end
  prelaunch = true -- 受理启动；slurp/mkdir 期间不进入 starting 态，update() 超时清场不可达
  -- 注意：starting 留待 doLaunch() 成功时才置 true（同时设 launchTime），
  -- 否则 slurp 期间心跳会因 launchTime=0 误触超时清场。

  local target = tostring(cmd.target or effective("target") or DEFAULTS.target)

  if target == "region" then
    -- slurp 选区：空输出 / 非零退出 → 视为取消（非错误）。
    -- 第三参显式给长超时（120s）：runAsync 默认超时会在用户还在拖框选区时
    -- 杀掉 slurp，表现为「选到一半被取消」。
    noctalia.runAsync({ "slurp", "-f", "%wx%h+%x+%y" }, function(result)
      local geom = result and trim(result.stdout) or ""
      if not result or result.timedOut or result.exitCode ~= 0 or geom == "" then
        prelaunch = false
        starting = false
        pendingStop = false -- 复位：slurp 期间取消不应把停止请求残留到下次启动（复审建议）
        noctalia.notify(noctalia.tr("notification.regionSelectionCancelled"))
        return
      end
      launchRecorder(geom)
    end, 120000) -- 选区超时 120s，可按需调大
    return
  end

  -- fullscreen → "-w screen"；其他值视为显示器名 "-w <name>"
  local windowArg = (target == "fullscreen") and "screen" or target
  launchRecorder(windowArg)
end

-- ---------------------------------------------------------------- 停止录制

local function confirmDeadCleanup()
  starting = false
  recActive = false
  prelaunch = false -- 防泄漏（正常路径下已为 false）
  pendingStop = false
  session = { startTime = 0, file = "", streaming = false }
  clearSessionFiles()
  broadcastIdle()
  noctalia.notify(noctalia.tr("notification.notRecording"))
end

-- 注：prelaunch 阶段（slurp/mkdir 进行中）面板仍显示 Start 按钮、rec_state 为 idle，
-- stopRecording 在该阶段不可达，故无需处理 prelaunch。
local function stopRecording()
  if not recActive and not starting then
    confirmDeadCleanup()
    return
  end
  local raw = noctalia.readFile(PID_FILE)
  local pid = raw and tonumber(trim(raw)) or nil
  if not pid then
    if starting then
      -- M2：starting 态下 pidfile 尚未落地——不能立即清场广播 idle，
      -- 否则会与随后 update() 轮询到 pid 的 adopt 竞态造成状态闪烁。
      -- 挂起停止标记，保留 starting 态，待 update() 轮询到 pid 后立即 kill -INT 收尾。
      pendingStop = true
      return
    end
    confirmDeadCleanup()
    return
  end
  noctalia.runAsync({ "kill", "-0", tostring(pid) }, function(result)
    if result and result.exitCode == 0 then
      noctalia.runAsync({ "kill", "-INT", tostring(pid) })
    else
      confirmDeadCleanup()
    end
  end)
end

-- ---------------------------------------------------------------- 命令消费

local function handleCommand(jsonStr)
  if type(jsonStr) ~= "string" or jsonStr == "" or jsonStr == "null" then return end
  local cmd = decodeJson(jsonStr)
  if not cmd or type(cmd.op) ~= "string" then return end

  -- 消费完立即复位，防重复消费（契约 §1）
  state.set("rec_cmd", "null")

  if cmd.op == "start" then
    startRecording(cmd)
  elseif cmd.op == "stop" then
    stopRecording()
  elseif cmd.op == "setOption" then
    if type(cmd.key) == "string" and cmd.key ~= "" and cmd.value ~= nil then
      persistOption(cmd.key, cmd.value)
    end
  end
end

-- ---------------------------------------------------------------- 启动恢复

local function adoptSession(s)
  recActive = true
  starting = false
  session = {
    startTime = tonumber(s.startTime) or nowMs(),
    file = tostring(s.file or ""),
    streaming = s.streaming == true,
  }
  broadcastState(true, session.startTime, session.file, session.streaming)
end

local function recoverOnStartup()
  -- 无论何种情况，启动即广播一次 rec_state 与 rec_options（§3/§4）
  broadcastOptions()

  local raw = noctalia.readFile(SESSION_FILE)
  local s = raw and decodeJson(raw) or nil
  local pid = s and tonumber(s.pid) or nil

  if pid and pid > 0 then
    -- 以 pid 存活为准：活 → 接管为录制中（沿用原 startTime）；绝不凭持久化字段直接显示录制中。
    -- 回调里收尾，避免误删存活录制进程的 pidfile。
    noctalia.runAsync({ "kill", "-0", tostring(pid) }, function(result)
      if result and result.exitCode == 0 then
        adoptSession(s)
      else
        clearSessionFiles()
        broadcastIdle()
      end
    end)
  else
    clearSessionFiles()
    broadcastIdle()
  end

  -- B3：显式启动心跳，保证 update() 运行——pidfile 轮询确认与
  -- PID_CONFIRM_TIMEOUT_MS 超时清场依赖此 tick。
  noctalia.setUpdateInterval(1000)
end

-- ---------------------------------------------------------------- 心跳

function update()
  -- 低频心跳：仅用于启动后确认 pidfile 出现；录制中不需要 service tick
  noctalia.setUpdateInterval(1000)

  if starting and not recActive then
    local raw = noctalia.readFile(PID_FILE)
    local pid = raw and tonumber(trim(raw)) or nil
    if pid then
      starting = false
      recActive = true
      if pendingStop then
        -- M2：启动确认期间用户已请求停止——不写 session.json、不广播录制态、
        -- 不发「已开始」通知，直接 SIGINT 走正常收尾（__GSR_EXIT__ → finishRecording）。
        pendingStop = false
        noctalia.runAsync({ "kill", "-INT", tostring(pid) })
        return
      end
      -- 写会话持久化（§4），供重启恢复判断 pid 存活
      noctalia.writeFile(SESSION_FILE, noctalia.json.encode({
        pid = pid,
        startTime = session.startTime,
        file = session.file,
        streaming = session.streaming,
      }))
      broadcastState(true, session.startTime, session.file, session.streaming)
      if session.streaming then
        noctalia.notify(noctalia.tr("notification.streamingStarted"))
      else
        noctalia.notify(noctalia.tr("notification.recordingStarted"))
      end
    elseif nowMs() - launchTime > PID_CONFIRM_TIMEOUT_MS then
      -- gsr 秒退且 __GSR_EXIT__ 行丢失：超时放弃，清场（含挂起的停止标记）
      prelaunch = false
      starting = false
      pendingStop = false
      gifPending = nil
      session = { startTime = 0, file = "", streaming = false }
      clearSessionFiles()
      broadcastIdle()
      noctalia.notifyError(noctalia.tr("notification.startFailed"))
    end
  end
end

-- ---------------------------------------------------------------- 入口

recoverOnStartup()
state.watch("rec_cmd", handleCommand)

-- 同步 get 兜底：watch 注册前若已有待处理命令（global-menu 模式）
local okCmd, vCmd = pcall(state.get, "rec_cmd")
if okCmd then handleCommand(vCmd) end
