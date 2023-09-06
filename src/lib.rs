//! Crate for syncing files between (theoretically) arbitrary sources. For example,
//! syncing local files to an S3 bucket.
//!
//! # Example
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # use std::path::PathBuf;
//! use filesync::{
//!     local::LocalFiles,
//!     s3::S3Files,
//! };
//!
//! let config = aws_config::load_from_env().await;
//! let client = aws_sdk_s3::Client::new(&config);
//!
//! let mut local = LocalFiles::new("./my_local_files", true);
//! let mut s3 = S3Files::new(client, "my_s3_bucket", "path/in/bucket", true);
//!
//! let synced_paths = filesync::sync_one_way(&mut local, &mut s3).await?;
//! assert_eq!(synced_paths, vec![PathBuf::from("my_changed_file.txt")]);
//! # Ok(())
//! # }
//! ```

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    result::Result as StdResult,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error as ErrorTrait;

pub mod local;

#[cfg(feature = "s3")]
pub mod s3;

mod tests;

/// Error type for this crate.
#[derive(Debug, ErrorTrait)]
pub enum SyncError {
    #[error("Not enough metadata to tell if file `{}` has changed", filename.display())]
    NoMetadata { filename: PathBuf },

    #[error("Errors occurred while comparing files. No changes have been written:\n{}", errors.iter().map(SyncError::to_string).collect::<Vec<String>>().join("\n"))]
    ErrorComparing { errors: Vec<SyncError> },

    #[error(transparent)]
    FileSourceError(#[from] Box<dyn std::error::Error>),
}

impl SyncError {
    fn boxed<E: std::error::Error + 'static>(error: E) -> Self {
        SyncError::FileSourceError(Box::new(error))
    }
}

/// General result type for this crate.
pub type Result<T> = StdResult<T, SyncError>;

/// Represents a file at a path with some metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub modified: Option<DateTime<Utc>>,
    pub size: Option<u64>,
    pub md5_hash: Option<u128>,
}

impl FileEntry {
    /// Compares two files to see if `self` is an update to `other`.
    ///
    /// The rules are as following:
    ///
    /// 1. If the size and MD5 hash match, the files are considered to be the
    ///    same.
    /// 2. Failing that, if a modified time is present for both files, the most recent one takes
    ///    precedence.
    /// 3. Failing that, if either the size or MD5 hash are different, the files
    ///    are considered to be different.
    ///
    /// In the absence of a modified time, any change is considered to be an
    /// "update".
    pub fn is_changed_from(&self, other: &FileEntry) -> Result<bool> {
        let size_different = match (self.size, other.size) {
            (Some(a), Some(b)) => Some(a != b),
            _ => None,
        };

        let hash_different = match (self.md5_hash, other.md5_hash) {
            (Some(a), Some(b)) => Some(a != b),
            _ => None,
        };

        let date_later = match (self.modified, other.modified) {
            (Some(a), Some(b)) => Some(a > b),
            _ => None,
        };

        Ok(match (size_different, hash_different, date_later) {
            // Size and md5 unchanged -> file is unchanged
            (Some(false), Some(false), _) => false,

            // Next, modified date is the arbiter if present
            (_, _, Some(true)) => true,
            (_, _, Some(false)) => false,

            // Without a date present size/hash determine changes
            (Some(x), Some(y), None) => x || y,
            (Some(x), None, None) => x,
            (None, Some(x), None) => x,

            (None, None, None) => {
                return Err(SyncError::NoMetadata {
                    filename: self.path.clone(),
                })
            }
        })
    }
}

/// The trait that powers the sync function. Implemented using the [async_trait](https://docs.rs/async-trait/latest/async_trait/) crate.
///
/// You shouldn't need to manually deal with this trait unless you are implementing it for an
/// otherwise unsupported data storage.
#[async_trait]
pub trait FileSource {
    type Error: std::error::Error + 'static;

    /// Recursively list all files in the source.
    async fn list_files(&mut self) -> StdResult<Vec<FileEntry>, Self::Error>;

    /// Read a single file and return its contents as bytes.
    async fn read_file<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
    ) -> StdResult<Vec<u8>, Self::Error>;

    /// Write a single file.
    async fn write_file<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
        bytes: &[u8],
    ) -> StdResult<(), Self::Error>;

    /// Set the modified time, if provided, for a single file.
    ///
    /// Returns `true` if the time was set.
    async fn set_modified<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
        modified: Option<DateTime<Utc>>,
    ) -> StdResult<bool, Self::Error>;
}

/// Sync any new or modified files from one [`FileSource`] to another.
///
/// This will recursively list files from both sources and compare them. Any files in `from`
/// that are missing from `to` will be written.
///
/// Any files present in both source that differ will be written if the file in `from` is
/// considered to be more up-to-date than the one in `to`. (See
/// [`FileEntry::is_changed_from`].)
///
/// If a newer file is written over an older file, each copy will likely have a slightly
/// different modified timestamp. This function will then attempt to set one or the other so
/// that they match. This may not always be possible, depending on the type of [`FileSource`]
/// being used.
///
/// # Example
///
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # use std::path::PathBuf;
/// use filesync::{
///     local::LocalFiles,
///     s3::S3Files,
/// };
///
/// let config = aws_config::load_from_env().await;
/// let client = aws_sdk_s3::Client::new(&config);
///
/// let mut local = LocalFiles::new("./my_local_files", true);
/// let mut s3 = S3Files::new(client, "my_s3_bucket", "path/in/bucket", true);
///
/// let synced_paths = filesync::sync_one_way(&mut local, &mut s3).await?;
/// assert_eq!(synced_paths, vec![PathBuf::from("my_changed_file.txt")]);
/// # Ok(())
/// # }
/// ```
pub async fn sync_one_way<A, B>(from: &mut A, to: &mut B) -> Result<Vec<PathBuf>>
where
    A: FileSource,
    B: FileSource,
{
    let destination_files = to
        .list_files()
        .await
        .map_err(SyncError::boxed)?
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect::<HashMap<_, _>>();

    let source_files = from.list_files().await.map_err(SyncError::boxed)?;

    struct Write {
        path: PathBuf,
        src_modified: Option<DateTime<Utc>>,
        dst_modified: Option<DateTime<Utc>>,
    }

    let mut to_write: Vec<Write> = vec![];
    let mut errors: Vec<SyncError> = vec![];
    for source_file in &source_files {
        let path = &source_file.path;
        let matching = destination_files.get(path);
        match matching {
            Some(dest_file) => match source_file.is_changed_from(dest_file) {
                Ok(true) => to_write.push(Write {
                    path: path.to_owned(),
                    src_modified: source_file.modified,
                    dst_modified: dest_file.modified,
                }),
                Ok(false) => (),
                Err(err) => errors.push(err),
            },
            None => to_write.push(Write {
                path: path.to_owned(),
                src_modified: source_file.modified,
                dst_modified: None,
            }),
        }
    }

    if !errors.is_empty() {
        return Err(SyncError::ErrorComparing { errors });
    }

    for write in &to_write {
        let path = &write.path;
        let bytes = from.read_file(path).await.map_err(SyncError::boxed)?;
        to.write_file(path, &bytes)
            .await
            .map_err(SyncError::boxed)?;
        let dest_file_modified_time_updated = to
            .set_modified(path, write.src_modified)
            .await
            .map_err(SyncError::boxed)?;
        if !dest_file_modified_time_updated {
            from.set_modified(path, write.dst_modified)
                .await
                .map_err(SyncError::boxed)?;
        }
    }

    Ok(to_write.into_iter().map(|write| write.path).collect())
}
