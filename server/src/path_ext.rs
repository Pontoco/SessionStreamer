use std::path::PathBuf;

use thiserror::Error;

pub trait PathExt {
    fn join_safe<P>(&self, relative: P) -> Result<PathBuf, SafeJoinErr>
    where
        P: AsRef<std::path::Path>;
}

#[derive(Error, Debug)]
pub enum SafeJoinErr {
    #[error("joined directory must only contain simple components (no .., etc)")]
    DirContainsInvalidComponents,
}

impl<P> PathExt for P
where
    P: AsRef<std::path::Path>,
{
    // to prevent directory traversal attacks we ensure the path consists of exactly one normal
    // component
    fn join_safe<P2>(&self, relative: P2) -> Result<PathBuf, SafeJoinErr>
    where
        P2: AsRef<std::path::Path>,
    {
        let mut components = relative.as_ref().components().peekable();
        if !components.all(|component| matches!(component, std::path::Component::Normal(_))) {
            return Err(SafeJoinErr::DirContainsInvalidComponents);
        }

        Ok(self.as_ref().join(relative))
    }
}
