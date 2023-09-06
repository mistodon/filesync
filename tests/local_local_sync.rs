pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn local_to_local() -> Result<()> {
    clear_files()?;
    setup_files()?;

    let mut local_a = filesync::local::LocalFiles::new("./temp/local_local_sync/local_a", false);
    let mut local_b = filesync::local::LocalFiles::new("./temp/local_local_sync/local_b", false);

    pollster::block_on(filesync::sync_one_way(&mut local_a, &mut local_b))?;

    let files = [
        std::fs::read_to_string("./temp/local_local_sync/local_b/file_a.txt")?,
        std::fs::read_to_string("./temp/local_local_sync/local_b/file_b.txt")?,
        std::fs::read_to_string("./temp/local_local_sync/local_b/file_c.txt")?,
    ];

    assert_eq!(files, ["file_a_new", "file_b_new", "file_c_new"]);

    Ok(())
}

fn clear_files() -> Result<()> {
    let path: &std::path::Path = "./temp/local_local_sync".as_ref();
    if path.exists() {
        std::fs::remove_dir_all("./temp/local_local_sync")?;
    }
    Ok(())
}

fn setup_files() -> Result<()> {
    std::fs::create_dir_all("./temp/local_local_sync/local_a")?;
    std::fs::create_dir_all("./temp/local_local_sync/local_b")?;

    // local_b has an outdated file_b
    std::fs::write("./temp/local_local_sync/local_b/file_b.txt", "file_b_old")?;

    // local_a has up-to-date everything except file_a
    std::fs::write("./temp/local_local_sync/local_a/file_a.txt", "file_a_old")?;
    std::fs::write("./temp/local_local_sync/local_a/file_b.txt", "file_b_new")?;
    std::fs::write("./temp/local_local_sync/local_a/file_c.txt", "file_c_new")?;

    // local_b already has the updated file_a
    std::fs::write("./temp/local_local_sync/local_b/file_a.txt", "file_a_new")?;

    Ok(())
}
