//! Detecting which agent CLI is running inside a terminal tile, per platform.
//!
//! Linux is the source of truth: it inspects the pty's foreground process group, the precise
//! program the user is interacting with. Windows approximates by walking the descendants of
//! the tile's child process (agents hosted inside WSL are invisible to Win32 and are covered
//! by the caller's launch-command fallback instead).

/// Command strings of processes running under a terminal tile that may be an agent CLI, for
/// classification via `AgentKind::from_command`.
///
/// - `pty_fd`: the tile's pty master fd (Linux).
/// - `child_pid`: the tile's spawned child pid (Windows).
///
/// Returns an empty vec when unavailable or on unsupported platforms.
pub fn terminal_agent_candidates(pty_fd: Option<i32>, child_pid: Option<i32>) -> Vec<String> {
    imp::terminal_agent_candidates(pty_fd, child_pid)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::fs;

    pub fn terminal_agent_candidates(pty_fd: Option<i32>, _child_pid: Option<i32>) -> Vec<String> {
        let Some(fd) = pty_fd else {
            return Vec::new();
        };
        // The foreground process group leader is the program currently reading the terminal.
        let fpgid = unsafe { libc::tcgetpgrp(fd) };
        if fpgid <= 0 {
            return Vec::new();
        }
        match fs::read(format!("/proc/{fpgid}/cmdline")) {
            Ok(bytes) => {
                let cmdline = String::from_utf8_lossy(&bytes).replace('\0', " ");
                let cmdline = cmdline.trim();
                if cmdline.is_empty() {
                    Vec::new()
                } else {
                    vec![cmdline.to_string()]
                }
            }
            Err(_) => Vec::new(),
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::collections::HashMap;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    pub fn terminal_agent_candidates(_pty_fd: Option<i32>, child_pid: Option<i32>) -> Vec<String> {
        let Some(root) = child_pid.map(|pid| pid as u32) else {
            return Vec::new();
        };
        // (pid -> (parent_pid, exe_name)) snapshot of all processes.
        let processes = snapshot_processes();
        if processes.is_empty() {
            return Vec::new();
        }
        // Collect exe names of `root` and all its descendants.
        let mut candidates = Vec::new();
        let mut frontier = vec![root];
        let mut seen = std::collections::HashSet::new();
        while let Some(pid) = frontier.pop() {
            if !seen.insert(pid) {
                continue;
            }
            if let Some((_, exe)) = processes.get(&pid)
                && !exe.is_empty()
            {
                candidates.push(exe.clone());
            }
            for (&child, &(parent, _)) in processes.iter() {
                if parent == pid {
                    frontier.push(child);
                }
            }
        }
        candidates
    }

    fn snapshot_processes() -> HashMap<u32, (u32, String)> {
        let mut map = HashMap::new();
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return map;
        }
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
        while ok != 0 {
            let exe = wide_to_string(&entry.szExeFile);
            map.insert(entry.th32ProcessID, (entry.th32ParentProcessID, exe));
            ok = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        unsafe { CloseHandle(snapshot) };
        map
    }

    fn wide_to_string(wide: &[u16]) -> String {
        let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        String::from_utf16_lossy(&wide[..len])
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod imp {
    pub fn terminal_agent_candidates(_pty_fd: Option<i32>, _child_pid: Option<i32>) -> Vec<String> {
        Vec::new()
    }
}
