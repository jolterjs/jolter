use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DoctorError {
    #[error("failed to read diagnostics directory {path}: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Resolver(#[from] jolter_resolver::ResolverError),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
}
