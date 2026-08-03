//! 会话管理：焦点信息 → 菜单树解析 → 事件组装。
//!
//! 桥无状态：每次焦点变化/点击/展开都从 niri + AT-SPI 重新解析。
//! id 语义 = DFS 解析序；path 语义 = 相对 menubar 的 child-index 链（点击/展开提交 path）。

use crate::atspi::{build_menu_tree, is_visible, AtspiClient, RawNode};
use crate::protocol::{AppInfo, BridgeEvent, MenuItem};
use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct FocusInfo {
    pub win_id: u64,
    pub app_id: String,
    pub title: String,
    pub pid: u32,
}

/// 焦点变化处理：解析菜单并产出事件。menu=None 表示无菜单（回退占位）。
pub fn resolve_focus(atspi: &AtspiClient, focus: &FocusInfo) -> Result<Option<MenuItem>> {
    let Some(raw) = atspi.fetch_menubar(focus.pid, &focus.title)? else {
        return Ok(None);
    };
    let mut ids = 0u32;
    let tree = build_menu_tree(&raw, &mut ids);
    // 只下发顶层：完整树可能超过 runStream 单行 64KB 上限（实测 GIMP ~78KB 被整行丢弃）。
    // 子菜单内容一律由 HTTP /open 懒加载（响应无行大小限制）。
    Ok(Some(prune_to_top_level(tree)))
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

/// 点击：完整重解析 + path 定位 + DoAction（返回是否找到/执行）。
pub fn click_path(atspi: &AtspiClient, focus: &FocusInfo, path: &[u32]) -> Result<(bool, bool)> {
    atspi.click_path(focus.pid, &focus.title, path)
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

/// 展开兜底：重解析 + path 定位 + 读子项，转统一模型（id 空间与一次全新解析一致）。
pub fn open_path(atspi: &AtspiClient, focus: &FocusInfo, path: &[u32]) -> Result<(bool, Vec<MenuItem>)> {
    let (found, raws) = atspi.read_children_by_path(focus.pid, &focus.title, path)?;
    if !found {
        return Ok((false, vec![]));
    }
    Ok((true, build_children(&raws, path)))
}

/// 跨线程共享状态（proxy 主循环写，HTTP 线程读）。
#[derive(Default)]
pub struct Shared {
    /// 当前焦点窗口（主循环在 WindowsChanged/焦点处理时持续更新）。
    /// HTTP 线程点击/展开用它而非 session：菜单全树解析耗时数秒，
    /// 期间 session 可能还是旧焦点（实测竞态）。
    pub focus: Option<FocusInfo>,
    pub session: Option<Session>,
}

#[derive(Clone)]
pub struct Session {
    pub focus: FocusInfo,
    pub menu: Option<MenuItem>,
}
