use std::io::Write;

fn main() {
    // Placeholder: Task 7 组装完整启动流程。先验证工程可编译、stdout 可逐行输出。
    let hello = serde_json::json!({"type": "hello", "port": 0, "pid": std::process::id()});
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{hello}");
    let _ = out.flush();
}
