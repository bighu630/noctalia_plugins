--!nonstrict
-- Global Menu 菜单栏条：渲染焦点应用顶层菜单；空时占位。
--
-- 面板开关语义（review 修复）：本模块是 global_menu_popup 的**唯一**开关方
-- （popup.luau 不再自行 toggle，避免与这里双 toggle 竞态）。Noctalia 仅暴露
-- togglePanel（无面板关闭回调/可见性查询），因此：
-- 1) 用 panelOpen 维护"面板应处于打开"的状态；
-- 2) 顶层点击时若已开 → 先 toggle 关、更新内容、再 toggle 开（切换而非关闭）；
-- 3) 收到新 menu 事件（焦点变化/点击后重拉/桥重启）→ 面板内容已陈旧 → 关闭并复位。
-- 已知退化：用户 Esc/外部点击关面板无法感知，panelOpen 会短暂失步；下次顶层点击
-- 的"先关后开"等效于刷新面板，不影响最终正确性。

local state = noctalia.state
local menuData = nil -- {app=..., menu=..., source=...}
local bridgeInfo = nil
local panelOpen = false

state.watch("gm_menu", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  menuData = ok and v or nil
  -- 新 menu 事件 → 旧弹出面板内容已失效（焦点已切换 / 点击已生效 / 桥重启）
  if panelOpen then
    panelOpen = false
    noctalia.togglePanel("bighu630/global-menu:global_menu_popup")
  end
end)

state.watch("gm_bridge", function(json)
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

-- 点击顶层菜单项：拉最新子项 + 打开/切换 popup 面板（切换而非关闭）
local function onTopClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/open", { path = item.path }, function(ok2, status, body)
    if not ok2 then return end
    local ok3, resp = pcall(noctalia.json.decode, body or "{}")
    if not ok3 or not resp or not resp.ok then return end
    if panelOpen then
      -- 面板已开：先关（再开=切换内容；只 toggle 一次会变成"关闭面板"）
      noctalia.togglePanel("bighu630/global-menu:global_menu_popup")
    end
    state.set("gm_popup", noctalia.json.encode({
      title = item.label,
      parentPath = item.path,
      items = resp.children or {},
    }))
    -- 再开：未开时即首次打开；已开时完成"关→开"的内容切换
    noctalia.togglePanel("bighu630/global-menu:global_menu_popup")
    panelOpen = true
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
