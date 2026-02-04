#!/usr/bin/env bats
load 'test_helper/bats-support/load'
load 'test_helper/bats-assert/load'

setup() {
    export TEST_DIR=$(mktemp -d -t zks-unfreeze.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "content" > "$SRC/file.txt"
    mkdir -p "$SRC/dir"
    echo "content2" > "$SRC/dir/file2.txt"
    
    if [ -z "$ZKS_BIN" ]; then
        if [ -f "./target/debug/zks-rs" ]; then
            export ZKS_BIN="./target/debug/zks-rs"
        elif [ -f "../target/debug/zks-rs" ]; then
            export ZKS_BIN="../target/debug/zks-rs"
        else
            echo "ZKS_BIN not set and not found"
            exit 1
        fi
    fi
    
    # We also need squash_manager-rs in PATH for zks-rs to work
    # Build dir usually adds to path? Or assume installed?
    # Tests run via Justfile usually setup PATH.
    # If running manually, we might fail if squash_manager-rs is not found.
    export PATH="$(dirname "$ZKS_BIN"):$PATH"
}

teardown() {
    chmod -R u+w "$TEST_DIR"
    rm -rf "$TEST_DIR"
}

@test "Unfreeze: Restore files successfully" {
    # 1. Freeze
    run $ZKS_BIN freeze "$SRC" "$TEST_DIR/archive.sqfs" --no-progress
    assert_success
    
    # 2. Remove source to simulate loss
    rm -rf "$SRC"
    
    run unsquashfs -l "$TEST_DIR/archive.sqfs"
    echo "DEBUG ARCHIVE CONTENT: $output" >&3
    
    # 3. Unfreeze
    run $ZKS_BIN unfreeze "$TEST_DIR/archive.sqfs"
    echo "DEBUG: $output" >&3
    assert_success
    
    # 4. Verify
    [ -f "$SRC/file.txt" ]
    [ -d "$SRC/dir" ]
    [ -f "$SRC/dir/file2.txt" ]
    run cat "$SRC/file.txt"
    assert_output "content"
}

@test "Unfreeze: Conflict detection (fail by default)" {
    # 1. Freeze
    run $ZKS_BIN freeze "$SRC" "$TEST_DIR/archive.sqfs" --no-progress
    assert_success
    
    # 2. Modify source file
    echo "conflict" > "$SRC/file.txt"
    
    # 3. Unfreeze (should fail)
    run $ZKS_BIN unfreeze "$TEST_DIR/archive.sqfs"
    assert_failure
    assert_output --partial "File exists"
    
    # Verify content unchanged
    run cat "$SRC/file.txt"
    assert_output "conflict"
}

@test "Unfreeze: Overwrite with --overwrite" {
    # 1. Freeze
    run $ZKS_BIN freeze "$SRC" "$TEST_DIR/archive.sqfs" --no-progress
    assert_success
    
    # 2. Modify source file
    echo "conflict" > "$SRC/file.txt"
    
    # 3. Unfreeze --overwrite
    run $ZKS_BIN unfreeze "$TEST_DIR/archive.sqfs" --overwrite
    assert_success
    
    # Verify content restored
    run cat "$SRC/file.txt"
    assert_output "content"
}

@test "Unfreeze: Skip existing with --skip-existing" {
    # 1. Freeze
    run $ZKS_BIN freeze "$SRC" "$TEST_DIR/archive.sqfs" --no-progress
    assert_success
    
    # 2. Modify source file
    echo "conflict" > "$SRC/file.txt"
    rm "$SRC/dir/file2.txt" # This one is missing, should be restored
    
    # 3. Unfreeze --skip-existing
    run $ZKS_BIN unfreeze "$TEST_DIR/archive.sqfs" --skip-existing
    assert_success
    
    # Verify conflict skipped (kept conflict)
    run cat "$SRC/file.txt"
    assert_output "conflict"
    
    # Verify missing file restored
    [ -f "$SRC/dir/file2.txt" ]
}
