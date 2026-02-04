#!/usr/bin/env bats
load 'test_helper/bats-support/load'
load 'test_helper/bats-assert/load'

setup() {
    export TEST_DIR=$(mktemp -d -t zks-freeze.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "content" > "$SRC/file.txt"
    mkdir -p "$SRC/subdir"
    echo "subcontent" > "$SRC/subdir/file2.txt"
    
    # Ensure ZKS_BIN is set; fallback to local debug build if not
    if [ -z "$ZKS_BIN" ]; then
        # Assume running from project root or tests dir
        if [ -f "./target/debug/zks-rs" ]; then
            export ZKS_BIN="./target/debug/zks-rs"
        elif [ -f "../target/debug/zks-rs" ]; then
            export ZKS_BIN="../target/debug/zks-rs"
        else
            # Try to locate
             export ZKS_BIN="$(git rev-parse --show-toplevel)/target/debug/zks-rs"
        fi
    fi
    
    # Add directory of ZKS_BIN to PATH so generated script can find squash_manager-rs
    BIN_DIR=$(dirname "$ZKS_BIN")
    export PATH="$BIN_DIR:$PATH"
}

teardown() {
    rm -rf "$TEST_DIR"
}

@test "Freeze: Fail if existing file (no overwrite)" {
    OUT="$TEST_DIR/archive.sqfs"
    # Create empty file to trigger overwrite error
    touch "$OUT"
    run $ZKS_BIN freeze "$SRC" "$OUT"
    assert_failure
    assert_output --partial "exists"
}

@test "Freeze: Auto-generate name if output is directory" {
    # Directory exists
    run $ZKS_BIN freeze "$SRC" "$TEST_DIR"
    assert_success
    # Check that a .sqfs file was created inside TEST_DIR
    run find "$TEST_DIR" -maxdepth 1 -name "*.sqfs"
    assert_line --index 0 --partial ".sqfs"
}

@test "Freeze: Using -r (read from file)" {
    LIST="$TEST_DIR/list.txt"
    OUT="$TEST_DIR/list_archive.sqfs"
    
    # Only freeze file.txt, ignoring subdir
    echo "$SRC/file.txt" > "$LIST"
    
    run $ZKS_BIN freeze "$OUT" -r "$LIST"
    assert_success
    [ -f "$OUT" ]
    
    # We can verify content by mounting or unsquashfs -l
    # unsquashfs might not be installed, but assuming env has tools
    if command -v unsquashfs >/dev/null; then
        run unsquashfs -l "$OUT"
        assert_output --partial "file.txt"
        refute_output --partial "file2.txt"
    fi
}

@test "Freeze: Fail if input missing" {
    run $ZKS_BIN freeze "$TEST_DIR/missing" "$TEST_DIR/out.sqfs"
    assert_failure
    # Expect error from prepare_staging or main
    # "Failed to get metadata" or "No targets"
    assert_output --partial "Failed"
}
