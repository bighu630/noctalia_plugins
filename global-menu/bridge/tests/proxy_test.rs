use noctalia_global_menu_bridge::atspi::{RawNode, ROLE_MENU, ROLE_MENU_ITEM};
use noctalia_global_menu_bridge::proxy::{build_children, build_children_response, make_menu_event, FocusInfo};
use noctalia_global_menu_bridge::protocol::{MenuItem, MenuItemType};

fn item(id: u32, label: &str, item_type: MenuItemType, children: Vec<MenuItem>) -> MenuItem {
    MenuItem { id, label: label.into(), mnemonic: None, item_type, enabled: true, visible: true, checked: false, icon: None, children, path: vec![] }
}

// 可见性位字面量 1<<30 = ATSPI_STATE_VISIBLE（权威值，防常量回归）
fn raw_node(role: u32, name: &str, state: (u32, u32), children: Vec<RawNode>) -> RawNode {
    RawNode { role, name: name.into(), state, children, acc: None }
}

#[test]
fn menu_event_carries_app_and_tree() {
    let focus = FocusInfo { win_id: 5, app_id: "gimp".into(), title: "GIMP".into(), pid: 42 };
    let menu = Some(item(1, "", MenuItemType::Submenu, vec![item(2, "File", MenuItemType::Submenu, vec![item(3, "New", MenuItemType::Item, vec![])])]));
    let ev = make_menu_event(&focus, menu, "atspi");
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "menu");
    assert_eq!(v["app"]["pid"], 42);
    assert_eq!(v["app"]["app_id"], "gimp");
    assert_eq!(v["source"], "atspi");
    assert_eq!(v["menu"]["children"][0]["label"], "File");
}

#[test]
fn no_menu_yields_null_menu_with_none_source() {
    let focus = FocusInfo { win_id: 5, app_id: "chrome".into(), title: "Chrome".into(), pid: 7 };
    let ev = make_menu_event(&focus, None, "none");
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["menu"], serde_json::Value::Null);
    assert_eq!(v["source"], "none");
}

#[test]
fn children_response_uses_same_id_space() {
    // /open 响应：children 数组 + ok 标记（id 从解析后的树取）
    let children = vec![item(3, "Zoom", MenuItemType::Item, vec![])];
    let resp = build_children_response(true, children);
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["children"][0]["id"], 3);
}

#[test]
fn open_children_paths_are_parent_plus_raw_index() {
    // B2 回归锁定：/open 返回的子项 path 必须是 父 path + 原始 child-index。
    // 曾把父 path 原样传给每个子项 → /click {path:[0]} 定位到 File 菜单本身
    // 并 DoAction（静默执行错误动作）；点子菜单 → /open 死循环。
    let raws = vec![
        raw_node(ROLE_MENU_ITEM, "New", (1 << 30, 0), vec![]),
        raw_node(ROLE_MENU_ITEM, "Open…", (1 << 30, 0), vec![]),
        raw_node(ROLE_MENU, "Recent", (0, 0), vec![
            raw_node(ROLE_MENU_ITEM, "Recent A", (1 << 30, 0), vec![]),
        ]),
    ];
    let items = build_children(&raws, &[0]); // 父 path=[0]（File 菜单）
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].path, vec![0, 0]); // New
    assert_eq!(items[1].path, vec![0, 1]); // Open…
    assert_eq!(items[2].path, vec![0, 2]); // Recent（子菜单自身）
    assert_eq!(items[2].children[0].path, vec![0, 2, 0]); // Recent ▸ Recent A
}

#[test]
fn open_children_filter_invisible_like_main_tree() {
    // 与主树可见性策略一致：不可见叶子不出现，但 path 槽位仍是原始 child-index
    let raws = vec![
        raw_node(ROLE_MENU_ITEM, "Hidden", (0, 0), vec![]), // 无 VISIBLE 位
        raw_node(ROLE_MENU_ITEM, "Shown", (1 << 30, 0), vec![]),
    ];
    let items = build_children(&raws, &[0]);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Shown");
    assert_eq!(items[0].path, vec![0, 1]); // 原始索引 1（Hidden 被过滤但占槽）
}

#[test]
fn prune_to_top_level_keeps_types_but_drops_nested_children() {
    use noctalia_global_menu_bridge::proxy::prune_to_top_level;
    let tree = item(1, "", MenuItemType::Submenu, vec![
        item(2, "File", MenuItemType::Submenu, vec![
            item(3, "New", MenuItemType::Item, vec![]),
        ]),
        item(4, "View", MenuItemType::Submenu, vec![
            item(5, "Zoom", MenuItemType::Item, vec![]),
        ]),
    ]);
    let pruned = prune_to_top_level(tree);
    assert_eq!(pruned.children.len(), 2);
    assert_eq!(pruned.children[0].item_type, MenuItemType::Submenu); // type 保留
    assert_eq!(pruned.children[0].label, "File");
    assert!(pruned.children[0].children.is_empty()); // 子树清空
    assert!(pruned.children[1].children.is_empty());
}
