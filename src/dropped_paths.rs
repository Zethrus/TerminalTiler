#[cfg(any(target_os = "windows", test))]
use crate::platform::translate_path_for_wsl;

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DroppedPathTarget<'a> {
    Posix,
    PowerShell,
    Wsl { distro: &'a str },
}

#[cfg(any(target_os = "windows", test))]
pub fn serialize_for_target<I, S>(
    paths: I,
    target: DroppedPathTarget<'_>,
) -> (Option<String>, Vec<String>)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    match target {
        DroppedPathTarget::Posix => (serialize_posix_paths(paths), Vec::new()),
        DroppedPathTarget::PowerShell => (serialize_powershell_paths(paths), Vec::new()),
        DroppedPathTarget::Wsl { distro } => serialize_wsl_paths(paths, distro),
    }
}

pub fn serialize_posix_paths<I, S>(paths: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    serialize_paths(paths, posix_quote_path)
}

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
pub fn serialize_powershell_paths<I, S>(paths: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    serialize_paths(paths, powershell_quote_path)
}

#[cfg(any(target_os = "windows", test))]
pub fn serialize_wsl_paths<I, S>(paths: I, distro: &str) -> (Option<String>, Vec<String>)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut translated_paths = Vec::new();
    let mut errors = Vec::new();

    for path in paths {
        let path = path.as_ref();
        if path.is_empty() {
            continue;
        }
        match translate_path_for_wsl(path, distro) {
            Ok(translated) => translated_paths.push(translated),
            Err(error) => errors.push(error.to_string()),
        }
    }

    (
        serialize_posix_paths(translated_paths.iter().map(String::as_str)),
        errors,
    )
}

fn serialize_paths<I, S>(paths: I, quote_path: fn(&str) -> String) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let serialized = paths
        .into_iter()
        .map(|path| path.as_ref().to_string())
        .filter(|path| !path.is_empty())
        .map(|path| quote_path(&path))
        .collect::<Vec<_>>();

    if serialized.is_empty() {
        None
    } else {
        Some(format!("{} ", serialized.join(" ")))
    }
}

fn posix_quote_path(path: &str) -> String {
    let escaped = path.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
fn powershell_quote_path(path: &str) -> String {
    let escaped = path.replace('\'', "''");
    format!("'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::{
        DroppedPathTarget, serialize_for_target, serialize_posix_paths, serialize_powershell_paths,
        serialize_wsl_paths,
    };

    #[test]
    fn serializes_single_posix_path_for_shell_paste() {
        let payload = serialize_posix_paths(["/tmp/report.txt"]);

        assert_eq!(payload.as_deref(), Some("'/tmp/report.txt' "));
    }

    #[test]
    fn serializes_multiple_posix_paths_with_spaces() {
        let payload = serialize_posix_paths(["/tmp/screenshot 1.png", "/workspace/notes.md"]);

        assert_eq!(
            payload.as_deref(),
            Some("'/tmp/screenshot 1.png' '/workspace/notes.md' ")
        );
    }

    #[test]
    fn escapes_single_quotes_in_posix_paths() {
        let payload = serialize_posix_paths(["/tmp/it's-here.txt"]);

        assert_eq!(payload.as_deref(), Some("'/tmp/it'\"'\"'s-here.txt' "));
    }

    #[test]
    fn ignores_empty_posix_drop_payloads() {
        let payload = serialize_posix_paths([""]);

        assert_eq!(payload, None);
    }

    #[test]
    fn serializes_powershell_paths_with_spaces() {
        let payload = serialize_powershell_paths([r"C:\Users\me\a b.png"]);

        assert_eq!(payload.as_deref(), Some(r"'C:\Users\me\a b.png' "));
    }

    #[test]
    fn escapes_single_quotes_in_powershell_paths() {
        let payload = serialize_powershell_paths([r"C:\Users\me\it isn't.txt"]);

        assert_eq!(payload.as_deref(), Some(r"'C:\Users\me\it isn''t.txt' "));
    }

    #[test]
    fn serializes_multiple_powershell_paths() {
        let payload = serialize_powershell_paths([r"C:\one.txt", r"D:\two words.txt"]);

        assert_eq!(
            payload.as_deref(),
            Some(r"'C:\one.txt' 'D:\two words.txt' ")
        );
    }

    #[test]
    fn translates_windows_drive_paths_for_wsl_before_posix_quoting() {
        let (payload, errors) = serialize_wsl_paths([r"C:\Users\dev\a b.png"], "Ubuntu");

        assert!(errors.is_empty());
        assert_eq!(payload.as_deref(), Some("'/mnt/c/Users/dev/a b.png' "));
    }

    #[test]
    fn translates_matching_wsl_unc_paths_for_wsl() {
        let (payload, errors) = serialize_wsl_paths([r"\\wsl$\Ubuntu\home\dev\a b.png"], "Ubuntu");

        assert!(errors.is_empty());
        assert_eq!(payload.as_deref(), Some("'/home/dev/a b.png' "));
    }

    #[test]
    fn skips_unsupported_wsl_drop_paths_with_errors() {
        let (payload, errors) = serialize_wsl_paths([r"\\server\share\file.txt"], "Ubuntu");

        assert_eq!(payload, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("unsupported WSL path"));
    }

    #[test]
    fn serializes_multiple_wsl_paths_and_skips_bad_entries() {
        let (payload, errors) = serialize_wsl_paths(
            [
                r"C:\Users\dev\one.txt",
                r"\\server\share\bad.txt",
                r"D:\two.txt",
            ],
            "Ubuntu",
        );

        assert_eq!(
            payload.as_deref(),
            Some("'/mnt/c/Users/dev/one.txt' '/mnt/d/two.txt' ")
        );
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn target_serializer_uses_posix_quoting_for_ssh_local_path_text() {
        let (payload, errors) =
            serialize_for_target([r"C:\Users\me\a b.png"], DroppedPathTarget::Posix);

        assert!(errors.is_empty());
        assert_eq!(payload.as_deref(), Some(r"'C:\Users\me\a b.png' "));
    }

    #[test]
    fn target_serializer_uses_powershell_quoting_for_native_windows_panes() {
        let (payload, errors) =
            serialize_for_target([r"C:\Users\me\it isn't.txt"], DroppedPathTarget::PowerShell);

        assert!(errors.is_empty());
        assert_eq!(payload.as_deref(), Some(r"'C:\Users\me\it isn''t.txt' "));
    }

    #[test]
    fn target_serializer_translates_wsl_paths_for_wsl_panes() {
        let (payload, errors) = serialize_for_target(
            [r"C:\Users\dev\a b.png"],
            DroppedPathTarget::Wsl { distro: "Ubuntu" },
        );

        assert!(errors.is_empty());
        assert_eq!(payload.as_deref(), Some("'/mnt/c/Users/dev/a b.png' "));
    }
}
