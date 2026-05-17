use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::logging;
use crate::storage::fs_utils::{atomic_write_private, preserve_corrupt_file};

pub fn read_optional_string(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn write_toml_private<T>(path: &Path, document: &T) -> io::Result<()>
where
    T: Serialize,
{
    let serialized =
        toml::to_string_pretty(document).map_err(|error| io::Error::other(error.to_string()))?;
    atomic_write_private(path, &serialized)
}

pub fn preserve_corrupt_warning(path: &Path, message: &str) -> String {
    let warning = match preserve_corrupt_file(path) {
        Ok(Some(preserved)) => format!("{message} Recovery copy: {}.", preserved.display()),
        Ok(None) => message.to_string(),
        Err(error) => format!(
            "{message} TerminalTiler could not preserve the original file: {}.",
            error
        ),
    };
    logging::error(&warning);
    warning
}
