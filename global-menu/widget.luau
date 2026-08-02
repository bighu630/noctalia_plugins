--!nonstrict
-- Global Menu 菜单栏条：渲染焦点应用顶层菜单；空时占位。

local state = noctalia.state
local menuData = nil -- {app=..., menu=..., source=...}
local bridgeInfo = nil

state.watch("menu", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  menuData = ok and v or nil
end)

state.watch("bridge", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  bridgeInfo = ok and v or nil
end)

local function httpPost(port, path, bodyTable, cb)
  if not port or port == 0 then return end
  noctalia.http({
    url = "http://127.0.0.1:" .. tostring(port) .. path,
    method = "POST",
    body = noctalia.json.encode(bodyTable),
  }, cb)
end

-- 点击顶层菜单项：拉最新子项 + 打开 popup 面板
local function onTopClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/open", { path = item.path }, function(ok2, status, body)
    if not ok2 then return end
    local ok3, resp = pcall(noctalia.json.decode, body or "{}")
    if not ok3 or not resp or not resp.ok then return end
    state.set("popup", noctalia.json.encode({
      title = item.label,
      parentPath = item.path,
      items = resp.children or {},
    }))
    noctalia.togglePanel("bighu630/global-menu:global_menu_popup")
  end)
end

function render()
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
