--!nonstrict
-- Global Menu bridge host: 托管桥进程、解析 NDJSON 事件、经 state 广播。
-- 桥由本 service 入口独占托管（widget/panel 只读 state）。

local state = noctalia.state
local bridgePath = noctalia.pluginDir() .. "/bridge-bin/noctalia-global-menu-bridge"
local bridgePort = nil
local restartDelayUntil = 0
local bridgeStarting = false
local bridgeStartTime = 0
local BRIDGE_HELLO_TIMEOUT_MS = 15000
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

local function clearMenu()
  -- 桥 down：清空菜单状态，widget 不显示陈旧菜单
  state.set("gm_menu", "null")
end

local function handleLine(line)
  local ev = decode(line)
  if not ev then return end
  if ev.type == "hello" then
    bridgePort = ev.port
    bridgeStarting = false
    state.set("gm_bridge", noctalia.json.encode({ port = ev.port, alive = true }))
    noctalia.log("GlobalMenu bridge up on port " .. tostring(ev.port))
  elseif ev.type == "menu" then
    state.set("gm_menu", noctalia.json.encode(ev))
  elseif ev.type == "error" then
    noctalia.log("GlobalMenu bridge: " .. tostring(ev.msg))
  end
end

local function startBridge()
  if nowMs() < restartDelayUntil then return end
  if bridgeStarting then
    -- 已启动等待 hello；超时（桥可能卡死/流断）才允许重启
    if nowMs() - bridgeStartTime > BRIDGE_HELLO_TIMEOUT_MS then
      bridgeStarting = false
    else
      return
    end
  end
  local ok = noctalia.runStream(bridgePath, handleLine)
  if ok then
    bridgeStarting = true
    bridgeStartTime = nowMs()
  else
    -- 常见原因：bridge-bin 未构建（noctalia-global-menu-bridge 缺失）
    noctalia.log("GlobalMenu: failed to start bridge（若 bridge-bin 缺失，请运行 scripts/build.sh 构建桥），" .. math.floor(RESTART_COOLDOWN_MS / 1000) .. "s 后重试")
    clearMenu()
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
      state.set("gm_bridge", noctalia.json.encode({ port = 0, alive = false }))
      clearMenu()
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
