//! Canonical workspace-root enforcement shared by filesystem-facing tools.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;

use thiserror::Error;

/// The stable agent-facing error returned when a tool path leaves its workspace.
pub const WORKSPACE_DENIAL: &str = "Error: workspace boundary denied path";

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("{WORKSPACE_DENIAL}: parent traversal is not allowed")]
    ParentTraversal,
    #[error("{WORKSPACE_DENIAL}: path is outside the configured workspace")]
    OutsideRoot,
    #[error("{WORKSPACE_DENIAL}: could not resolve path safely")]
    Unresolvable,
}

/// A canonical root with no implicit access to the process working directory.
#[derive(Debug, Clone)]
pub struct WorkspaceBoundary {
    root: PathBuf,
    additional_roots: Vec<PathBuf>,
    runtime_root: Arc<tempfile::TempDir>,
}

pub type SharedWorkspaceBoundary = Arc<RwLock<WorkspaceBoundary>>;

impl WorkspaceBoundary {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = std::fs::canonicalize(root).map_err(|_| WorkspaceError::Unresolvable)?;
        if !root.is_dir() {
            return Err(WorkspaceError::Unresolvable);
        }
        let runtime_root = tempfile::Builder::new()
            .prefix("heddle-runtime-")
            .tempdir()
            .map_err(|_| WorkspaceError::Unresolvable)?;
        Ok(Self {
            root,
            additional_roots: Vec::new(),
            runtime_root: Arc::new(runtime_root),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn roots(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.root.as_path())
            .chain(self.additional_roots.iter().map(PathBuf::as_path))
    }

    /// Heddle-owned mutable runtime state for the lifetime of this boundary.
    /// It is deliberately not a workspace root and is therefore unavailable to
    /// filesystem-facing agent tools.
    pub(crate) fn runtime_root(&self) -> &Path {
        self.runtime_root.path()
    }

    pub fn add_root(&mut self, raw: impl AsRef<Path>) -> Result<PathBuf, WorkspaceError> {
        let raw = raw.as_ref();
        let candidate = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.root.join(raw)
        };
        let root = std::fs::canonicalize(candidate).map_err(|_| WorkspaceError::Unresolvable)?;
        if !root.is_dir() {
            return Err(WorkspaceError::Unresolvable);
        }
        if root != self.root && !self.additional_roots.contains(&root) {
            self.additional_roots.push(root.clone());
        }
        Ok(root)
    }

    pub fn remove_root(&mut self, raw: impl AsRef<Path>) -> Result<bool, WorkspaceError> {
        let raw = raw.as_ref();
        let candidate = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.root.join(raw)
        };
        let root = std::fs::canonicalize(candidate).map_err(|_| WorkspaceError::Unresolvable)?;
        let before = self.additional_roots.len();
        self.additional_roots.retain(|entry| entry != &root);
        Ok(before != self.additional_roots.len())
    }

    /// Resolves a path without permitting lexical `..` components or symlink
    /// escapes. For a path that does not yet exist, its nearest existing parent
    /// is canonicalized before the missing suffix is appended.
    pub fn resolve(&self, raw: impl AsRef<Path>) -> Result<PathBuf, WorkspaceError> {
        let raw = raw.as_ref();
        if raw
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(WorkspaceError::ParentTraversal);
        }
        let candidate = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.root.join(raw)
        };
        let resolved = canonicalize_with_missing_suffix(&candidate)?;
        if self.roots().any(|root| resolved.starts_with(root)) {
            Ok(resolved)
        } else {
            Err(WorkspaceError::OutsideRoot)
        }
    }
}

fn canonicalize_with_missing_suffix(path: &Path) -> Result<PathBuf, WorkspaceError> {
    let mut existing = path;
    let mut suffix = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or(WorkspaceError::Unresolvable)?;
        suffix.push(name.to_os_string());
        existing = existing.parent().ok_or(WorkspaceError::Unresolvable)?;
    }
    let mut canonical =
        std::fs::canonicalize(existing).map_err(|_| WorkspaceError::Unresolvable)?;
    for component in suffix.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_parent_traversal_and_symlink_escapes() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        let boundary = WorkspaceBoundary::new(root.path()).unwrap();

        assert!(matches!(
            boundary.resolve("../secret"),
            Err(WorkspaceError::ParentTraversal)
        ));
        assert!(matches!(
            boundary.resolve(root.path().join("escape/secret")),
            Err(WorkspaceError::OutsideRoot)
        ));
        assert!(matches!(
            boundary.resolve(boundary.runtime_root()),
            Err(WorkspaceError::OutsideRoot)
        ));
    }
}
