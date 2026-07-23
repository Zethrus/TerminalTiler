use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn closing_client_stdin_exits_server_within_one_second() {
    let project_root =
        std::env::temp_dir().join(format!("terminaltiler-mcp-eof-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&project_root).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_terminaltiler-mcp"))
        .arg("--project-root")
        .arg(&project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    drop(child.stdin.take());

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("terminaltiler-mcp did not exit after client stdin closed");
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let _ = std::fs::remove_dir_all(project_root);
}
