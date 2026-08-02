//! 用桥自身代码探测 AT-SPI 链路（诊断用）：pid [title]
use noctalia_global_menu_bridge::atspi::AtspiClient;

fn main() {
    let pid: u32 = std::env::args().nth(1).expect("pid").parse().expect("pid u32");
    let title = std::env::args().nth(2).unwrap_or_default();
    let c = AtspiClient::connect().expect("connect");
    match c.find_app_for_pid(pid) {
        Ok(Some(app)) => {
            println!("app found: {} {}", app.bus, app.path);
            match c.choose_frame(&app, &title) {
                Ok(scope) => println!("choose_frame -> {:?}", scope),
                Err(e) => println!("choose_frame ERR {e:#}"),
            }
            let scope = c.choose_frame(&app, &title).ok().flatten().unwrap_or(app.clone());
            match c.find_menubar(&scope) {
                Ok(Some(mb)) => {
                    println!("menubar found: {} {}", mb.bus, mb.path);
                    match c.fetch_menubar(pid, &title) {
                        Ok(Some(raw)) => {
                            let mut ids = 0u32;
                            let tree = noctalia_global_menu_bridge::atspi::build_menu_tree(&raw, &mut ids);
                            println!("tree: {} top items, ids={}", tree.children.len(), ids);
                            for ch in &tree.children { println!("  {} ({:?})", ch.label, ch.item_type); }
                        }
                        Ok(None) => println!("fetch_menubar -> None"),
                        Err(e) => println!("fetch_menubar ERR {e:#}"),
                    }
                }
                Ok(None) => println!("find_menubar -> None (scope={} {})", scope.bus, scope.path),
                Err(e) => println!("find_menubar ERR {e:#}"),
            }
        }
        Ok(None) => println!("app NOT found for pid {pid}"),
        Err(e) => println!("find_app ERR {e:#}"),
    }
}
