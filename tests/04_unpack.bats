#!/usr/bin/env bats

setup() {
    # Create a unique temporary directory for each test
    TEST_DIR=$(mktemp -d)
    echo "Files created in: $TEST_DIR" >&3
    
    # Define source and destination paths
    INPUT_DIR="$TEST_DIR/input_data"
    OUTPUT_SQFS="$TEST_DIR/output.sqfs"
    MOUNT_POINT="$TEST_DIR/mnt"
    
    # Pre-test artifacts
    ARGS_FILE="$TEST_DIR/create_args"
    
    # Populate input directory with some dummy data
    mkdir -p "$INPUT_DIR/subdir"
    echo "Hello World" > "$INPUT_DIR/file1.txt"
    echo "Subdir Content" > "$INPUT_DIR/subdir/file2.txt"
}

teardown() {
    # Run helper script to unmount if needed (best effort)
    # Using run with command to avoid failing teardown
    run $ZKS_SQM_BIN umount "$MOUNT_POINT"
    
    # Remove test directory
    rm -rf "$TEST_DIR"
}

@test "Repack: .tar (plain) to .sqfs" {
    # Create input tar
    tar -cf "$TEST_DIR/input.tar" -C "$TEST_DIR" input_data
    
    # Tests that create accepts a file (not just dir) and produces valid sqfs
    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    [ -f "$OUTPUT_SQFS" ]
    
    # Verification via unsquashfs listing
    run unsquashfs -l "$OUTPUT_SQFS"
    [ "$status" -eq 0 ]
    [[ "$output" == *"input_data/file1.txt"* ]]
}

@test "Repack: .tar.gz (gzip) to .sqfs" {
    tar -czf "$TEST_DIR/input.tar.gz" -C "$TEST_DIR" input_data
    
    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar.gz" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    [ -f "$OUTPUT_SQFS" ]
    
    run unsquashfs -l "$OUTPUT_SQFS"
    [[ "$output" == *"input_data/subdir/file2.txt"* ]]
}

@test "Repack: .tar.zst (zstd) to .sqfs" {
    # Requires tar with zstd support
    if ! tar --help | grep -q zstd; then
        skip "tar does not support --zstd"
    fi
    
    tar --zstd -cf "$TEST_DIR/input.tar.zst" -C "$TEST_DIR" input_data
    
    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar.zst" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    
    run unsquashfs -l "$OUTPUT_SQFS"
    [[ "$output" == *"input_data/file1.txt"* ]]
}

@test "Repack: .tar.xz (xz) to .sqfs" {
    tar -cJf "$TEST_DIR/input.tar.xz" -C "$TEST_DIR" input_data
    
    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar.xz" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    
    run unsquashfs -l "$OUTPUT_SQFS"
    [[ "$output" == *"input_data/file1.txt"* ]]
}

@test "Repack: .tar.zip (InfoZip) to .sqfs" {
    # Requires zip
    if ! command -v zip &> /dev/null; then
        skip "zip not found"
    fi
    
    # Create inner tar first
    tar -cf "$TEST_DIR/input.tar" -C "$TEST_DIR" input_data
    cd "$TEST_DIR" && zip input.tar.zip input.tar
    
    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar.zip" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    
    run unsquashfs -l "$OUTPUT_SQFS"
    [[ "$output" == *"input_data/file1.txt"* ]]
}

@test "Repack: .tar.7z (7zip) to .sqfs" {
    # Requires 7z
    if ! command -v 7z &> /dev/null; then
        skip "7z not found"
    fi
    
    tar -cf "$TEST_DIR/input.tar" -C "$TEST_DIR" input_data
    cd "$TEST_DIR" && 7z a input.tar.7z input.tar
    
    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar.7z" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    
    run unsquashfs -l "$OUTPUT_SQFS"
    [[ "$output" == *"input_data/file1.txt"* ]]
}

@test "Repack: .tar.rar (RAR) to .sqfs" {
    # Requires rar (for creating) and unrar (for testing)
    # Check for rar or skip, since we need to CREATE the test file
    if ! command -v rar &> /dev/null; then
        skip "rar not found (needed to create test file)"
    fi
    if ! command -v unrar &> /dev/null; then
        skip "unrar not found (needed for extraction logic)"
    fi

    tar -cf "$TEST_DIR/input.tar" -C "$TEST_DIR" input_data
    cd "$TEST_DIR" && rar a input.tar.rar input.tar

    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar.rar" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]

    run unsquashfs -l "$OUTPUT_SQFS"
    [[ "$output" == *"input_data/file1.txt"* ]]
}
