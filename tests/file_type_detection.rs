use zero_kelvin::utils;
use std::fs;

#[test]
fn test_reproduce_misleading_extension() {
    // This test verifies that we use magic numbers (via infer) instead of extensions.
    
    let temp_dir = tempfile::tempdir().unwrap();
    
    // 1. Create a "fake" tarball (text file named .tar)
    let fake_tar = temp_dir.path().join("fake.tar");
    fs::write(&fake_tar, "This is just text, not a tarball").unwrap();
    
    // 2. Create a "fake" text file (tarball named .txt)
    let fake_txt = temp_dir.path().join("real_tar.txt");
    let tar_gz_path = temp_dir.path().join("test.tar.gz");
    
    // Create a valid small archive
    let status = std::process::Command::new("tar")
        .args(&["-czf", tar_gz_path.to_str().unwrap(), "--files-from", "/dev/null"])
        .status()
        .expect("failed to execute tar");
    assert!(status.success());
        
    // Rename it to .txt
    fs::copy(&tar_gz_path, &fake_txt).unwrap();
    
    // 1. Check the fake tar
    match utils::get_file_type(&fake_tar) {
        Ok(t) => match t {
             utils::ArchiveType::Tar => panic!("Should not identify text file as Tar"),
             _ => {}, // Correct (Unknown or other)
        },
        Err(e) => panic!("Failed to detect type of fake tar: {}", e),
    }
    
    // 2. Check the real archive with .txt extension
    match utils::get_file_type(&fake_txt) {
        Ok(t) => match t {
             utils::ArchiveType::Gzip => {}, // Correct
             _ => panic!("Should identify tar.gz (as gzip) even with .txt extension"),
        },
        Err(e) => panic!("Failed to detect type of real archive: {}", e),
    }
}
