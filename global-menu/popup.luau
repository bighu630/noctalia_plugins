--!nonstrict
-- Global Menu 子菜单弹出面板（macOS 风格）：紧凑列表、左对齐、点击展开、可返回父菜单。
--
-- 交互：
-- - 点击叶子项 → POST /click {path} → 桥执行（DoAction / DBusMenu Event）→ panel.close()
-- - 点击带 ▸ 的项 → 展开子菜单（不自动，点击才展开）
-- - 菜单栈：展开子菜单压栈，顶部「← 返回」按钮弹栈（用缓存 items，即时返回）
-- - 面板位置：floating + open_near_click（Noctalia 锚点补丁生效后跟随触发按钮）

local state = noctalia.state
local popupData = nil -- {title=..., path=..., items=...}（widget 点击顶层时写入，重置栈）
local bridgeInfo = nil
local hoverPending = nil -- 当前展开请求的 path key（防重复请求）
local hoverSeq = 0

-- 前置声明（Lua 词法作用域：local function 不 forward-reference，闭包需在定义后创建）
local onItemClick, onSubmenuClick, popSubmenu

-- 菜单栈：{ {title=..., path=..., items=...}, ... }；栈底=顶层菜单，栈顶=当前显示
local menuStack = {}

local function pathKey(path)
  local parts = {}
  if path then
    for _, p in ipairs(path) do parts[#parts + 1] = tostring(p) end
  end
  return table.concat(parts, ",")
end

local function topOfStack()
  return menuStack[#menuStack]
end

local function buildTree()
  local top = topOfStack()
  if not top then
    return ui.column({ padding = 8 }, {
      ui.label({ text = noctalia.tr("global_menu.no_menu"), color = "on_surface_variant" }),
    })
  end
  local rows = {}
  -- 返回父菜单按钮（栈深 > 1）
  if #menuStack > 1 then
    local parentTitle = menuStack[#menuStack - 1].title or "←"
    rows[#rows + 1] = ui.button({
      text = "← " .. parentTitle,
      contentAlign = "start",
      controlSize = "sm",
      variant = "primary",
      onClick = function() popSubmenu() end,
    })
    rows[#rows + 1] = ui.separator({ thickness = 1 })
  end
  for _, item in ipairs(top.items or {}) do
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
            onSubmenuClick(item) -- 点击带 ▸ 的项才展开
          else
            onItemClick(item)
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
  -- widget 点击了新的顶层菜单：重置菜单栈（栈底=新顶层）
  if popupData and popupData.path then
    menuStack = { { title = popupData.title, path = popupData.path, items = popupData.items or {} } }
    hoverPending = nil
  end
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
  if popupData and popupData.path then
    menuStack = { { title = popupData.title, path = popupData.path, items = popupData.items or {} } }
  end
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
  menuStack[#menuStack + 1] = { title = item.label, path = item.path, items = data.children or {} }
  refresh()
end

-- 展开子菜单：/open → 压栈 → 渲染（带请求序号，过期响应丢弃）
local function expandSubmenu(item)
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

-- 返回父菜单：弹栈 + 渲染缓存内容（即时）
function popSubmenu()
  if #menuStack > 1 then
    menuStack[#menuStack] = nil
    hoverPending = nil
    refresh()
  end
end

-- 点击叶子项：POST /click {path}; 成功后桥自动重拉 → 补发 menu 事件 → widget 刷新。
function onItemClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  httpPost(bridgeInfo.port, "/click", { path = item.path }, function()
    panel.close()
  end)
end

-- 点击带子菜单的项：立即展开（压栈）
function onSubmenuClick(item)
  if not bridgeInfo or not bridgeInfo.port or not item.path then return end
  expandSubmenu(item)
end

-- 初始渲染（面板打开前内容可能已就绪）
refresh()
