use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslUncPath {
    pub distro: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WslPathTranslationError {
    EmptyPath,
    UnexpectedDistro { actual: String, expected: String },
    UnsupportedPath(String),
}

impl fmt::Display for WslPathTranslationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => formatter.write_str("path is empty"),
            Self::UnexpectedDistro { actual, expected } => write!(
                formatter,
                "WSL UNC path targets distro '{}' instead of '{}'",
                actual, expected
            ),
            Self::UnsupportedPath(path) => write!(formatter, "unsupported WSL path '{}'", path),
        }
    }
}

impl Error for WslPathTranslationError {}

pub fn translate_path_for_wsl(
    path: &str,
    expected_distro: &str,
) -> Result<String, WslPathTranslationError> {
    if path.trim().is_empty() {
        return Err(WslPathTranslationError::EmptyPath);
    }

    if let Some(unc) = parse_wsl_unc_path(path) {
        if !unc.distro.eq_ignore_ascii_case(expected_distro) {
            return Err(WslPathTranslationError::UnexpectedDistro {
                actual: unc.distro,
                expected: expected_distro.to_string(),
            });
        }
        return Ok(unc.path);
    }

    if looks_like_wsl_absolute_path(path) {
        return Ok(path.replace('\\', "/"));
    }

    if let Some(wsl_path) = translate_windows_drive_path(path) {
        return Ok(wsl_path);
    }

    Err(WslPathTranslationError::UnsupportedPath(path.to_string()))
}

pub fn parse_wsl_unc_path(path: &str) -> Option<WslUncPath> {
    let trimmed = path.trim();
    let normalized = trimmed.replace('/', "\\");
    let prefix = "\\\\wsl$\\";
    let suffix = normalized.strip_prefix(prefix)?;
    let (distro, remainder) = suffix.split_once('\\')?;
    let remainder = remainder.trim_start_matches('\\');

    Some(WslUncPath {
        distro: distro.to_string(),
        path: if remainder.is_empty() {
            "/".into()
        } else {
            format!("/{}", remainder.replace('\\', "/"))
        },
    })
}

pub fn looks_like_wsl_absolute_path(path: &str) -> bool {
    let trimmed = path.trim();
    trimmed.starts_with('/') && !trimmed.starts_with("//")
}

fn translate_windows_drive_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || (bytes[2] != b'\\' && bytes[2] != b'/')
    {
        return None;
    }

    let drive = (bytes[0] as char).to_ascii_lowercase();
    let remainder = trimmed[3..].replace('\\', "/");
    if remainder.is_empty() {
        Some(format!("/mnt/{drive}"))
    } else {
        Some(format!("/mnt/{drive}/{remainder}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{WslPathTranslationError, parse_wsl_unc_path, translate_path_for_wsl};

    #[test]
    fn parses_matching_wsl_unc_paths() {
        let unc = parse_wsl_unc_path(r"\\wsl$\Ubuntu\home\dev\project").unwrap();

        assert_eq!(unc.distro, "Ubuntu");
        assert_eq!(unc.path, "/home/dev/project");
    }

    #[test]
    fn translates_windows_drive_paths_to_wsl_mounts() {
        assert_eq!(
            translate_path_for_wsl(r"C:\Users\dev\project", "Ubuntu").unwrap(),
            "/mnt/c/Users/dev/project"
        );
        assert_eq!(
            translate_path_for_wsl("D:/work/tree", "Ubuntu").unwrap(),
            "/mnt/d/work/tree"
        );
    }

    #[test]
    fn preserves_raw_wsl_absolute_paths() {
        assert_eq!(
            translate_path_for_wsl("/home/dev/project", "Ubuntu").unwrap(),
            "/home/dev/project"
        );
    }

    #[test]
    fn rejects_cross_distro_unc_paths() {
        let error = translate_path_for_wsl(r"\\wsl$\Debian\home\dev", "Ubuntu")
            .expect_err("cross-distro path should fail");

        assert_eq!(
            error,
            WslPathTranslationError::UnexpectedDistro {
                actual: "Debian".into(),
                expected: "Ubuntu".into(),
            }
        );
    }
}
