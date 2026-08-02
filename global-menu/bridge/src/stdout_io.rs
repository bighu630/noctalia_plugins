//! NDJSON 上行发射器：逐行、加锁，避免多线程交错。
use crate::protocol::BridgeEvent;
use std::io::Write;
use std::sync::Mutex;

pub struct StdoutSink {
    lock: Mutex<()>,
}

impl Clone for StdoutSink {
    // 每行 emit 内部经 std::io::stdout() 的共享锁串行化，多份 sink 实例安全。
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl StdoutSink {
    pub fn new() -> Self {
        Self { lock: Mutex::new(()) }
    }

    pub fn emit(&self, ev: &BridgeEvent) {
        let _g = self.lock.lock().unwrap();
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", ev.to_line());
        let _ = out.flush();
    }
}
