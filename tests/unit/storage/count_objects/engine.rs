//! Unit tests for `src/storage/count_objects/engine.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::*;

#[test]
fn test_format_human_readable_size() {
    assert_eq!(format_human_readable_size(0), "0 bytes");
    assert_eq!(format_human_readable_size(31), "31 bytes");
    assert_eq!(format_human_readable_size(1024), "1024 bytes");
    assert_eq!(format_human_readable_size(1025), "1.00 KiB");
    assert_eq!(format_human_readable_size(1366), "1.33 KiB");
    assert_eq!(format_human_readable_size(1048576), "1024.00 KiB");
    assert_eq!(format_human_readable_size(1048577), "1.00 MiB");
    assert_eq!(format_human_readable_size(1073741824), "1024.00 MiB");
    assert_eq!(format_human_readable_size(1073741825), "1.00 GiB");
}

#[test]
fn test_read_pack_idx_v2() {
    // Construct a minimal valid pack index v2 header with 1 object
    let mut data = Vec::new();
    // Magic
    data.extend_from_slice(b"\xfftOc");
    // Version 2
    data.extend_from_slice(&2u32.to_be_bytes());
    // 256 fanout entries (4 bytes each)
    for _ in 0..255 {
        data.extend_from_slice(&0u32.to_be_bytes());
    }
    // Last fanout entry: 1 object
    data.extend_from_slice(&1u32.to_be_bytes());
    // 1 SHA-1 hash (20 bytes of 0xab)
    data.extend_from_slice(&[0xab; 20]);

    let temp_dir = std::env::temp_dir().join(format!("rgit_test_idx_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);
    let idx_path = temp_dir.join("test.idx");
    fs::write(&idx_path, &data).unwrap();

    let (num_objects, hashes) = read_pack_idx(&idx_path).unwrap();
    assert_eq!(num_objects, 1);
    assert_eq!(hashes.len(), 1);
    assert_eq!(hashes[0], "abababababababababababababababababababab");

    let _ = fs::remove_dir_all(&temp_dir);
}
