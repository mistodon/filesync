//! Provides a FileSource for a path in an S3 bucket.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use aws_sdk_s3::Client;
use chrono::{DateTime, NaiveDateTime, Utc};
use thiserror::Error as ErrorTrait;

use crate::{FileEntry, FileSource};

/// Error type for `S3Files` errors.
#[derive(Debug, ErrorTrait)]
pub enum S3Error {
    #[error("One of the objects returned does not have a key")]
    ObjectMissingKey,

    #[error("One of the objects returned has an incorrect prefix")]
    ObjectWrongPrefix,

    #[error(transparent)]
    ByteStreamError(#[from] aws_sdk_s3::primitives::ByteStreamError),

    #[error(transparent)]
    S3Error(#[from] aws_sdk_s3::Error),
}

/// A [`FileSource`] for files under a path in an S3 bucket.
///
/// Depends on the `aws-sdk-s3` crate to read and write files.
pub struct S3Files {
    client: Client,
    bucket: String,
    prefix: PathBuf,
    use_etag_as_hash: bool,
}

impl S3Files {
    /// Create a new `S3Files` for a path in an S3 bucket.
    ///
    /// If the `use_etag_as_hash` flag is set, the ETag of each S3 object will be assumed to
    /// be an MD5 hash of the contents (if it is a 128 hex value).
    pub fn new<S: AsRef<str>, P: AsRef<Path>>(
        client: Client,
        bucket: S,
        prefix: P,
        use_etag_as_hash: bool,
    ) -> Self {
        S3Files {
            client,
            bucket: bucket.as_ref().to_owned(),
            prefix: prefix.as_ref().to_owned(),
            use_etag_as_hash,
        }
    }
}

#[async_trait]
impl FileSource for S3Files {
    type Error = S3Error;

    async fn list_files(&mut self) -> Result<Vec<FileEntry>, Self::Error> {
        let empty_path: PathBuf = PathBuf::new();

        let response = self
            .client
            .list_objects_v2()
            .bucket(self.bucket.clone())
            .prefix(self.prefix.display().to_string())
            .send()
            .await
            .map_err(aws_sdk_s3::Error::from)?;

        let mut files = vec![];

        if let Some(contents) = response.contents {
            for object in contents {
                let key: PathBuf = object
                    .key
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or(S3Error::ObjectMissingKey)?
                    .strip_prefix(&self.prefix)
                    .map_err(|_| S3Error::ObjectWrongPrefix)?
                    .to_owned();

                if key != empty_path {
                    let modified = object.last_modified.and_then(|date_time| {
                        NaiveDateTime::from_timestamp_opt(
                            date_time.secs(),
                            date_time.subsec_nanos(),
                        )
                        .map(|x| x.and_utc())
                    });

                    let md5_hash = match self.use_etag_as_hash {
                        true => object.e_tag.and_then(|etag| {
                            let digest: Option<u128> =
                                u128::from_str_radix(etag.trim_matches('"'), 16).ok();

                            digest
                        }),
                        false => None,
                    };

                    files.push(FileEntry {
                        path: key,
                        size: u64::try_from(object.size).ok(),
                        modified,
                        md5_hash,
                    });
                }
            }
        }

        Ok(files)
    }

    async fn read_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<Vec<u8>, Self::Error> {
        let mut key = self.prefix.clone();
        key.push(path.as_ref());
        let key = key.display().to_string();

        let output = self
            .client
            .get_object()
            .bucket(self.bucket.clone())
            .key(key)
            .send()
            .await
            .map_err(aws_sdk_s3::Error::from)?;

        let stream = output.body.collect().await?.to_vec();

        Ok(stream)
    }

    async fn write_file<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
        bytes: &[u8],
    ) -> Result<(), Self::Error> {
        let mut key = self.prefix.clone();
        key.push(path.as_ref());
        let key = key.display().to_string();

        let stream = aws_sdk_s3::primitives::ByteStream::from(bytes.to_owned());

        self.client
            .put_object()
            .bucket(self.bucket.clone())
            .key(key)
            .body(stream)
            .send()
            .await
            .map_err(aws_sdk_s3::Error::from)?;

        Ok(())
    }

    async fn set_modified<P: AsRef<Path> + Send>(
        &mut self,
        _path: P,
        _modified: Option<DateTime<Utc>>,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
}
