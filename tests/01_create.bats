#!/usr/bin/env bats
load 'test_helper/bats-support/load'
load 'test_helper/bats-assert/load'

setup() {
    export TEST_DIR=$(mktemp -d -t zks-create.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "data" > "$SRC/file.txt"
    mkdir -p "$SRC/dir"
    echo $RANDOM > "$SRC/dir/file2.txt"
    echo $RANDOM > "$SRC/dir/file3.txt"
    mkdir -p "$SRC/dir/dir2"
    echo $RANDOM > "$SRC/dir/dir2/file4.txt"
    echo $RANDOM > "$SRC/dir/dir2/file5.txt"
}

teardown() {
    rm -rf "$TEST_DIR"
}

@test "Smoke: Successfully create a plain archive" {
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/out.sqfs" --no-progress
    assert_success
    [ -f "$TEST_DIR/out.sqfs" ]
}

@test "Logic: Error if output file exists" {
    touch "$TEST_DIR/existing.sqfs"
    # Default behavior (without --overwrite-*) should fail
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/existing.sqfs" --no-progress
    assert_failure
    assert_output --partial "exists"
}

@test "Logic: Verify file type using 'file'" {
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/type_check.sqfs" --no-progress
    assert_success
    run file "$TEST_DIR/type_check.sqfs"
    assert_output --partial "Squashfs filesystem"
    assert_output --partial "zstd compressed"
}

@test "Logic: Error when input directory is missing" {
    run $ZKS_SQM_BIN create "/bad/path" "$TEST_DIR/out.sqfs"
    echo "DEBUG output: [$output]" >&3
    assert_failure
    assert_output --partial "Invalid path"
}

@test "Logic: Compression level flag is accepted" {
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/comp.sqfs" -c 1 --no-progress
    assert_success
    run bash -c "unsquashfs -s $TEST_DIR/comp.sqfs | grep compression-level"
    echo "DEBUG output: [$output]" >&3
    assert_output --partial "compression-level 1"
    [ -f "$TEST_DIR/comp.sqfs" ]
}

@test "Logic: Default compression level" {
    # This test is disabled in release mode as it might break
    # if the default compression level changes in the external mksquashfs utility

    if [ "$ZKS_RELEASE" = "true" ]; then
        skip "Default compression test is disabled in release mode"
    fi
    # Extract value from Rust source code (reliable parsing of number after =)
    local default_comp=$(sed -n 's/.*DEFAULT_ZSTD_COMPRESSION.*= *\([0-9]\+\).*/\1/p' "$ZKS_PROJECT_ROOT/src/constants.rs")
    [ "$default_comp" -eq 15 ] && skip "Default compression is 15"
    
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/default.sqfs" --no-progress
    assert_success
    run bash -c "unsquashfs -s $TEST_DIR/default.sqfs | grep compression-level"
    echo "DEBUG output: [$output]" >&3
    assert_output --partial "compression-level $default_comp"
}

@test "Smoke: Create archive without compression (-c 0)" {
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/nocomp.sqfs" -c 0 --no-progress
    assert_success
    [ -f "$TEST_DIR/nocomp.sqfs" ]
    
    # Verify the file is actually created and readable
    run file "$TEST_DIR/nocomp.sqfs"
    assert_output --partial "Squashfs filesystem"
}

@test "Logic: Efficacy of disabling compression (-c 0 vs -c 1)" {
    # 1. Generate highly compressible data (text pattern)
    # 10 files ~85KB each = ~850KB of data
    mkdir -p "$SRC/compressible"
    for i in {1..10}; do
        yes "ZeroKelvinStazis_Test_String" | head -n 5000 > "$SRC/compressible/file_$i.txt"
    done

    # 2. Create compressed archive (using fast compression -c 1)
    run $ZKS_SQM_BIN create "$SRC/compressible" "$TEST_DIR/compressed.sqfs" -c 1 --no-progress
    assert_success

    # 3. Create UNcompressed archive (-c 0)
    run $ZKS_SQM_BIN create "$SRC/compressible" "$TEST_DIR/nocomp.sqfs" -c 0 --no-progress
    assert_success

    # 4. Compare sizes
    local size_comp=$(stat -c%s "$TEST_DIR/compressed.sqfs")
    local size_nocomp=$(stat -c%s "$TEST_DIR/nocomp.sqfs")

    echo "Size Compressed (-c 1): $size_comp" >&3
    echo "Size Uncompressed (-c 0): $size_nocomp" >&3

    # Uncompressed should be significantly larger
    # (For compressible data, the difference will be multiple times)
    [ "$size_nocomp" -gt "$size_comp" ]

    # 5. Check metadata (optional, depends on mksquashfs version)
    # Typically, without compression, unsquashfs reports "gzip" (as metadata default)
    # or "no compression". The main thing is the size check above.
}

@test "Cleanup: Interrupted create removes incomplete file (Directory)" {
    # Create random data file (slow to compress, unlike sparse files)
    BIGDIR="$TEST_DIR/bigdir"
    mkdir -p "$BIGDIR"
    dd if=/dev/urandom of="$BIGDIR/random.bin" bs=1M count=50 2>/dev/null
    
    OUTPUT="$TEST_DIR/interrupted.sqfs"
    
    # Start create in background
    $ZKS_SQM_BIN create "$BIGDIR" "$OUTPUT" --no-progress &
    CREATE_PID=$!
    
    # Wait for file to appear (max 10 sec)
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