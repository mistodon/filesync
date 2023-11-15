#![cfg(feature = "s3_integration_test")]

use std::path::PathBuf;

use pretty_assertions::assert_eq;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn s3_integration_test() -> Result<()> {
    use tokio::runtime::Runtime;

    let rt = Runtime::new().unwrap();
    rt.block_on(run_test())?;
    Ok(())
}

async fn run_test() -> Result<()> {
    clear_files()?;
    setup_files()?;

    let bucket = env!("TARGET_BUCKET");
    let commit = env!("COMMIT_HASH");
    assert!(!commit.is_empty());
    eprintln!("Commit hash is: {}", commit);

    let mut prefix = PathBuf::from(env!("TARGET_PREFIX"));
    prefix.push(commit);

    let config = aws_config::load_from_env().await;
    let client = aws_sdk_s3::Client::new(&config);

    let mut local = filesync::local::LocalFiles::new("./temp/s3_test", true);
    let mut s3 = filesync::s3::S3Files::new(client, bucket, prefix, true);

    eprintln!("1. Syncing initial files to S3");
    let mut synced_paths = filesync::sync_one_way(&mut local, &mut s3).await?;
    std::thread::sleep(std::time::Duration::from_secs(2));

    synced_paths.sort();
    assert_eq!(
        dbg!(synced_paths),
        vec![
            PathBuf::from("folder/four_changes.txt"),
            PathBuf::from("folder/three_no_changes.txt"),
            PathBuf::from("one_no_changes.txt"),
            PathBuf::from("two_changes.txt"),
        ]
    );

    eprintln!("2. Deleting some local files");
    std::fs::remove_file("./temp/s3_test/two_changes.txt")?;
    std::fs::remove_dir_all("./temp/s3_test/folder")?;

    eprintln!("3. Restoring files from S3");
    let mut synced_paths = filesync::sync_one_way(&mut s3, &mut local).await?;
    std::thread::sleep(std::time::Duration::from_secs(2));

    synced_paths.sort();
    assert_eq!(
        dbg!(synced_paths),
        vec![
            PathBuf::from("folder/four_changes.txt"),
            PathBuf::from("folder/three_no_changes.txt"),
            PathBuf::from("two_changes.txt"),
        ]
    );

    eprintln!("4. Modifying some local files");
    std::fs::write("./temp/s3_test/two_changes.txt", "two up-to-date")?;
    std::fs::write("./temp/s3_test/folder/four_changes.txt", "four up-to-date")?;

    eprintln!("5. Syncing changes to S3");
    let mut synced_paths = filesync::sync_one_way(&mut local, &mut s3).await?;
    std::thread::sleep(std::time::Duration::from_secs(2));

    synced_paths.sort();
    assert_eq!(
        dbg!(synced_paths),
        vec![
            PathBuf::from("folder/four_changes.txt"),
            PathBuf::from("two_changes.txt"),
        ]
    );

    eprintln!("6. Verifying contents of S3");
    use filesync::FileSource;
    let files = [
        s3.read_file("one_no_changes.txt").await?,
        s3.read_file("two_changes.txt").await?,
        s3.read_file("folder/three_no_changes.txt").await?,
        s3.read_file("folder/four_changes.txt").await?,
    ];

    assert_eq!(
        files,
        [
            b"one up-to-date".to_vec(),
            b"two up-to-date".to_vec(),
            b"three up-to-date".to_vec(),
            b"four up-to-date".to_vec(),
        ]
    );

    Ok(())
}

fn clear_files() -> Result<()> {
    let path: &std::path::Path = "./temp/s3_test".as_ref();
    if path.exists() {
        std::fs::remove_dir_all("./temp/s3_test")?;
    }
    Ok(())
}

fn setup_files() -> Result<()> {
    std::fs::create_dir_all("./temp/s3_test/folder")?;

    std::fs::write("./temp/s3_test/one_no_changes.txt", "one up-to-date")?;
    std::fs::write("./temp/s3_test/two_changes.txt", "two old")?;

    std::fs::write(
        "./temp/s3_test/folder/three_no_changes.txt",
        "three up-to-date",
    )?;
    std::fs::write("./temp/s3_test/folder/four_changes.txt", "four old")?;

    Ok(())
}
