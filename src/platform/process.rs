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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForegroundProcess {
    pub pid: u32,
    pub started_at_ticks: Option<u64>,
    pub tty: Option<String>,
    pub executable: String,
    pub command: String,
}

/// Best-effort identity of the process currently associated with a terminal.
/// Callers must require a start discriminator before destructive control.
pub fn terminal_foreground_process(
    pty_fd: Option<i32>,
    child_pid: Option<i32>,
) -> Option<ForegroundProcess> {
    imp::terminal_foreground_process(pty_fd, child_pid)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::fs;

    use super::ForegroundProcess;

    pub fn terminal_agent_candidates(pty_fd: Option<i32>, _child_pid: Option<i32>) -> Vec<String> {
        terminal_foreground_process(pty_fd, None)
            .map(|process| vec![process.command])
            .unwrap_or_default()
    }

    pub fn terminal_foreground_process(
        pty_fd: Option<i32>,
        _child_pid: Option<i32>,
    ) -> Option<ForegroundProcess> {
        let fd = pty_fd?;
        let pid = unsafe { libc::tcgetpgrp(fd) };
        if pid <= 0 {
            return None;
        }
        let pid = u32::try_from(pid).ok()?;
        let bytes = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let arguments = bytes
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .map(|argument| String::from_utf8_lossy(argument).into_owned())
            .collect::<Vec<_>>();
        let command = arguments.join(" ");
        let executable = arguments
            .first()
            .and_then(|argument| argument.rsplit('/').next())
            .filter(|argument| !argument.is_empty())?
            .to_string();
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok();
        let started_at_ticks = stat.as_deref().and_then(parse_start_ticks);
        let tty = fs::read_link(format!("/proc/{pid}/fd/0"))
            .ok()
            .map(|path| path.display().to_string());
        Some(ForegroundProcess {
            pid,
            started_at_ticks,
            tty,
            executable,
            command,
        })
    }

    fn parse_start_ticks(stat: &str) -> Option<u64> {
        let close = stat.rfind(')')?;
        // Remaining fields begin at proc field 3 (state). Start time is field
        // 22, therefore index 19 in this tail.
        stat.get(close + 1..)?
            .split_whitespace()
            .nth(19)?
            .parse()
            .ok()
    }

    #[cfg(test)]
    mod tests {
        use super::parse_start_ticks;

        #[test]
        fn parses_start_ticks_with_spaces_and_parentheses_in_comm() {
            let mut fields = vec!["S".to_string()];
            fields.extend((4..=21).map(|value| value.to_string()));
            fields.push("424242".into());
            let stat = format!("99 (agent worker (nested)) {}", fields.join(" "));
            assert_eq!(parse_start_ticks(&stat), Some(424242));
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

    use super::ForegroundProcess;

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

    pub fn terminal_foreground_process(
        _pty_fd: Option<i32>,
        child_pid: Option<i32>,
    ) -> Option<ForegroundProcess> {
        let pid = u32::try_from(child_pid?).ok()?;
        let processes = snapshot_processes();
        let (_, executable) = processes.get(&pid)?.clone();
        Some(ForegroundProcess {
            pid,
            started_at_ticks: None,
            tty: None,
            command: executable.clone(),
            executable,
        })
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
    use super::ForegroundProcess;

    pub fn terminal_agent_candidates(_pty_fd: Option<i32>, _child_pid: Option<i32>) -> Vec<String> {
        Vec::new()
    }

    pub fn terminal_foreground_process(
        _pty_fd: Option<i32>,
        _child_pid: Option<i32>,
    ) -> Option<ForegroundProcess> {
        None
    }
}
