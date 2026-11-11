//! A tiny filesystem abstraction so every collector can be tested against
//! in-memory string fixtures instead of the live `/proc` and `/sys`.

use std::io;
use std::path::Path;
use std::str::FromStr;

/// Size of a mounted filesystem, in bytes (what `statvfs` reports).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    pub total: u64,
    /// Free blocks, including those reserved for root.
    pub free: u64,
    /// Free blocks an unprivileged user may use.
    pub avail: u64,
}

/// The operations the collectors need: read a small text file, list the
/// entry names of a directory, and ask how full a mounted filesystem is.
pub trait Fs {
    fn read(&self, path: &Path) -> io::Result<String>;
    /// Entry names (not full paths) of `dir`, sorted.
    fn list(&self, dir: &Path) -> io::Result<Vec<String>>;
    /// Size of the filesystem mounted at `mount`.
    fn space(&self, mount: &Path) -> io::Result<Space>;
}

impl<T: Fs + ?Sized> Fs for &T {
    fn read(&self, path: &Path) -> io::Result<String> {
        (**self).read(path)
    }
    fn list(&self, dir: &Path) -> io::Result<Vec<String>> {
        (**self).list(dir)
    }
    fn space(&self, mount: &Path) -> io::Result<Space> {
        (**self).space(mount)
    }
}

/// The real filesystem, through `std::fs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs;

impl Fs for RealFs {
    fn read(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            if let Some(name) = entry?.file_name().to_str() {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    fn space(&self, mount: &Path) -> io::Result<Space> {
        let st = rustix::fs::statvfs(mount)?;
        let block = st.f_frsize;
        Ok(Space {
            total: st.f_blocks.saturating_mul(block),
            free: st.f_bfree.saturating_mul(block),
            avail: st.f_bavail.saturating_mul(block),
        })
    }
}

/// Reads a file and parses its trimmed contents. Any failure yields `None`.
pub fn read_parse<T: FromStr>(fs: &impl Fs, path: impl AsRef<Path>) -> Option<T> {
    fs.read(path.as_ref()).ok()?.trim().parse().ok()
}

#[cfg(test)]
pub use mem_fs::MemFs;

#[cfg(test)]
mod mem_fs {
    use super::{Fs, Space};
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::io;
    use std::path::{Path, PathBuf};

    /// In-memory filesystem for tests: a map from absolute path to contents.
    /// Directories exist implicitly whenever a file lives below them.
    #[derive(Debug, Default)]
    pub struct MemFs {
        files: RefCell<BTreeMap<PathBuf, String>>,
        spaces: RefCell<BTreeMap<PathBuf, Space>>,
    }

    impl MemFs {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn with(self, path: &str, contents: &str) -> Self {
            self.set(path, contents);
            self
        }

        pub fn set(&self, path: &str, contents: &str) {
            self.files
                .borrow_mut()
                .insert(PathBuf::from(path), contents.to_owned());
        }

        /// Makes `mount` report `total` bytes of which `used` are taken
        /// (and nothing reserved for root).
        pub fn with_space(self, mount: &str, total: u64, used: u64) -> Self {
            let free = total.saturating_sub(used);
            self.spaces.borrow_mut().insert(
                PathBuf::from(mount),
                Space {
                    total,
                    free,
                    avail: free,
                },
            );
            self
        }

        pub fn remove_prefix(&self, prefix: &str) {
            self.files
                .borrow_mut()
                .retain(|p, _| !p.starts_with(prefix));
        }
    }

    impl Fs for MemFs {
        fn read(&self, path: &Path) -> io::Result<String> {
            self.files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
        }

        fn list(&self, dir: &Path) -> io::Result<Vec<String>> {
            let names: BTreeSet<String> = self
                .files
                .borrow()
                .keys()
                .filter_map(|p| p.strip_prefix(dir).ok())
                .filter_map(|rest| rest.components().next())
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            if names.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    dir.display().to_string(),
                ));
            }
            Ok(names.into_iter().collect())
        }

        fn space(&self, mount: &Path) -> io::Result<Space> {
            self.spaces
                .borrow()
                .get(mount)
                .copied()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, mount.display().to_string()))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lists_immediate_children_only() {
            let fs = MemFs::new()
                .with("/a/b/c", "1")
                .with("/a/b/d", "2")
                .with("/a/e", "3");
            assert_eq!(fs.list(Path::new("/a")).unwrap(), vec!["b", "e"]);
            assert_eq!(fs.list(Path::new("/a/b")).unwrap(), vec!["c", "d"]);
            assert!(fs.list(Path::new("/zzz")).is_err());
            assert_eq!(fs.read(Path::new("/a/e")).unwrap(), "3");
        }
    }
}
