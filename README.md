Test structure:

sync    ./temp/*
to      s3://.../(commit-hash)/*

1. Sync to S3 (uploads all files)
2. Delete some local files
3. Sync S3 to local (verify only the subset is downloaded)
4. Modify some local files
5. Sync to S3 again (verify only the subset is uploaded)

File structure:

/
    one_no_changes.txt
    two_changes.txt
    folder/
        three_no_changes.txt
        four_changes.txt
