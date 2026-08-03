--!nonstrict
-- Global Menu 菜单栏条：渲染焦点应用顶层菜单；空时占位。
--
-- 渲染机制（Noctalia v5）：widget 是**命令式**渲染——调用 barWidget.render(ui树)
-- 推送 UI 树（不是定义 render() 函数等宿主调用）。state watch 变化时重建并推送。
--
-- 面板开关语义：Noctalia 仅暴露 togglePanel（严格切换，无可见性查询/关闭回调）。
-- 本插件面板配置 dismiss_on_outside_click=true：点击菜单栏按钮属于"面板外点击"，
-- 会先 dismiss 面板，随后 /open 回调里的单次 togglePanel 总是"从关到开"——
-- 因此无需状态机，点击任意顶层项 = 打开（或重开）面板并渲染新内容。
-- 已知行为：点击已打开的同一菜单项会先关闭再立即重开（内容刷新），不会保持关闭。

local state = noctalia.state
local POPUP_ID = "bighu630/global-menu:global_menu_popup"
local menuData = nil -- {app=..., menu=..., source=...}
local bridgeInfo = nil

-- 前置声明（Lua 词法作用域：local function 不 forward-reference，闭包需在定义后创建）
local onTopClick

local function buildTree()
  if not menuData or not menuData.menu then
    local appName = menuData and menuData.app and menuData.app.app_id or ""
    if appName == "" then appName = noctalia.tr("global_menu.placeholder") end
    return ui.row({ gap = 6, paddingH = 8, paddingV = 2 }, {
      ui.label({ text = appName, color = "on_surface_variant" }),
    })
  end
  local buttons = {}
  for _, item in ipairs(menuData.menu.children or {}) do
    if item.type == "separator" then
      buttons[#buttons + 1] = ui.separator({ orientation = "vertical" })
    elseif item.type == "submenu" or item.type == "item" then
      buttons[#buttons + 1] = ui.button({
        text = item.label,
        enabled = item.enabled,
        onClick = function() onTopClick(item) end,
      })
    end
  end
  return ui.row({ gap = 2, paddingH = 8, paddingV = 2 }, buttons)
end

local function refresh()
  barWidget.render(buildTree())
end

state.watch("gm_menu", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  menuData = ok and v or nil
  refresh()
end)

state.watch("gm_bridge", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  bridgeInfo = ok and v or nil
  refresh()
end)

-- 初始化恢复：watch 只响应变化（热重载/新实例拿不到当前值），用同步 get 兜底
local okG, vG = pcall(noctalia.state.get, "gm_bridge")
if okG and vG then
  local okD, vD = pcall(noctalia.json.decode, vG)
  if okD and vD then bridgeInfo = vD end
end
local okG2, vG2 = pcall(noctalia.state.get, "gm_menu")
if okG2 and vG2 then
  local okD2, vD2 = pcall(noctalia.json.decode, vG2)
  if okD2 and vD2 then menuData = vD2 end
end

local function httpPost(port, path, bodyTable, cb)
  if not port or port == 0 then return end
  noctalia.http({
    url = "http://127.0.0.1:" .. tostring(port) .. path,
    method = "POST",
    body = noctalia.json.encode(bodyTable),
  }, cb)
end

-- 点击顶层菜单项：拉最新子项 + 打开 popup 面板（外部 dismiss 已先行关闭 → 单 toggle 即打开）
function onTopClick(item)
  noctalia.log("GM-CLICK: " .. tostring(item.label) .. " port=" .. tostring(bridgeInfo and bridgeInfo.port or "nil"))
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/open", { path = item.path }, function(resp)
    if not resp or not resp.ok then
      noctalia.log("GM-CLICK /open failed: " .. tostring(resp and resp.ok))
      return
    end
    local ok3, data = pcall(noctalia.json.decode, resp.body or "{}")
    if not ok3 or not data or not data.ok then return end
    state.set("gm_popup", noctalia.json.encode({
      title = item.label,
      parentPath = item.path,
      items = data.children or {},
    }))
    noctalia.togglePanel(POPUP_ID)
  end)
end

-- 初始渲染（桥可能尚未就绪：menuData 为 nil → 占位；hello/menu 到达后自动刷新）
refresh()
