#!/usr/bin/env bats

setup() {
    bats_require_minimum_version 1.5.0

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
    if ! command -v tar2sqfs &> /dev/null; then
        skip "tar2sqfs (optional dependency) not found"
    fi

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
    if ! command -v tar2sqfs &> /dev/null; then
        skip "tar2sqfs (optional dependency) not found"
    fi

    tar -czf "$TEST_DIR/input.tar.gz" -C "$TEST_DIR" input_data
    
    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar.gz" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    [ -f "$OUTPUT_SQFS" ]
    
    run unsquashfs -l "$OUTPUT_SQFS"
    [[ "$output" == *"input_data/subdir/file2.txt"* ]]
}

@test "Repack: .tar.zst (zstd) to .sqfs" {
    if ! command -v tar2sqfs &> /dev/null; then
        skip "tar2sqfs (optional dependency) not found"
    fi
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
    if ! command -v tar2sqfs &> /dev/null; then
        skip "tar2sqfs (optional dependency) not found"
    fi

    tar -cJf "$TEST_DIR/input.tar.xz" -C "$TEST_DIR" input_data
    
    run $ZKS_SQM_BIN create "$TEST_DIR/input.tar.xz" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    
    run unsquashfs -l "$OUTPUT_SQFS"
    [[ "$output" == *"input_data/file1.txt"* ]]
}

@test "Repack: .tar.zip (InfoZip) to .sqfs" {
    if ! command -v tar2sqfs &> /dev/null; then
        skip "tar2sqfs (optional dependency) not found"
    fi
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
    if ! command -v tar2sqfs &> /dev/null; then
        skip "tar2sqfs (optional dependency) not found"
    fi
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
    if ! command -v tar2sqfs &> /dev/null; then
        skip "tar2sqfs (optional dependency) not found"
    fi
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

@test "Negative: Input path does not exist" {
    run $ZKS_SQM_BIN create "$TEST_DIR/non_existent.tar" "$OUTPUT_SQFS"
    [ "$status" -ne 0 ]
    [[ "$output" == *"Invalid path"* ]]
}

@test "Negative: Unsupported archive format" {
    touch "$TEST_DIR/bad.xyz"
    run $ZKS_SQM_BIN create "$TEST_DIR/bad.xyz" "$OUTPUT_SQFS"
    [ "$status" -ne 0 ]
    [[ "$output" == *"Unsupported or unknown archive format"* ]]
}

@test "Negative: Missing optional dependency tar2sqfs returns exit code 127" {
    tar -cf "$TEST_DIR/input.tar" -C "$TEST_DIR" input_data

    # Create a restricted PATH without tar2sqfs
    local restricted_bin="$TEST_DIR/restricted_bin"
    mkdir -p "$restricted_bin"
    ln -s "$(command -v sh)" "$restricted_bin/sh"
    ln -s "$(command -v cat)" "$restricted_bin/cat"

    run -127 env PATH="$restricted_bin" "$ZKS_SQM_BIN" create "$TEST_DIR/input.tar" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 127 ]
    [[ "$output" == *"Optional dependency 'tar2sqfs' is not installed"* ]]
    [[ "$output" == *"squashfs-tools-ng"* ]]
    [ ! -f "$OUTPUT_SQFS" ]
}

@test "Negative: Missing optional decompressor returns exit code 127" {
    tar -czf "$TEST_DIR/input.tar.gz" -C "$TEST_DIR" input_data

    # Provide a dummy executable tar2sqfs in restricted PATH, but omit gzip
    local restricted_bin="$TEST_DIR/restricted_bin"
    mkdir -p "$restricted_bin"
    printf '#!/bin/sh\nexit 0\n' > "$restricted_bin/tar2sqfs"
    chmod +x "$restricted_bin/tar2sqfs"

    run -127 env PATH="$restricted_bin" "$ZKS_SQM_BIN" create "$TEST_DIR/input.tar.gz" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 127 ]
    [[ "$output" == *"Optional dependency 'gzip' is not installed"* ]]
    [ ! -f "$OUTPUT_SQFS" ]
}

@test "Directory create succeeds when tar2sqfs is absent from PATH" {
    local restricted_bin="$TEST_DIR/restricted_bin"
    mkdir -p "$restricted_bin"
    ln -s "$(command -v mksquashfs)" "$restricted_bin/mksquashfs"

    run env PATH="$restricted_bin" "$ZKS_SQM_BIN" create "$INPUT_DIR" "$OUTPUT_SQFS" --no-progress
    [ "$status" -eq 0 ]
    [ -f "$OUTPUT_SQFS" ]
}

@test "Cleanup: Interrupted archive repack removes incomplete file" {
    if ! command -v tar2sqfs &> /dev/null; then
        skip "tar2sqfs (optional dependency) not found"
    fi

    # Create random data file and tar it (slow to repack, unlike sparse files)
    dd if=/dev/urandom of="$TEST_DIR/random.bin" bs=1M count=50 2>/dev/null
    tar -cf "$TEST_DIR/big.tar" -C "$TEST_DIR" random.bin
    
    OUTPUT="$TEST_DIR/interrupted.sqfs"
    
    # Start create in background
    $ZKS_SQM_BIN create "$TEST_DIR/big.tar" "$OUTPUT" --no-progress &
    CREATE_PID=$!
    
    # Wait for output file to appear (max 10 sec)
    for i in {1..100}; do
        [ -f "$OUTPUT" ] && break
        sleep 0.1
    done
    
    # Ensure file was created before we interrupt
    [ -f "$OUTPUT" ] || skip "Output file never appeared (system too slow?)"
    
    # Interrupt the process
    kill -INT $CREATE_PID 2>/dev/null || true
    wait $CREATE_PID 2>/dev/null || true
    
    # Assert: file should NOT exist after cleanup
    [ ! -f "$OUTPUT" ]
}
