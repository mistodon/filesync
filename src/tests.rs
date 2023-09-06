#![cfg(test)]

use std::{
    path::Path,
    sync::{atomic::AtomicU64, Arc},
};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use pretty_assertions::assert_eq;
use thiserror::Error as ErrorTrait;

use crate::{FileEntry, FileSource};

#[derive(Debug, ErrorTrait)]
#[error("Some error occurred.")]
pub struct TestError;

pub struct TestSource {
    files: Vec<(FileEntry, Vec<u8>)>,
    clock: Option<Arc<AtomicU64>>,
    use_hashes: bool,
}

impl TestSource {
    pub fn new(clock: Option<Arc<AtomicU64>>, use_hashes: bool) -> Self {
        TestSource {
            files: vec![],
            clock,
            use_hashes,
        }
    }
}

#[async_trait]
impl FileSource for TestSource {
    type Error = TestError;

    async fn list_files(&mut self) -> Result<Vec<FileEntry>, Self::Error> {
        Ok(self.files.iter().map(|x| x.0.clone()).collect())
    }

    async fn read_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<Vec<u8>, Self::Error> {
        Ok(self
            .files
            .iter()
            .find(|x| x.0.path == path.as_ref())
            .unwrap()
            .1
            .clone())
    }

    async fn write_file<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
        bytes: &[u8],
    ) -> Result<(), Self::Error> {
        let path = path.as_ref();
        let modified = self.clock.as_ref().map(|clock| {
            let time = clock.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let base_date = chrono::Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
            base_date.checked_add_days(chrono::Days::new(time)).unwrap()
        });

        let md5_hash = self.use_hashes.then(|| {
            let digest = md5::compute(bytes);
            let bytes: [u8; 16] = digest.into();
            u128::from_be_bytes(bytes)
        });

        self.files.retain(|entry| entry.0.path != path);
        self.files.push((
            FileEntry {
                path: path.to_owned(),
                size: Some(bytes.len() as u64),
                modified,
                md5_hash,
            },
            bytes.to_owned(),
        ));

        Ok(())
    }

    async fn set_modified<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
        modified: Option<DateTime<Utc>>,
    ) -> Result<bool, Self::Error> {
        let entry = self.files.iter_mut().find(|x| x.0.path == path.as_ref());
        if let (Some(entry), Some(modified)) = (entry, modified) {
            entry.0.modified = Some(modified);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[test]
fn sync_nothing_to_nothing() {
    let clock = Arc::new(AtomicU64::new(0));

    let mut from = TestSource::new(Some(Arc::clone(&clock)), false);
    let mut to = TestSource::new(Some(Arc::clone(&clock)), false);

    pollster::block_on(crate::sync_one_way(&mut from, &mut to)).unwrap();

    assert_eq!(&from.files, &[]);
    assert_eq!(&to.files, &[]);
}

#[test]
fn sync_file_to_nothing() {
    let clock = Arc::new(AtomicU64::new(0));

    let mut from = TestSource::new(Some(Arc::clone(&clock)), false);
    pollster::block_on(from.write_file("one.txt", b"one")).unwrap();

    let mut to = TestSource::new(Some(Arc::clone(&clock)), false);

    pollster::block_on(crate::sync_one_way(&mut from, &mut to)).unwrap();

    assert_eq!(
        &from.files,
        &[(
            FileEntry {
                path: "one.txt".into(),
                size: Some(3),
                modified: Some(chrono::Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap()),
                md5_hash: None,
            },
            b"one".to_vec()
        )]
    );
    assert_eq!(
        &to.files,
        &[(
            FileEntry {
                path: "one.txt".into(),
                size: Some(3),
                modified: Some(chrono::Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap()),
                md5_hash: None,
            },
            b"one".to_vec()
        )]
    );
}

#[test]
fn only_sync_more_recent_files() {
    let clock = Arc::new(AtomicU64::new(0));

    let mut from = TestSource::new(Some(Arc::clone(&clock)), false);
    let mut to = TestSource::new(Some(Arc::clone(&clock)), false);

    pollster::block_on(from.write_file("first_in_from.txt", b"old")).unwrap();
    pollster::block_on(to.write_file("first_in_to.txt", b"old")).unwrap();

    pollster::block_on(to.write_file("first_in_from.txt", b"changed")).unwrap();
    pollster::block_on(from.write_file("first_in_to.txt", b"changed")).unwrap();

    pollster::block_on(crate::sync_one_way(&mut from, &mut to)).unwrap();

    assert_eq!(
        &from.files,
        &[
            (
                FileEntry {
                    path: "first_in_from.txt".into(),
                    size: Some(3),
                    modified: Some(chrono::Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap()),
                    md5_hash: None,
                },
                b"old".to_vec()
            ),
            (
                FileEntry {
                    path: "first_in_to.txt".into(),
                    size: Some(7),
                    modified: Some(chrono::Utc.with_ymd_and_hms(2000, 1, 4, 0, 0, 0).unwrap()),
                    md5_hash: None,
                },
                b"changed".to_vec()
            ),
        ]
    );

    assert_eq!(
        &to.files,
        &[
            (
                FileEntry {
                    path: "first_in_from.txt".into(),
                    size: Some(7),
                    modified: Some(chrono::Utc.with_ymd_and_hms(2000, 1, 3, 0, 0, 0).unwrap()),
                    md5_hash: None,
                },
                b"changed".to_vec()
            ),
            (
                FileEntry {
                    path: "first_in_to.txt".into(),
                    size: Some(7),
                    modified: Some(chrono::Utc.with_ymd_and_hms(2000, 1, 4, 0, 0, 0).unwrap()),
                    md5_hash: None,
                },
                b"changed".to_vec()
            ),
        ]
    );
}

#[test]
fn sync_based_on_size_if_lacking_timestamps() {
    let mut from = TestSource::new(None, false);
    let mut to = TestSource::new(None, false);

    pollster::block_on(from.write_file("one.txt", b"on")).unwrap();
    pollster::block_on(from.write_file("two.txt", b"too")).unwrap();
    pollster::block_on(from.write_file("three.txt", b"threeee")).unwrap();

    pollster::block_on(to.write_file("one.txt", b"one")).unwrap();
    pollster::block_on(to.write_file("two.txt", b"two")).unwrap();
    pollster::block_on(to.write_file("three.txt", b"three")).unwrap();

    pollster::block_on(crate::sync_one_way(&mut from, &mut to)).unwrap();

    assert_eq!(
        &to.files,
        &[
            (
                FileEntry {
                    path: "two.txt".into(),
                    size: Some(3),
                    modified: None,
                    md5_hash: None,
                },
                b"two".to_vec()
            ),
            (
                FileEntry {
                    path: "one.txt".into(),
                    size: Some(2),
                    modified: None,
                    md5_hash: None,
                },
                b"on".to_vec()
            ),
            (
                FileEntry {
                    path: "three.txt".into(),
                    size: Some(7),
                    modified: None,
                    md5_hash: None,
                },
                b"threeee".to_vec()
            ),
        ]
    );
}

#[test]
fn sync_based_on_hash_if_size_fails() {
    let mut from = TestSource::new(None, true);
    let mut to = TestSource::new(None, true);

    pollster::block_on(from.write_file("one.txt", b"won")).unwrap();
    pollster::block_on(from.write_file("two.txt", b"two")).unwrap();

    pollster::block_on(to.write_file("one.txt", b"one")).unwrap();
    pollster::block_on(to.write_file("two.txt", b"two")).unwrap();

    pollster::block_on(crate::sync_one_way(&mut from, &mut to)).unwrap();

    // NOTE: The order proves that `two` was not written.
    assert_eq!(
        &to.files,
        &[
            (
                FileEntry {
                    path: "two.txt".into(),
                    size: Some(3),
                    modified: None,
                    md5_hash: Some(245460460880478039906047464050106960481),
                },
                b"two".to_vec()
            ),
            (
                FileEntry {
                    path: "one.txt".into(),
                    size: Some(3),
                    modified: None,
                    md5_hash: Some(164013335976871257125904378601358726325),
                },
                b"won".to_vec()
            ),
        ]
    );
}

#[test]
fn size_and_hash_matching_bypasses_modified_date() {
    let mut from = TestSource::new(None, true);
    let mut to = TestSource::new(None, true);

    pollster::block_on(to.write_file("one.txt", b"one")).unwrap();
    pollster::block_on(to.write_file("two.txt", b"two")).unwrap();

    pollster::block_on(from.write_file("one.txt", b"one")).unwrap();

    pollster::block_on(crate::sync_one_way(&mut from, &mut to)).unwrap();

    // NOTE: The order proves that `one` was not written.
    assert_eq!(
        &to.files,
        &[
            (
                FileEntry {
                    path: "one.txt".into(),
                    size: Some(3),
                    modified: None,
                    md5_hash: Some(331623505319187781935359225974189632386),
                },
                b"one".to_vec()
            ),
            (
                FileEntry {
                    path: "two.txt".into(),
                    size: Some(3),
                    modified: None,
                    md5_hash: Some(245460460880478039906047464050106960481),
                },
                b"two".to_vec()
            ),
        ]
    );
}
