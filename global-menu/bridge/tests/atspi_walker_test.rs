use noctalia_global_menu_bridge::atspi::{build_menu_tree, RawNode, ROLE_CHECK_MENU_ITEM, ROLE_MENU, ROLE_MENU_BAR, ROLE_MENU_ITEM, ROLE_SEPARATOR, STATE_CHECKED, STATE_ENABLED, STATE_SENSITIVE};
use noctalia_global_menu_bridge::protocol::{MenuItemType};

// 可见性位用字面量 1<<30 锁定（ATSPI_STATE_VISIBLE 权威值 = 30，31 是 MANAGES_DESCENDANTS）：
// fixture 不依赖常量，防止常量回归时测试静默失真。

fn node(role: u32, name: &str, state: (u32, u32), children: Vec<RawNode>) -> RawNode {
    RawNode { role, name: name.into(), state, children, acc: None }
}

#[test]
fn builds_tree_with_types_and_checked_state() {
    // File ▸ New / Open… ; View ─ Show All [checked]
    let root = node(ROLE_MENU_BAR, "", (0, 0), vec![
        node(ROLE_MENU, "File", (0, 0), vec![
            node(ROLE_MENU_ITEM, "New", (1 << STATE_ENABLED | 1 << STATE_SENSITIVE | 1 << 30, 0), vec![]),
            node(ROLE_MENU_ITEM, "Open…", (1 << STATE_ENABLED | 1 << STATE_SENSITIVE | 1 << 30, 0), vec![]),
        ]),
        node(ROLE_MENU, "View", (0, 0), vec![
            node(ROLE_CHECK_MENU_ITEM, "Show All", (1 << STATE_ENABLED | 1 << STATE_SENSITIVE | 1 << 30 | 1 << STATE_CHECKED, 0), vec![]),
            node(ROLE_SEPARATOR, "", (1 << 30, 0), vec![]),
        ]),
    ]);
    let mut ids = 0u32;
    let tree = build_menu_tree(&root, &mut ids);
    assert_eq!(tree.item_type, MenuItemType::Submenu); // menubar 根按 submenu 承载
    assert_eq!(tree.children.len(), 2);
    assert_eq!(tree.children[0].label, "File");
    assert_eq!(tree.children[0].item_type, MenuItemType::Submenu);
    assert_eq!(tree.children[0].children[1].label, "Open…");
    assert_eq!(tree.children[0].children[1].path, vec![0, 1]);
    assert_eq!(tree.children[1].children[0].label, "Show All");
    assert_eq!(tree.children[1].children[0].item_type, MenuItemType::Checkbox);
    assert!(tree.children[1].children[0].checked);
    assert_eq!(tree.children[1].children[1].item_type, MenuItemType::Separator);
}

#[test]
fn ids_are_dfs_ordered_and_unique() {
    let root = node(ROLE_MENU_BAR, "", (0, 0), vec![
        node(ROLE_MENU, "A", (0, 0), vec![
            node(ROLE_MENU_ITEM, "A1", (1 << 30, 0), vec![]),
            node(ROLE_MENU_ITEM, "A2", (1 << 30, 0), vec![]),
        ]),
        node(ROLE_MENU, "B", (0, 0), vec![
            node(ROLE_MENU_ITEM, "B1", (1 << 30, 0), vec![]),
        ]),
    ]);
    let mut ids = 0u32;
    let tree = build_menu_tree(&root, &mut ids);
    let mut seen = std::collections::HashSet::new();
    fn walk(t: &noctalia_global_menu_bridge::protocol::MenuItem, seen: &mut std::collections::HashSet<u32>) {
        assert!(seen.insert(t.id), "duplicate id {}", t.id);
        for c in &t.children { walk(c, seen); }
    }
    walk(&tree, &mut seen);
    assert_eq!(ids, 6); // 根 + 2 顶层 + 2 + 1 = 6 个节点（separator 也占 id）
}

#[test]
fn invisible_items_are_filtered() {
    let root = node(ROLE_MENU_BAR, "", (0, 0), vec![
        node(ROLE_MENU, "A", (0, 0), vec![
            node(ROLE_MENU_ITEM, "Hidden", (1 << STATE_ENABLED, 0), vec![]), // 无 VISIBLE
            node(ROLE_MENU_ITEM, "Shown", (1 << 30, 0), vec![]),
        ]),
    ]);
    let mut ids = 0u32;
    let tree = build_menu_tree(&root, &mut ids);
    assert_eq!(tree.children[0].children.len(), 1);
    assert_eq!(tree.children[0].children[0].label, "Shown");
}

#[test]
fn qt_wrapper_menu_is_flattened() {
    // Qt 每个 MENU_ITEM 的 popup 包在无名 MENU 里 → 扁平化
    let root = node(ROLE_MENU_BAR, "", (0, 0), vec![
        node(ROLE_MENU, "File", (0, 0), vec![
            node(ROLE_MENU_ITEM, "Open", (1 << 30, 0), vec![
                node(ROLE_MENU, "", (0, 0), vec![
                    node(ROLE_MENU_ITEM, "Open Recent", (1 << 30, 0), vec![]),
                ]),
            ]),
        ]),
    ]);
    let mut ids = 0u32;
    let tree = build_menu_tree(&root, &mut ids);
    let open = &tree.children[0].children[0];
    assert_eq!(open.item_type, MenuItemType::Submenu);
    assert_eq!(open.children.len(), 1);
    assert_eq!(open.children[0].label, "Open Recent");
    assert_eq!(open.children[0].path, vec![0, 0, 0]);
}
