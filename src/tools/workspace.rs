//! Canonical workspace-root enforcement shared by filesystem-facing tools.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;

use thiserror::Error;

/// The stable agent-facing error returned when a tool path leaves its workspace.
pub const WORKSPACE_DENIAL: &str = "Error: workspace boundary denied path";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRootSource {
    Primary,
    ProjectConfig,
    Interactive,
}

impl WorkspaceRootSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::ProjectConfig => "project config",
            Self::Interactive => "session",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoot {
    path: PathBuf,
    source: WorkspaceRootSource,
}

impl WorkspaceRoot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> WorkspaceRootSource {
        self.source
    }
}

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
    additional_roots: Vec<WorkspaceRoot>,
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
            .chain(self.additional_roots.iter().map(WorkspaceRoot::path))
    }

    pub fn roots_with_sources(&self) -> impl Iterator<Item = WorkspaceRoot> + '_ {
        std::iter::once(WorkspaceRoot {
            path: self.root.clone(),
            source: WorkspaceRootSource::Primary,
        })
        .chain(self.additional_roots.iter().cloned())
    }

    /// Heddle-owned mutable runtime state for the lifetime of this boundary.
    /// It is deliberately not a workspace root and is therefore unavailable to
    /// filesystem-facing agent tools.
    pub(crate) fn runtime_root(&self) -> &Path {
        self.runtime_root.path()
    }

    pub fn add_project_root(&mut self, raw: impl AsRef<Path>) -> Result<PathBuf, WorkspaceError> {
        self.add_root(raw, WorkspaceRootSource::ProjectConfig)
    }

    pub fn add_interactive_root(
        &mut self,
        raw: impl AsRef<Path>,
    ) -> Result<PathBuf, WorkspaceError> {
        self.add_root(raw, WorkspaceRootSource::Interactive)
    }

    fn add_root(
        &mut self,
        raw: impl AsRef<Path>,
        source: WorkspaceRootSource,
    ) -> Result<PathBuf, WorkspaceError> {
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
        if root != self.root && !self.additional_roots.iter().any(|entry| entry.path == root) {
            self.additional_roots.push(WorkspaceRoot {
                path: root.clone(),
                source,
            });
        }
        Ok(root)
    }

    pub fn remove_interactive_root(
        &mut self,
        raw: impl AsRef<Path>,
    ) -> Result<bool, WorkspaceError> {
        let raw = raw.as_ref();
        let candidate = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.root.join(raw)
        };
        let root = std::fs::canonicalize(candidate).map_err(|_| WorkspaceError::Unresolvable)?;
        let before = self.additional_roots.len();
        self.additional_roots
            .retain(|entry| entry.path != root || entry.source != WorkspaceRootSource::Interactive);
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

    #[test]
    fn keeps_project_roots_visible_and_session_roots_removable() {
        let primary = tempdir().unwrap();
        let project_root = tempdir().unwrap();
        let session_root = tempdir().unwrap();
        let mut boundary = WorkspaceBoundary::new(primary.path()).unwrap();

        boundary.add_project_root(project_root.path()).unwrap();
        boundary.add_interactive_root(session_root.path()).unwrap();

        let roots = boundary.roots_with_sources().collect::<Vec<_>>();
        assert_eq!(roots[0].source(), WorkspaceRootSource::Primary);
        assert_eq!(roots[1].source(), WorkspaceRootSource::ProjectConfig);
        assert_eq!(roots[2].source(), WorkspaceRootSource::Interactive);
        assert!(!boundary
            .remove_interactive_root(project_root.path())
            .unwrap());
        assert!(boundary
            .remove_interactive_root(session_root.path())
            .unwrap());
    }
}
