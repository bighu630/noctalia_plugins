--!nonstrict
-- Global Menu 子菜单弹出面板：渲染子项、点击回传、外部点击/Esc 关闭（面板宿主行为）。
--
-- 渲染机制（Noctalia v5）：面板是**命令式**渲染——调用 panel.render(ui树) 推送 UI 树。
-- 面板开关：widget.luau 是唯一开关方（点击顶层 → /open → 更新 gm_popup → togglePanel）。
-- 本模块只负责渲染内容与回传点击；点击叶子成功后调用 panel.close() 关闭面板
-- （桥随后自动重拉 → 补发 menu 事件 → widget 刷新菜单栏勾选状态）。

local state = noctalia.state
local popupData = nil -- {title=..., parentPath=..., items=...}
local bridgeInfo = nil

-- 前置声明（Lua 词法作用域：local function 不 forward-reference）
local onItemClick, onSubmenuClick

local function buildTree()
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

local function refresh()
  panel.render(buildTree())
end

state.watch("gm_popup", function(json)
  local ok, v = pcall(noctalia.json.decode, json or "null")
  popupData = ok and v or nil
  refresh()
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

-- 点击叶子项：POST /click {path}; 成功后桥自动重拉 → 补发 menu 事件 → widget 刷新。
-- 面板随后关闭（panel.close）。
function onItemClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/click", { path = item.path }, function()
    panel.close()
  end)
end

-- 点击带子菜单的项：递归展开（同面板替换内容）
function onSubmenuClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/open", { path = item.path }, function(ok2, status, body)
    if not ok2 then return end
    local ok3, resp = pcall(noctalia.json.decode, body or "{}")
    if not ok3 or not resp or not resp.ok then return end
    state.set("gm_popup", noctalia.json.encode({
      title = item.label,
      parentPath = item.path,
      items = resp.children or {},
    }))
  end)
end

-- 初始渲染（面板打开前内容可能已就绪）
refresh()
