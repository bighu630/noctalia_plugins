//! DBusMenu 管线集成测试：wire fixture（zvariant 编码往返）→ 属性解析 →
//! 统一菜单树转换（含 checkbox/separator/path/隐藏项）→ 点击侧 path 定位。

use noctalia_global_menu_bridge::dbusmenu::{
    children_from_raw, decode_layout, locate_raw_by_path, raw_to_menu_item, strip_mnemonic, MenuLayout, RawMenuItem,
};
use noctalia_global_menu_bridge::protocol::MenuItemType;
use std::collections::HashMap;
use zvariant::serialized::Context;
use zvariant::{OwnedValue, Value};

fn owned(v: &Value<'_>) -> OwnedValue {
    v.try_to_owned().unwrap()
}

fn props(pairs: &[(&str, Value<'_>)]) -> HashMap<String, OwnedValue> {
    pairs.iter().map(|(k, v)| (k.to_string(), owned(v))).collect()
}

fn layout(id: i32, pairs: &[(&str, Value<'_>)], children: Vec<MenuLayout>) -> MenuLayout {
    let children = children.into_iter().map(|c| OwnedValue::try_from(c).unwrap()).collect();
    MenuLayout { id, props: props(pairs), children }
}

/// 构造 (ia{sv}av) fixture，走完整 wire 编码 → 解码。
fn wire_fixture() -> RawMenuItem {
    let root = layout(
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
                            ("label", Value::from("Word Wrap")),
                            ("toggle-type", Value::from("checkmark")),
                            ("toggle-state", Value::from(1i32)),
                        ],
                        vec![],
                    ),
                    layout(4, &[("type", Value::from("separator"))], vec![]),
                    layout(
                        5,
                        &[("label", Value::from("Hidden")) , ("visible", Value::from(false))],
                        vec![],
                    ),
                    layout(
                        6,
                        &[("label", Value::from("Zoom")), ("toggle-type", Value::from("radio")), ("toggle-state", Value::from(0i32))],
                        vec![],
                    ),
                ],
            ),
            layout(7, &[("label", Value::from("_Edit"))], vec![]),
        ],
    );
    let ctx = Context::new_dbus(zvariant::LE, 0);
    let encoded = zvariant::to_bytes(ctx, &(0u32, &root)).unwrap();
    let (_rev, decoded) = encoded.deserialize::<(u32, MenuLayout)>().unwrap().0;
    decode_layout(decoded)
}

#[test]
fn wire_fixture_decodes_full_tree() {
    let raw = wire_fixture();
    assert_eq!(raw.id, 0);
    assert_eq!(raw.children.len(), 2);
    let file = &raw.children[0];
    assert_eq!(file.prop_str("label", ""), "_File");
    assert_eq!(file.prop_str("type", ""), "submenu");
    assert_eq!(file.children.len(), 5); // 隐藏项也在 raw 树里（转换时才过滤）
    assert_eq!(file.children[1].prop_str("toggle-type", ""), "checkmark");
    assert_eq!(file.children[1].prop_i32("toggle-state", -99), 1);
    assert_eq!(file.children[1].prop_bool("visible", true), true);
    assert_eq!(file.children[3].prop_bool("visible", true), false);
    assert_eq!(file.children[4].prop_str("toggle-type", ""), "radio");
}

#[test]
fn converts_to_menu_item_tree_with_types_and_paths() {
    let raw = wire_fixture();
    let mut ids = 0u32;
    let tree = raw_to_menu_item(&raw, &[], &mut ids);

    // 根（隐藏锚点）：id=1，submenu，label 空
    assert_eq!(tree.id, 1);
    assert_eq!(tree.item_type, MenuItemType::Submenu);
    assert_eq!(tree.label, "");
    assert_eq!(tree.path, Vec::<u32>::new());

    // 顶层两项
    assert_eq!(tree.children.len(), 2);
    let file = &tree.children[0];
    assert_eq!(file.label, "File"); // 助记符剥离
    assert_eq!(file.mnemonic.as_deref(), Some("F"));
    assert_eq!(file.item_type, MenuItemType::Submenu); // type=submenu
    assert_eq!(file.path, vec![0]);

    // File 子项：New / checkbox / separator / (隐藏) / radio —— 隐藏项被过滤
    assert_eq!(file.children.len(), 4);
    assert_eq!(file.children[0].label, "New");
    assert_eq!(file.children[0].item_type, MenuItemType::Item);
    assert_eq!(file.children[0].path, vec![0, 0]);

    let check = &file.children[1];
    assert_eq!(check.item_type, MenuItemType::Checkbox);
    assert_eq!(check.checked, true);
    assert_eq!(check.path, vec![0, 1]); // 原始索引

    let sep = &file.children[2];
    assert_eq!(sep.item_type, MenuItemType::Separator);
    assert_eq!(sep.path, vec![0, 2]);

    let radio = &file.children[3];
    assert_eq!(radio.item_type, MenuItemType::Radio);
    assert_eq!(radio.checked, false);
    assert_eq!(radio.path, vec![0, 4]); // 隐藏项占槽（原始索引 3 被过滤）

    let edit = &tree.children[1];
    assert_eq!(edit.label, "Edit");
    assert_eq!(edit.item_type, MenuItemType::Item); // 无 children 无 type → item
    assert_eq!(edit.path, vec![1]);

    // DFS id：根1 File2 New3 check4 sep5 radio6 Edit7
    assert_eq!(file.id, 2);
    assert_eq!(file.children[0].id, 3);
    assert_eq!(check.id, 4);
    assert_eq!(sep.id, 5);
    assert_eq!(radio.id, 6);
    assert_eq!(edit.id, 7);
}

#[test]
fn click_side_locates_node_and_recovers_dbusmenu_id() {
    // 点击链路：path → 原始树定位 → DBusMenu id（Event 回传用）
    let raw = wire_fixture();
    let target = locate_raw_by_path(&raw, &[0, 1]).expect("checkbox node");
    assert_eq!(target.id, 3); // DBusMenu 侧 id（与协议 DFS id 不同）
    let hidden = locate_raw_by_path(&raw, &[0, 3]).expect("hidden node also locatable");
    assert_eq!(hidden.id, 5);
    let edit = locate_raw_by_path(&raw, &[1]).expect("edit node");
    assert_eq!(edit.id, 7);
    assert!(locate_raw_by_path(&raw, &[0, 9]).is_none());
    assert!(locate_raw_by_path(&raw, &[2]).is_none());
}

#[test]
fn open_children_matches_main_tree_ids_and_paths() {
    // /open 路径：目标节点的子项 → 与主树同形同 id（确定性 DFS）
    let raw = wire_fixture();
    let file = locate_raw_by_path(&raw, &[0]).unwrap();
    let items = children_from_raw(&file.children, &[0]);
    assert_eq!(items.len(), 4); // 隐藏项过滤
    assert_eq!(items[0].label, "New");
    assert_eq!(items[0].id, 1);
    assert_eq!(items[0].path, vec![0, 0]);
    assert_eq!(items[3].label, "Zoom");
    assert_eq!(items[3].id, 4); // 与主树转换一致（隐藏项不占 id）
    assert_eq!(items[3].path, vec![0, 4]);
}

#[test]
fn separator_and_defaults_are_sane() {
    // 无 type 的叶子 → item；无 label → 空串；无 enabled → true
    let raw = decode_layout(layout(0, &[], vec![layout(1, &[], vec![])]));
    let mut ids = 0u32;
    let item = raw_to_menu_item(&raw, &[], &mut ids).children.remove(0);
    assert_eq!(item.item_type, MenuItemType::Item);
    assert_eq!(item.label, "");
    assert!(item.enabled);
    assert!(item.visible);
    assert!(!item.checked);
    assert_eq!(item.icon, None);
}

#[test]
fn strip_mnemonic_is_exported_and_correct() {
    assert_eq!(strip_mnemonic("_Format"), ("Format".to_string(), Some("F".to_string())));
    assert_eq!(strip_mnemonic("NoAccel"), ("NoAccel".to_string(), None));
}
