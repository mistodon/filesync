//! Provides a FileSource for local files on disk.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use thiserror::Error as ErrorTrait;

use crate::{FileEntry, FileSource};

/// Error type for `LocalFiles` errors.
#[derive(Debug, ErrorTrait)]
pub enum LocalError {
    #[error(transparent)]
    Ignore(#[from] ignore::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A [`FileSource`] for local files on disk.
pub struct LocalFiles {
    root: PathBuf,
    compute_md5_hashes: bool,
}

impl LocalFiles {
    /// Create a new `LocalFiles` for a given path.
    ///
    /// If `compute_md5_hashes` is set, files will have their
    /// MD5 hashes computed when being listed.
    pub fn new<P: AsRef<Path>>(path: P, compute_md5_hashes: bool) -> Self {
        LocalFiles {
            root: path.as_ref().into(),
            compute_md5_hashes,
        }
    }

    fn list_files_sync(&mut self) -> Result<Vec<FileEntry>, LocalError> {
        let mut entries = vec![];

        for entry in ignore::WalkBuilder::new(&self.root).build() {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.file_type().is_file() {
                use std::time::SystemTime;

                let size = metadata.len();

                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|system_time| system_time.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .and_then(|duration| {
                        NaiveDateTime::from_timestamp_opt(
                            duration.as_secs() as i64,
                            duration.subsec_nanos(),
                        )
                        .map(|x| x.and_utc())
                    });

                let md5_hash = match self.compute_md5_hashes {
                    false => None,
                    true => Some({
                        let bytes = std::fs::read(entry.path())?;
                        let digest = md5::compute(bytes);
                        u128::from_be_bytes(digest.into())
                    }),
                };

                entries.push(FileEntry {
                    path: entry.path().strip_prefix(&self.root).unwrap().to_owned(),
                    modified,
                    size: Some(size),
                    md5_hash,
                });
            }
        }

        Ok(entries)
    }

    fn read_file_sync(&mut self, path: &Path) -> Result<Vec<u8>, LocalError> {
        let mut filepath = self.root.clone();
        filepath.push(path);

        Ok(std::fs::read(&filepath)?)
    }

    fn write_file_sync(&mut self, path: &Path, bytes: &[u8]) -> Result<(), LocalError> {
        let mut filepath = self.root.clone();
        filepath.push(path);

        if let Some(path) = filepath.parent() {
            std::fs::create_dir_all(path)?;
        }

        Ok(std::fs::write(&filepath, bytes)?)
    }

    fn set_modified_sync(
        &mut self,
        path: &Path,
        modified: Option<DateTime<Utc>>,
    ) -> Result<bool, LocalError> {
        use filetime::FileTime;

        if let Some(modified) = modified {
            let mut filepath = self.root.clone();
            filepath.push(path);

            let time =
                FileTime::from_unix_time(modified.timestamp(), modified.timestamp_subsec_nanos());
            filetime::set_file_mtime(&filepath, time)?;

            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[async_trait]
impl FileSource for LocalFiles {
    type Error = LocalError;

    async fn list_files(&mut self) -> Result<Vec<FileEntry>, Self::Error> {
        self.list_files_sync()
    }

    async fn read_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<Vec<u8>, Self::Error> {
        self.read_file_sync(path.as_ref())
    }

    async fn write_file<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
        bytes: &[u8],
    ) -> Result<(), Self::Error> {
        self.write_file_sync(path.as_ref(), bytes)
    }

    async fn set_modified<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
        modified: Option<DateTime<Utc>>,
    ) -> Result<bool, Self::Error> {
        self.set_modified_sync(path.as_ref(), modified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_files() {
        let mut fs = LocalFiles::new("./src", false);
        let files = fs.list_files_sync().unwrap();
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn read_write_roundtrip() {
        let temp: &Path = "./temp/local".as_ref();
        let tempfile: &Path = "tempfile".as_ref();

        // Prepare directories
        if temp.exists() {
            std::fs::remove_dir_all(temp).unwrap();
        }
        std::fs::create_dir(temp).unwrap();

        // Create FileSource
        let mut fs = LocalFiles::new("./temp/local", false);

        // Assert emptiness
        assert!(fs.read_file_sync(tempfile).is_err());
        assert!(std::fs::read_to_string("./temp/local/tempfile").is_err());

        // Write a file
        fs.write_file_sync(tempfile, b"Hello").unwrap();

        // Assert contents exist
        assert_eq!(fs.read_file_sync(tempfile).unwrap(), b"Hello");

        let bytes = std::fs::read_to_string("./temp/local/tempfile").unwrap();
        assert_eq!(bytes, "Hello");
    }
}
