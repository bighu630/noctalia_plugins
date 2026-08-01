use noctalia_global_menu_bridge::protocol::BridgeEvent;

fn main() {
    let ev = BridgeEvent::Hello { port: 0, pid: std::process::id() };
    println!("{}", ev.to_line());
}
