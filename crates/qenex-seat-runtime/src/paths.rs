//! Path confinement for the seat workspace (R2).
//!
//! Every path a seat-bound runtime touches is resolved against the seat
//! workspace. Traversal, absolute paths and symlink escapes are refused with
//! distinct errors so the caller can tell them apart.

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Why a path was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// The requested path escaped the workspace through `..`.
    Traversal,
    /// The requested path was absolute.
    Absolute,
    /// The resolved path is outside the seat workspace.
    Outside,
    /// The resolved path left the workspace through a symlink.
    SymlinkEscape,
    /// The path does not exist.
    NotFound,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::Traversal => write!(f, "path traversal is not allowed inside a seat"),
            PathError::Absolute => write!(f, "absolute paths are not allowed inside a seat"),
            PathError::Outside => write!(f, "path is outside the seat workspace"),
            PathError::SymlinkEscape => {
                write!(f, "path leaves the seat workspace through a symlink")
            }
            PathError::NotFound => write!(f, "path does not exist"),
        }
    }
}

impl std::error::Error for PathError {}

/// Resolves paths inside one seat workspace.
#[derive(Debug, Clone)]
pub struct Confinement {
    workspace: PathBuf,
}

impl Confinement {
    /// Create a confinement rooted at `workspace`.
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
        }
    }

    /// The workspace root.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Resolve a workspace-relative path.
    ///
    /// Refuses `..` traversal, absolute paths and anything that resolves
    /// outside the workspace.
    pub fn resolve(&self, requested: &str) -> Result<PathBuf, PathError> {
        let raw = Path::new(requested);
        if raw.is_absolute() {
            return Err(PathError::Absolute);
        }
        if raw
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(PathError::Traversal);
        }
        let candidate = self.workspace.join(raw);
        let normalized = normalize(&candidate);
        if !normalized.starts_with(&self.workspace) {
            return Err(PathError::Outside);
        }
        Ok(normalized)
    }

    /// Resolve a path that is expected to exist, also refusing symlink escapes.
    pub fn resolve_existing(&self, requested: &str) -> Result<PathBuf, PathError> {
        let candidate = self.resolve(requested)?;
        let Ok(real) = candidate.canonicalize() else {
            return Err(PathError::NotFound);
        };
        let root = self
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone());
        if !real.starts_with(&root) {
            return Err(PathError::SymlinkEscape);
        }
        Ok(real)
    }

    /// Check that `path` may be written to.
    pub fn ensure_writable(&self, path: &Path) -> Result<(), PathError> {
        let normalized = normalize(path);
        if normalized.starts_with(&self.workspace) {
            return Ok(());
        }
        Err(PathError::Outside)
    }
}

/// Lexically normalize a path: drop `.` and collapse `..`.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confinement() -> Confinement {
        Confinement::new("/var/lib/qenex/seats/coder/workspace")
    }

    #[test]
    fn relative_path_resolves_inside_the_workspace() {
        let resolved = confinement()
            .resolve("src/main.rs")
            .expect("a relative path resolves");
        assert_eq!(
            resolved,
            Path::new("/var/lib/qenex/seats/coder/workspace/src/main.rs")
        );
    }

    #[test]
    fn traversal_is_refused() {
        assert_eq!(
            confinement().resolve("../../etc/passwd").unwrap_err(),
            PathError::Traversal
        );
        assert_eq!(confinement().resolve("..").unwrap_err(), PathError::Traversal);
    }

    #[test]
    fn absolute_paths_are_refused() {
        assert_eq!(
            confinement().resolve("/etc/passwd").unwrap_err(),
            PathError::Absolute
        );
    }

    #[test]
    fn writes_outside_the_workspace_are_refused() {
        assert_eq!(
            confinement()
                .ensure_writable(Path::new("/etc/rustdesk/config.toml"))
                .unwrap_err(),
            PathError::Outside
        );
        assert!(
            confinement()
                .ensure_writable(Path::new(
                    "/var/lib/qenex/seats/coder/workspace/out.txt"
                ))
                .is_ok()
        );
    }
}
