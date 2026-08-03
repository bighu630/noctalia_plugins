//! `com.canonical.dbusmenu` 客户端（blocking）：菜单树拉取、属性解析、点击回传。
//!
//! ## Wire 格式
//!
//! `GetLayout(parentId i32, recursionDepth i32, propertyNames as)`
//! 返回 `(revision u32, layout (ia{sv}av))`：
//!
//! - id: i32 — 应用侧稳定菜单项 id（点击回传用）
//! - props: a{sv} — 属性表。标准键：label(s，'_' 为助记符标记)、type(s，
//!   ""/"standard"/"separator"/"submenu")、enabled(b)、visible(b)、
//!   icon-name(s)、toggle-type(s，"checkmark"/"radio")、toggle-state(i，
//!   0=off 1=on -1=indeterminate)
//! - children: av — 变体数组，每个元素是嵌套的 (ia{sv}av)（递归解包，
//!   参考 ADR-0023 的 OwnedValue 递归 decode 做法）
//!
//! ## 拉取策略
//!
//! 每次焦点变化/点击/展开都 `GetLayout(0, -1, [])` 全量重拉（无缓存）——
//! 与 AT-SPI 的重解析策略一致（树变化时 path 定位始终对当前树生效）。

use crate::protocol::{MenuItem, MenuItemType};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use zbus::blocking::Connection;
use zvariant::OwnedValue;

const IFACE_DBUSMENU: &str = "com.canonical.dbusmenu";

/// GetLayout 返回的 wire 结构 `(ia{sv}av)`。
/// Value/OwnedValue derive 提供 OwnedValue ↔ MenuLayout 的递归互转。
#[derive(Debug, Deserialize, serde::Serialize, zvariant::Type, zvariant::OwnedValue, zvariant::Value)]
pub struct MenuLayout {
    pub id: i32,
    pub props: HashMap<String, OwnedValue>,
    pub children: Vec<OwnedValue>,
}

/// 解码后的中间表示（属性保留 OwnedValue，与 zbus 调用解耦，便于纯函数测试）。
#[derive(Debug, PartialEq)]
pub struct RawMenuItem {
    pub id: i32,
    pub props: HashMap<String, OwnedValue>,
    pub children: Vec<RawMenuItem>,
}

impl RawMenuItem {
    pub fn prop_str(&self, key: &str, default: &str) -> String {
        get_str(&self.props, key, default)
    }
    pub fn prop_bool(&self, key: &str, default: bool) -> bool {
        get_bool(&self.props, key, default)
    }
    pub fn prop_i32(&self, key: &str, default: i32) -> i32 {
        get_i32(&self.props, key, default)
    }
}

// ── 属性提取 ─────────────────────────────────────────────────

fn get_str(props: &HashMap<String, OwnedValue>, key: &str, default: &str) -> String {
    props
        .get(key)
        .and_then(|v| <&str>::try_from(v as &zvariant::Value).ok())
        .map(str::to_owned)
        .unwrap_or_else(|| default.to_string())
}

fn get_bool(props: &HashMap<String, OwnedValue>, key: &str, default: bool) -> bool {
    props
        .get(key)
        .and_then(|v| bool::try_from(v as &zvariant::Value).ok())
        .unwrap_or(default)
}

fn get_i32(props: &HashMap<String, OwnedValue>, key: &str, default: i32) -> i32 {
    props
        .get(key)
        .and_then(|v| i32::try_from(v as &zvariant::Value).ok())
        .unwrap_or(default)
}

// ── 总线调用 ─────────────────────────────────────────────────

/// 全量拉取菜单树。bus_name 为注册时记录的调用者连接名，menu_path 为注册路径。
/// 返回解码后的原始树（根 id=0 为隐藏锚点，其 children 即菜单栏顶层项）。
pub fn fetch_layout(conn: &Connection, bus_name: &str, menu_path: &str) -> Result<RawMenuItem> {
    let (_revision, layout): (u32, MenuLayout) = conn
        .call_method(
            Some(bus_name),
            menu_path,
            Some(IFACE_DBUSMENU),
            "GetLayout",
            &(0i32, -1i32, Vec::<String>::new()),
        )
        .with_context(|| format!("GetLayout({bus_name}, {menu_path}) — app may have left the bus"))?
        .body()
        .deserialize()
        .with_context(|| format!("GetLayout({bus_name}, {menu_path}) body decode"))?;
    Ok(decode_layout(layout))
}

/// 递归解包 (ia{sv}av)：children 的每个 OwnedValue 变体再转回 MenuLayout。
pub fn decode_layout(layout: MenuLayout) -> RawMenuItem {
    let children = layout
        .children
        .into_iter()
        .filter_map(|owned| MenuLayout::try_from(owned).ok().map(decode_layout))
        .collect();
    RawMenuItem { id: layout.id, props: layout.props, children }
}

/// 点击回传：Event(id, "clicked", "", timestamp)。
/// data 用空字符串（dbusmenu 文档约定的普通点击载荷，参考实现同款；
/// Chromium 忽略 clicked 的 data 内容）。
pub fn event(conn: &Connection, bus_name: &str, menu_path: &str, id: i32) -> Result<()> {
    let ts: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    conn.call_method(
        Some(bus_name),
        menu_path,
        Some(IFACE_DBUSMENU),
        "Event",
        &(id, "clicked", zvariant::Value::from(""), ts),
    )
    .with_context(|| format!("Event({id}, clicked) on {bus_name} {menu_path}"))?;
    Ok(())
}

// ── 转换：RawMenuItem → 统一菜单模型 ─────────────────────────

/// 去助记符下划线（DBusMenu 惯例：'_' 后跟字母/数字 = 助记符，参考 GTK 约定）。
/// "_File" → ("File", Some("F"))；无助记符时原样返回。
/// 协议承诺 label 已去助记符（§5 统一 schema 注释），与 atspi 侧（ATK name
/// 天然干净）保持一致。
pub fn strip_mnemonic(label: &str) -> (String, Option<String>) {
    let mut out = String::with_capacity(label.len());
    let mut mnemonic: Option<String> = None;
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            match chars.peek() {
                Some(&next) if next.is_alphanumeric() => {
                    if mnemonic.is_none() {
                        mnemonic = Some(next.to_string());
                    }
                    out.push(next);
                    chars.next();
                }
                _ => out.push('_'),
            }
        } else {
            out.push(c);
        }
    }
    (out, mnemonic)
}

/// 可见性判据：DBusMenu 树**含隐藏项**（Chromium 把 disabled 的 Edit 菜单等
/// 一并导出），与 AT-SPI 不同（ATK 树只含可见组件，is_visible 恒 true）。
/// 隐藏项不占 id、不渲染，但 path 槽位保留原始 child-index（locate 走原始索引）。
fn is_visible(raw: &RawMenuItem) -> bool {
    get_bool(&raw.props, "visible", true)
}

/// RawMenuItem → 协议 MenuItem。ids 为 DFS 分配器（与 atspi::build_item 同语义：
/// 会话内 DFS 序号，根 id=1）；path = 相对根的原始 children 索引链。
/// type 映射：separator / submenu（有 children 或 type=="submenu"）/
/// checkbox（toggle-type=="checkmark"）/ radio（toggle-type=="radio"）/
/// item（含标准 type 缺省）。
pub fn raw_to_menu_item(raw: &RawMenuItem, path: &[u32], ids: &mut u32) -> MenuItem {
    let id = { *ids += 1; *ids };
    let (label, mnemonic) = strip_mnemonic(&get_str(&raw.props, "label", ""));
    let raw_type = get_str(&raw.props, "type", "standard");
    let enabled = get_bool(&raw.props, "enabled", true);
    let visible = get_bool(&raw.props, "visible", true);
    let icon = get_str(&raw.props, "icon-name", "");
    let toggle_type = get_str(&raw.props, "toggle-type", "");
    let toggle_state = get_i32(&raw.props, "toggle-state", 0);

    let mut children = Vec::new();
    for (i, child) in raw.children.iter().enumerate() {
        if !is_visible(child) {
            continue;
        }
        let mut child_path = path.to_vec();
        child_path.push(i as u32);
        children.push(raw_to_menu_item(child, &child_path, ids));
    }

    let item_type = if raw_type == "separator" {
        MenuItemType::Separator
    } else if raw_type == "submenu" || !children.is_empty() {
        MenuItemType::Submenu
    } else if toggle_type == "checkmark" {
        MenuItemType::Checkbox
    } else if toggle_type == "radio" {
        MenuItemType::Radio
    } else {
        MenuItemType::Item
    };

    MenuItem {
        id,
        label,
        mnemonic,
        item_type,
        enabled,
        visible,
        checked: toggle_state == 1,
        icon: if icon.is_empty() { None } else { Some(icon) },
        children,
        path: path.to_vec(),
    }
}

/// 由目标节点的直接子项构建 /open 响应（与主树同形、同 id 空间）。
/// 子项 path = 父 path + 原始 child-index；隐藏项过滤但占槽（与主树一致）。
pub fn children_from_raw(raws: &[RawMenuItem], parent_path: &[u32]) -> Vec<MenuItem> {
    let mut ids = 0u32;
    let mut items = Vec::new();
    for (i, raw) in raws.iter().enumerate() {
        if !is_visible(raw) {
            continue;
        }
        let mut p = parent_path.to_vec();
        p.push(i as u32);
        items.push(raw_to_menu_item(raw, &p, &mut ids));
    }
    items
}

/// 按 child-index 链在**原始**树定位（索引含隐藏项，与 path 语义一致）。
pub fn locate_raw_by_path<'a>(root: &'a RawMenuItem, path: &[u32]) -> Option<&'a RawMenuItem> {
    let mut cur = root;
    for &i in path {
        cur = cur.children.get(i as usize)?;
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zvariant::serialized::Context;
    use zvariant::Value;

    /// 构造 (ia{sv}av) fixture 并走完整 wire 编码/解码往返
    /// （to_bytes → Data::deserialize → decode_layout），锁定递归解包逻辑。
    fn wire_roundtrip(layout: &MenuLayout) -> RawMenuItem {
        let ctx = Context::new_dbus(zvariant::LE, 0);
        let encoded = zvariant::to_bytes(ctx, &(0u32, layout)).unwrap();
        let (_rev, decoded) = encoded.deserialize::<(u32, MenuLayout)>().unwrap().0;
        decode_layout(decoded)
    }

    fn owned(v: &Value<'_>) -> OwnedValue {
        v.try_to_owned().unwrap()
    }

    fn layout(id: i32, props: &[(&str, Value<'_>)], children: Vec<MenuLayout>) -> MenuLayout {
        let props = props
            .iter()
            .map(|(k, v)| (k.to_string(), owned(v)))
            .collect();
        let children = children
            .into_iter()
            .map(|c| OwnedValue::try_from(c).unwrap())
            .collect();
        MenuLayout { id, props, children }
    }

    fn menu_layout(id: i32, label: &str) -> MenuLayout {
        layout(id, &[("label", Value::from(label))], vec![])
    }

    #[test]
    fn wire_roundtrip_decodes_nested_children() {
        let root = layout(
            0,
            &[],
            vec![
                layout(
                    1,
                    &[("label", Value::from("_File")), ("type", Value::from("submenu"))],
                    vec![
                        menu_layout(2, "New"),
                        layout(3, &[("type", Value::from("separator"))], vec![]),
                    ],
                ),
                menu_layout(4, "_Edit"),
            ],
        );
        let raw = wire_roundtrip(&root);
        assert_eq!(raw.id, 0);
        assert_eq!(raw.children.len(), 2);
        assert_eq!(raw.children[0].id, 1);
        assert_eq!(raw.children[0].prop_str("label", ""), "_File");
        assert_eq!(raw.children[0].children.len(), 2);
        assert_eq!(raw.children[0].children[1].prop_str("type", ""), "separator");
        assert_eq!(raw.children[1].id, 4);
    }

    #[test]
    fn raw_to_menu_item_maps_types_and_paths() {
        let raw = wire_roundtrip(&layout(
            0,
            &[],
            vec![
                layout(
                    1,
                    &[("label", Value::from("_File")), ("type", Value::from("submenu"))],
                    vec![
                        layout(2, &[("label", Value::from("_New"))], vec![]),
                        layout(
                            3,
                            &[
                                ("label", Value::from("Check Me")),
                                ("toggle-type", Value::from("checkmark")),
                                ("toggle-state", Value::from(1i32)),
                            ],
                            vec![],
                        ),
                        layout(4, &[("type", Value::from("separator"))], vec![]),
                    ],
                ),
                layout(
                    5,
                    &[
                        ("label", Value::from("View")),
                        ("toggle-type", Value::from("radio")),
                        ("toggle-state", Value::from(0i32)),
                    ],
                    vec![],
                ),
            ],
        ));
        let mut ids = 0u32;
        let tree = raw_to_menu_item(&raw, &[], &mut ids);

        // DFS id 分配：根=1, File=2, New=3, Check=4, sep=5, View=6
        assert_eq!(tree.id, 1);
        assert_eq!(tree.item_type, MenuItemType::Submenu);
        assert_eq!(tree.label, "");
        assert_eq!(tree.children.len(), 2);
        assert_eq!(tree.children[0].label, "File"); // 助记符剥离
        assert_eq!(tree.children[0].mnemonic, Some("F".to_string()));
        assert_eq!(tree.children[0].item_type, MenuItemType::Submenu);
        assert_eq!(tree.children[0].path, vec![0]);
        assert_eq!(tree.children[0].children.len(), 3);

        let check = &tree.children[0].children[1];
        assert_eq!(check.item_type, MenuItemType::Checkbox);
        assert_eq!(check.checked, true);
        assert_eq!(check.path, vec![0, 1]);

        let sep = &tree.children[0].children[2];
        assert_eq!(sep.item_type, MenuItemType::Separator);
        assert_eq!(sep.path, vec![0, 2]);

        let radio = &tree.children[1];
        assert_eq!(radio.item_type, MenuItemType::Radio);
        assert_eq!(radio.checked, false);
        assert_eq!(radio.path, vec![1]);

        // id 连续 DFS（checkbox/separator 也占 id）
        assert_eq!(check.id, 4);
        assert_eq!(sep.id, 5);
        assert_eq!(radio.id, 6);
    }

    #[test]
    fn hidden_items_dropped_but_path_slots_kept() {
        let raw = wire_roundtrip(&layout(
            0,
            &[],
            vec![
                layout(1, &[("label", Value::from("Visible"))], vec![]),
                layout(
                    2,
                    &[("label", Value::from("Hidden")), ("visible", Value::from(false))],
                    vec![],
                ),
                layout(3, &[("label", Value::from("Also")), ("visible", Value::from(false))], vec![]),
                layout(4, &[("label", Value::from("Shown"))], vec![]),
            ],
        ));
        let mut ids = 0u32;
        let tree = raw_to_menu_item(&raw, &[], &mut ids);
        assert_eq!(tree.children.len(), 2);
        assert_eq!(tree.children[0].label, "Visible");
        assert_eq!(tree.children[0].path, vec![0]); // 原始索引
        assert_eq!(tree.children[1].label, "Shown");
        assert_eq!(tree.children[1].path, vec![3]); // 隐藏项占槽
        assert_eq!(tree.children[1].id, 3); // 隐藏项不占 id
    }

    #[test]
    fn defaults_applied_when_props_missing() {
        let raw = wire_roundtrip(&layout(0, &[], vec![layout(1, &[], vec![])]));
        let mut ids = 0u32;
        let item = raw_to_menu_item(&raw, &[], &mut ids).children.remove(0);
        assert_eq!(item.label, "");
        assert_eq!(item.item_type, MenuItemType::Item); // 无 type → standard
        assert_eq!(item.enabled, true);
        assert_eq!(item.visible, true);
        assert_eq!(item.checked, false);
        assert_eq!(item.icon, None);
    }

    #[test]
    fn icon_name_maps_to_icon_field() {
        let raw = wire_roundtrip(&layout(
            0,
            &[],
            vec![layout(1, &[("icon-name", Value::from("document-save"))], vec![])],
        ));
        let mut ids = 0u32;
        let item = raw_to_menu_item(&raw, &[], &mut ids).children.remove(0);
        assert_eq!(item.icon.as_deref(), Some("document-save"));
    }

    #[test]
    fn strip_mnemonic_handles_markers_and_literals() {
        assert_eq!(strip_mnemonic("_File"), ("File".to_string(), Some("F".to_string())));
        assert_eq!(strip_mnemonic("Save _As"), ("Save As".to_string(), Some("A".to_string())));
        // 无标记：原样
        assert_eq!(strip_mnemonic("plain"), ("plain".to_string(), None));
        // 尾部下划线不是标记
        assert_eq!(strip_mnemonic("File_"), ("File_".to_string(), None));
        // 下划线后跟非字母数字（空格）不是标记
        assert_eq!(strip_mnemonic("_ File"), ("_ File".to_string(), None));
    }

    #[test]
    fn locate_raw_by_path_uses_raw_indices() {
        let raw = wire_roundtrip(&layout(
            0,
            &[],
            vec![
                layout(
                    1,
                    &[("label", Value::from("File"))],
                    vec![menu_layout(2, "Inner"), menu_layout(3, "Deep")],
                ),
                menu_layout(4, "View"),
            ],
        ));
        assert_eq!(locate_raw_by_path(&raw, &[]).unwrap().id, 0);
        assert_eq!(locate_raw_by_path(&raw, &[0]).unwrap().id, 1);
        assert_eq!(locate_raw_by_path(&raw, &[0, 1]).unwrap().id, 3);
        assert_eq!(locate_raw_by_path(&raw, &[1]).unwrap().id, 4);
        assert!(locate_raw_by_path(&raw, &[2]).is_none());
        assert!(locate_raw_by_path(&raw, &[0, 5]).is_none());
    }

    #[test]
    fn children_from_raw_matches_main_tree_id_space() {
        // /open 路径：与主树同形同 id（确定性 DFS），path = 父 path + 原始索引
        let raw = wire_roundtrip(&layout(
            0,
            &[],
            vec![layout(
                1,
                &[("label", Value::from("File"))],
                vec![
                    menu_layout(2, "New"),
                    layout(3, &[("visible", Value::from(false))], vec![]),
                    menu_layout(4, "Open"),
                ],
            )],
        ));
        let items = children_from_raw(&raw.children[0].children, &[0]);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "New");
        assert_eq!(items[0].path, vec![0, 0]);
        assert_eq!(items[0].id, 1);
        assert_eq!(items[1].label, "Open");
        assert_eq!(items[1].path, vec![0, 2]); // 隐藏项占槽
        assert_eq!(items[1].id, 2);
    }
}
