//! Registrar 映射逻辑测试（纯函数部分；接口方法依赖会话总线，不测 wire）。

use noctalia_global_menu_bridge::registrar::{RegistrarState, MenuRegistration};

fn sample(xid: u32, pid: u32) -> (u32, String, String, u32) {
    (xid, format!(":1.{xid}"), format!("/com/canonical/menu/{xid:x}"), pid)
}

#[test]
fn register_builds_pid_index_for_focus_matching() {
    let mut s = RegistrarState::default();
    let (xid, bus, path, pid) = sample(0x800001, 242071);
    s.register(xid, bus.clone(), path.clone(), pid);
    assert_eq!(s.len(), 1);
    // pid 索引
    let regs = s.registrations_for_pid(242071);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0], MenuRegistration { xid, bus, path, pid });
}

#[test]
fn unregister_keeps_pid_index_consistent() {
    let mut s = RegistrarState::default();
    s.register(1, ":1.1".into(), "/m/1".into(), 100);
    s.register(2, ":1.1".into(), "/m/2".into(), 100);
    s.register(3, ":1.2".into(), "/m/3".into(), 200);

    assert!(s.unregister(2).is_some());
    assert_eq!(s.registrations_for_pid(100).len(), 1);
    assert_eq!(s.registrations_for_pid(100)[0].xid, 1);
    // 同 pid 全部注销后索引槽移除
    s.unregister(1);
    assert_eq!(s.registrations_for_pid(100).len(), 0);
    assert_eq!(s.registrations_for_pid(200).len(), 1);
    assert!(s.unregister(999).is_none());
}

#[test]
fn re_register_same_xid_updates_slot() {
    let mut s = RegistrarState::default();
    s.register(7, ":1.1".into(), "/m/old".into(), 100);
    // 菜单重建：同 xid 换 pid 重注册
    s.register(7, ":1.9".into(), "/m/new".into(), 900);
    assert_eq!(s.len(), 1);
    assert_eq!(s.registrations_for_pid(100).len(), 0);
    assert_eq!(s.registrations_for_pid(900).len(), 1);
    assert_eq!(s.lookup_xid(7).unwrap().path, "/m/new");
    assert_eq!(s.lookup_xid(7).unwrap().bus, ":1.9");
}

#[test]
fn find_for_focus_prefers_exact_pid() {
    let mut s = RegistrarState::default();
    s.register(1, ":1.1".into(), "/m/1".into(), 1111);
    s.register(2, ":1.2".into(), "/m/2".into(), 2222);
    // pid 命中 → 不看 app_id
    let found = s.find_for_focus(2222, "unrelated-app").unwrap();
    assert_eq!(found.xid, 2);
}

#[test]
fn find_for_focus_comm_fallback_handles_xwayland_pid_mismatch() {
    // 模拟 Typora 场景：niri 报 xwayland pid（≠注册 pid），app_id 与 comm 一致
    let mut s = RegistrarState::default();
    let real_pid = std::process::id();
    s.register(0x800001, ":1.99".into(), "/com/canonical/menu/800001".into(), real_pid);
    let comm = std::fs::read_to_string(format!("/proc/{real_pid}/comm")).unwrap();
    let comm = comm.trim();
    let found = s.find_for_focus(0xdead_beef, comm).unwrap();
    assert_eq!(found.xid, 0x800001);
    // 大小写不敏感
    assert!(s.find_for_focus(0xdead_beef, &comm.to_uppercase()).is_some());
}

#[test]
fn find_for_focus_no_match_returns_none() {
    let mut s = RegistrarState::default();
    s.register(1, ":1.1".into(), "/m/1".into(), 1111);
    assert!(s.find_for_focus(9999, "nonexistent").is_none());
    // 空 app_id 不触发兜底
    assert!(s.find_for_focus(9999, "").is_none());
}
