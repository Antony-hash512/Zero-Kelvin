#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    # Unique temp directory for isolation
    export TEMP_DIR="$(mktemp -d)"
    export DATA_DIR="$TEMP_DIR/data"
    export RESTORE_DIR="$TEMP_DIR/restore_target"
    export ARCHIVE_PATH="$TEMP_DIR/archive.sqfs"
    
    # Ensure binary path
    if [ -z "$ZKS_BIN" ]; then
        if [ -f "./target/debug/zks-rs" ]; then
             export ZKS_BIN="./target/debug/zks-rs"
        elif [ -f "../target/debug/zks-rs" ]; then
             export ZKS_BIN="../target/debug/zks-rs"
        else
             # Fallback: assume in PATH or just fail later
             export ZKS_BIN="zks-rs"
        fi
    fi

    # Create test data
    mkdir -p "$DATA_DIR/subdir"
    echo "content_root" > "$DATA_DIR/root.txt"
    echo "content_sub" > "$DATA_DIR/subdir/sub.txt"
    
    # Calculate checksums of original
    export SUM_ROOT=$(sha256sum "$DATA_DIR/root.txt" | awk '{print $1}')
    export SUM_SUB=$(sha256sum "$DATA_DIR/subdir/sub.txt" | awk '{print $1}')
}

teardown() {
    rm -rf "$TEMP_DIR"
}

@test "Full Cycle: Freeze -> Check/Delete -> Unfreeze -> Verify" {
    # 1. Freeze
    run "$ZKS_BIN" freeze "$DATA_DIR" "$ARCHIVE_PATH" --no-progress
    assert_success
    assert_output --partial "Successfully created archive"
    [ -f "$ARCHIVE_PATH" ]

    # 2. Check & Force Delete
    # This verifies the archive matches AND deletes the local source
    run "$ZKS_BIN" check "$ARCHIVE_PATH" --use-cmp --force-delete
    assert_success
    assert_output --partial "DELETED"
    
    # Assert local files are GONE
    [ ! -f "$DATA_DIR/root.txt" ]
    [ ! -f "$DATA_DIR/subdir/sub.txt" ]
    # Parent directory might remain depending on implementation (zks usually deletes files/dirs it staged)
    # Check explicitly that files are gone.

    # 3. Unfreeze
    # Since we deleted the source, unfreeze should restore them to their original path ($DATA_DIR)
    # Note: freeze records absolute paths or resolves them. 
    # If we froze "$DATA_DIR" (e.g. /tmp/.../data), it should restore to /tmp/.../data
    run "$ZKS_BIN" unfreeze "$ARCHIVE_PATH"
    assert_success
    
    # 4. Verify Restoration
    [ -f "$DATA_DIR/root.txt" ]
    [ -f "$DATA_DIR/subdir/sub.txt" ]
    
    # Check Content
    NEW_SUM_ROOT=$(sha256sum "$DATA_DIR/root.txt" | awk '{print $1}')
    NEW_SUM_SUB=$(sha256sum "$DATA_DIR/subdir/sub.txt" | awk '{print $1}')
    
    [ "$SUM_ROOT" = "$NEW_SUM_ROOT" ]
    [ "$SUM_SUB" = "$NEW_SUM_SUB" ]
    
    # 5. Final Integrity Check
    run "$ZKS_BIN" check "$ARCHIVE_PATH" --use-cmp
    assert_success
    assert_output --partial "Files Matched: 2"
    assert_output --partial "Mismatched: 0"
    assert_output --partial "Missing: 0"
}
