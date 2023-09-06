# filesync

Simple Rust library to sync files between different sources.

Currently supports:

1. Local files
2. S3 (`s3` feature)

## Usage

```rust
use filesync::{
    local::LocalFiles,
    s3::S3Files,
};

let config = aws_config::load_from_env().await;
let client = aws_sdk_s3::Client::new(&config);

let mut local = LocalFiles::new("./my_local_files", true);
let mut s3 = S3Files::new(client, "my_s3_bucket", "path/in/bucket", true);

let synced_paths = filesync::sync_one_way(&mut local, &mut s3).await?;
assert_eq!(synced_paths, vec![PathBuf::from("my_changed_file.txt")]);
```
