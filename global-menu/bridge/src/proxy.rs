//! 会话管理：焦点信息 → 菜单树解析 → 事件组装。
//!
//! 桥无状态：每次焦点变化/点击/展开都从 niri + AT-SPI（或 DBusMenu）重新解析。
//! id 语义 = DFS 解析序；path 语义 = 相对 menubar 的 child-index 链（点击/展开提交 path）。
//!
//! 菜单来源双管线：
//! - "atspi"：AT-SPI 基座（GTK/Qt 原生菜单，Wayland/X11 通用）
//! - "dbusmenu"：Registrar 注册的 com.canonical.dbusmenu 菜单（Electron/Chromium
//!   X11 模式、KDE 应用）；AT-SPI 解析不到时回退
//! - "none"：无菜单

use crate::atspi::{build_menu_tree, is_visible, AtspiClient, RawNode};
use crate::dbusmenu::{children_from_raw, event as dbusmenu_event, fetch_layout, locate_raw_by_path, raw_to_menu_item};
use crate::protocol::{AppInfo, BridgeEvent, MenuItem};
use crate::registrar::RegistrarHandle;
use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct FocusInfo {
    pub win_id: u64,
    pub app_id: String,
    pub title: String,
    pub pid: u32,
}

/// 跨线程共享的桥上下文（HTTP 线程 clone；主循环持有）。
#[derive(Clone)]
pub struct ProxyCtx {
    /// 会话总线连接（dbusmenu 调用用；registrar 接口持有同一条连接的 clone）。
    pub conn: zbus::blocking::Connection,
    pub registrar: RegistrarHandle,
}

/// 焦点变化处理：解析菜单并产出事件。返回 (菜单树, 来源)。
/// 优先 AT-SPI；无 menubar 时回退 Registrar → DBusMenu。
pub fn resolve_focus(
    atspi: &AtspiClient,
    ctx: &ProxyCtx,
    focus: &FocusInfo,
) -> Result<(Option<MenuItem>, &'static str)> {
    if let Some(raw) = atspi.fetch_menubar(focus.pid, &focus.title)? {
        let mut ids = 0u32;
        let tree = build_menu_tree(&raw, &mut ids);
        // 只下发顶层：完整树可能超过 runStream 单行 64KB 上限（实测 GIMP ~78KB 被整行丢弃）。
        // 子菜单内容一律由 HTTP /open 懒加载（响应无行大小限制）。
        return Ok((Some(prune_to_top_level(tree)), "atspi"));
    }
    // DBusMenu 回退：查 Registrar（按焦点 pid / X11 窗口类 / comm 匹配）
    let Some(reg) = ctx.registrar.lock().unwrap().find_for_focus(focus.pid, &focus.app_id) else {
        eprintln!("[global-menu-bridge] dbusmenu fallback: no reg for pid={} app_id={}", focus.pid, focus.app_id);
        return Ok((None, "none"));
    };
    eprintln!("[global-menu-bridge] dbusmenu fallback: matched xid={} bus={} path={} (pid={})", reg.xid, reg.bus, reg.path, reg.pid);
    let root = fetch_layout(&ctx.conn, &reg.bus, &reg.path)?;
    let mut ids = 0u32;
    let tree = raw_to_menu_item(&root, &[], &mut ids);
    Ok((Some(prune_to_top_level(tree)), "dbusmenu"))
}

/// 裁剪为仅顶层（保留顶层 type/label/enabled，清空所有 children）。
/// 顶层项是否含子菜单由 type=="submenu" 表达，内容经 /open 获取。
pub fn prune_to_top_level(mut root: MenuItem) -> MenuItem {
    for child in &mut root.children {
        child.children.clear();
    }
    root
}

pub fn make_menu_event(focus: &FocusInfo, menu: Option<MenuItem>, source: &'static str) -> BridgeEvent {
    BridgeEvent::Menu {
        app: AppInfo {
            app_id: focus.app_id.clone(),
            title: focus.title.clone(),
            pid: focus.pid,
        },
        menu,
        source,
    }
}

/// /open 响应体。
#[derive(serde::Serialize)]
pub struct ChildrenResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<MenuItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn build_children_response(ok: bool, children: Vec<MenuItem>) -> ChildrenResponse {
    ChildrenResponse { ok, children, error: None }
}

/// /click 响应体。
#[derive(serde::Serialize)]
pub struct ClickResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 点击：按 source 分派。
/// - atspi：完整重解析 + path 定位 + DoAction（返回是否找到/执行）
/// - dbusmenu：重查 Registrar + 重拉布局 + path 定位 + Event(id, "clicked")
pub fn click_path(
    source: &'static str,
    atspi: &AtspiClient,
    ctx: &ProxyCtx,
    focus: &FocusInfo,
    path: &[u32],
) -> Result<(bool, bool)> {
    if source == "dbusmenu" {
        return dbusmenu_click(ctx, focus, path);
    }
    atspi.click_path(focus.pid, &focus.title, path)
}

/// 展开：按 source 分派（懒构建兜底，/open 用）。
/// - atspi：重解析 + path 定位 + 读子项（id 与主树同一空间）
/// - dbusmenu：重拉布局 + path 定位 + 转换子项（同 id 空间规则）
pub fn open_path(
    source: &'static str,
    atspi: &AtspiClient,
    ctx: &ProxyCtx,
    focus: &FocusInfo,
    path: &[u32],
) -> Result<(bool, Vec<MenuItem>)> {
    if source == "dbusmenu" {
        return dbusmenu_open(ctx, focus, path);
    }
    let (found, raws) = atspi.read_children_by_path(focus.pid, &focus.title, path)?;
    if !found {
        return Ok((false, vec![]));
    }
    Ok((true, build_children(&raws, path)))
}

// ── DBusMenu 分派实现 ────────────────────────────────────────

/// 点击目标：按焦点查注册 → 重拉布局 → path 定位 → 取 DBusMenu id → Event。
fn dbusmenu_click(ctx: &ProxyCtx, focus: &FocusInfo, path: &[u32]) -> Result<(bool, bool)> {
    let reg = match ctx.registrar.lock().unwrap().find_for_focus(focus.pid, &focus.app_id) {
        Some(reg) => reg,
        None => return Ok((false, false)),
    };
    let root = fetch_layout(&ctx.conn, &reg.bus, &reg.path)?;
    let Some(target) = locate_raw_by_path(&root, path) else {
        return Ok((false, false));
    };
    dbusmenu_event(&ctx.conn, &reg.bus, &reg.path, target.id)?;
    Ok((true, true))
}

/// 展开子菜单：重拉布局 → path 定位 → 子项转换（与主树同形同 id 空间）。
fn dbusmenu_open(ctx: &ProxyCtx, focus: &FocusInfo, path: &[u32]) -> Result<(bool, Vec<MenuItem>)> {
    let reg = match ctx.registrar.lock().unwrap().find_for_focus(focus.pid, &focus.app_id) {
        Some(reg) => reg,
        None => return Ok((false, vec![])),
    };
    let root = fetch_layout(&ctx.conn, &reg.bus, &reg.path)?;
    let Some(target) = locate_raw_by_path(&root, path) else {
        return Ok((false, vec![]));
    };
    Ok((true, children_from_raw(&target.children, path)))
}

/// 由目标节点的直接子项 RawNode 列表构建 /open 响应（与主树同形、同 id 空间）。
/// 子项 path = 父 path + 原始 child-index（点击/再展开可直接提交）。
/// 独立成纯函数便于单测锁定 path 语义（B2 回归：曾把父 path 原样传给每个子项，
/// 导致 /click 定位到父项本身、/open 死循环）。
/// 可见性策略与主树一致：build_item 只过滤"自己的 children"，对传入节点本身
/// 不做可见性判断，因此这里补上 build_menu_tree 同款的顶层 is_visible 过滤
/// （不可见项不占 id；i 仍取原始索引，路径槽位与主树一致）。
pub fn build_children(raws: &[RawNode], parent_path: &[u32]) -> Vec<MenuItem> {
    let mut ids = 0u32;
    let mut items = Vec::new();
    for (i, raw) in raws.iter().enumerate() {
        if !is_visible(raw) {
            continue;
        }
        let mut p = parent_path.to_vec();
        p.push(i as u32);
        items.push(crate::atspi::build_item_pub(raw, &p, &mut ids));
    }
    items
}

/// 跨线程共享状态（proxy 主循环写，HTTP 线程读）。
#[derive(Default)]
pub struct Shared {
    /// 当前焦点窗口（主循环在 WindowsChanged/焦点处理时持续更新）。
    /// HTTP 线程点击/展开用它而非 session：菜单全树解析耗时数秒，
    /// 期间 session 可能还是旧焦点（实测竞态）。
    pub focus: Option<FocusInfo>,
    /// 当前焦点对应的菜单来源（"atspi"/"dbusmenu"/"none"；解析完成前为 None，
    /// HTTP 侧回退 atspi 分派，与旧行为一致）。
    pub source: Option<&'static str>,
    pub session: Option<Session>,
}

#[derive(Clone)]
pub struct Session {
    pub focus: FocusInfo,
    pub menu: Option<MenuItem>,
    pub source: &'static str,
}

