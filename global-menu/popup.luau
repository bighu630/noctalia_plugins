--!nonstrict
-- Global Menu 子菜单弹出面板：渲染子项、点击回传、Esc 关闭。

local state = noctalia.state
local popupData = nil -- {title=..., parentPath=..., items=...}
local bridgeInfo = nil

state.watch("popup", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  popupData = ok and v or nil
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

local function closePopup()
  noctalia.togglePanel("bighu630/global-menu:global_menu_popup")
end

-- 点击叶子项：POST /click {path}; 成功后桥自动重拉 → menu 事件 → 状态刷新
local function onItemClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/click", { path = item.path }, function(ok2, status, body)
    closePopup()
  end)
end

-- 点击带子菜单的项：递归展开（同面板替换）
local function onSubmenuClick(item)
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
  end)
end

function render()
  if not popupData then
    return ui.column({ padding = 8 }, {
      ui.label({ text = noctalia.tr("global_menu.no_menu"), color = "on_surface_variant" }),
    })
  end
  local rows = {}
  for _, item in ipairs(popupData.items or {}) do
    if item.type == "separator" then
      rows[#rows + 1] = ui.separator({})
    else
      local label = item.label
      if item.type == "checkbox" or item.type == "radio" then
        local mark = item.checked and "✓ " or "   "
        label = mark .. label
      end
      local hasChildren = item.type == "submenu" or (item.children and #item.children > 0)
      if hasChildren then
        label = label .. "  ▸"
      end
      rows[#rows + 1] = ui.button({
        text = label,
        enabled = item.enabled,
        onClick = function()
          if hasChildren then
            onSubmenuClick(item)
          else
            onItemClick(item)
          end
        end,
      })
    end
  end
  return ui.column({ gap = 2, paddingH = 6, paddingV = 6 }, rows)
end
