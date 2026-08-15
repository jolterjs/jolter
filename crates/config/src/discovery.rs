use std::path::{Path, PathBuf};

use crate::CONFIG_FILE_NAME;

#[must_use]
pub fn discover(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|directory| directory.join(CONFIG_FILE_NAME))
        .find(|candidate| candidate.is_file())
}
