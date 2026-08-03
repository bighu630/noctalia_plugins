--!nonstrict
-- Global Menu 子菜单弹出面板（macOS 风格）：紧凑列表、左对齐、hover 展开二级菜单。
--
-- 交互：
-- - 点击叶子项 → POST /click {path} → 桥 DoAction/DBusMenu Event → panel.close()
-- - hover 带 ▸ 的项 → 自动 /open 拉子项 → 同面板替换（跟手：带请求序号防过期响应）
-- - 面板位置：floating + position=top + open_near_click（按钮下方/顶部下拉）

local state = noctalia.state
local popupData = nil -- {title=..., parentPath=..., items=...}
local bridgeInfo = nil
local hoverSeq = 0
local hoverPending = nil -- 当前展开的 path key（字符串），防重复请求

local function pathKey(path)
  local parts = {}
  if path then
    for _, p in ipairs(path) do parts[#parts + 1] = tostring(p) end
  end
  return table.concat(parts, ",")
end

local function buildTree()
  if not popupData then
    return ui.column({ padding = 8 }, {
      ui.label({ text = noctalia.tr("global_menu.no_menu"), color = "on_surface_variant" }),
    })
  end
  local rows = {}
  for _, item in ipairs(popupData.items or {}) do
    if item.type == "separator" then
      rows[#rows + 1] = ui.separator({ thickness = 1 })
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
        contentAlign = "start", -- 左对齐（macOS 菜单风格）
        controlSize = "sm", -- 紧凑
        onClick = function()
          if hasChildren then
            onSubmenuClick(item)
          else
            onItemClick(item)
          end
        end,
        -- hover 自动展开二级菜单（跟手）
        onHover = function()
          if hasChildren then
            onSubmenuHover(item)
          end
        end,
      })
    end
  end
  return ui.column({ gap = 1, paddingH = 4, paddingV = 4 }, rows)
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

-- 初始化恢复：watch 只响应变化，用同步 get 兜底
local okG, vG = pcall(noctalia.state.get, "gm_bridge")
if okG and vG then
  local okD, vD = pcall(noctalia.json.decode, vG)
  if okD and vD then bridgeInfo = vD end
end
local okG2, vG2 = pcall(noctalia.state.get, "gm_popup")
if okG2 and vG2 then
  local okD2, vD2 = pcall(noctalia.json.decode, vG2)
  if okD2 and vD2 then popupData = vD2 end
end

local function httpPost(port, path, bodyTable, cb)
  if not port or port == 0 then return end
  noctalia.http({
    url = "http://127.0.0.1:" .. tostring(port) .. path,
    method = "POST",
    body = noctalia.json.encode(bodyTable),
  }, cb)
end

local function updatePopup(item, data)
  state.set("gm_popup", noctalia.json.encode({
    title = item.label,
    parentPath = item.path,
    items = data.children or {},
  }))
end

-- 点击叶子项：POST /click {path}; 成功后桥自动重拉 → 补发 menu 事件 → widget 刷新。
local function onItemClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/click", { path = item.path }, function()
    panel.close()
  end)
end

-- 点击带子菜单的项：拉子项（同面板替换）
local function onSubmenuClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  hoverPending = pathKey(item.path)
  httpPost(bridgeInfo.port, "/open", { path = item.path }, function(resp)
    if not resp or not resp.ok then return end
    local ok3, data = pcall(noctalia.json.decode, resp.body or "{}")
    if not ok3 or not data or not data.ok then return end
    updatePopup(item, data)
  end)
end

-- hover 展开：带请求序号，快速移动时过期响应直接丢弃（跟手）
local function onSubmenuHover(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  local key = pathKey(item.path)
  if hoverPending == key then return end
  hoverPending = key
  local seq = hoverSeq + 1
  hoverSeq = seq
  httpPost(bridgeInfo.port, "/open", { path = item.path }, function(resp)
    if seq ~= hoverSeq then return end -- 过期响应
    if not resp or not resp.ok then return end
    local ok3, data = pcall(noctalia.json.decode, resp.body or "{}")
    if not ok3 or not data or not data.ok then return end
    updatePopup(item, data)
  end)
end

-- 初始渲染（面板打开前内容可能已就绪）
refresh()
