#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    if [ "$SKIP_ROOT" = "1" ]; then
        skip "Root tests are disabled (or not running as root/sudo)"
    fi

    if [ -z "$ZKS_BIN" ]; then
        SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")" && pwd)"
        ZKS_PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
        ZKS_SQM_BIN="$ZKS_PROJECT_ROOT/target/debug/0k-core"
        ZKS_BIN="$ZKS_PROJECT_ROOT/target/debug/0k"
        export ZKS_SQM_BIN ZKS_BIN ZKS_PROJECT_ROOT
    fi

    if [ -z "${ROOT_CMD+x}" ]; then
        if [ "$(id -u)" -eq 0 ]; then
            ROOT_CMD=""
        else
            ROOT_CMD="sudo"
        fi
        export ROOT_CMD
    fi

    # Unique temp directory for isolation
    export TEMP_DIR=$(mktemp -d -t zks-luks-full.XXXXXX)
    export DATA_DIR="$TEMP_DIR/data"
    export ARCHIVE_PATH="$TEMP_DIR/archive.sqfs"

    # Create test data
    mkdir -p "$DATA_DIR/subdir"
    echo "content_root" > "$DATA_DIR/root.txt"
    echo "content_sub" > "$DATA_DIR/subdir/sub.txt"

    # Calculate checksums of original
    export SUM_ROOT=$(sha256sum "$DATA_DIR/root.txt" | awk '{print $1}')
    export SUM_SUB=$(sha256sum "$DATA_DIR/subdir/sub.txt" | awk '{print $1}')
}

teardown() {
    [ "$SKIP_ROOT" = "1" ] && return

    for mapper_path in /dev/mapper/sq_*; do
        if [ -e "$mapper_path" ]; then
            mapper_name=$(basename "$mapper_path")
            ${ROOT_CMD:-} cryptsetup close "$mapper_name" 2>/dev/null || true
        fi
    done
    ${ROOT_CMD:-} losetup -D 2>/dev/null || true
    rm -rf "$TEMP_DIR"
}

@test "LUKS Full Cycle: Freeze -> Check/Delete -> Unfreeze -> Verify" {
    # 1. Freeze
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$DATA_DIR\" \"$ARCHIVE_PATH\" --no-progress"
    assert_success
    assert_output --partial "Successfully created archive"
    [ -f "$ARCHIVE_PATH" ]

    # 2. Check & Force Delete
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" check \"$ARCHIVE_PATH\" --use-cmp --delete"
    assert_success
    assert_output --partial "DELETED"

    # Assert local files are GONE
    [ ! -f "$DATA_DIR/root.txt" ]
    [ ! -f "$DATA_DIR/subdir/sub.txt" ]

    # 3. Unfreeze
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" unfreeze \"$ARCHIVE_PATH\""
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
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" check \"$ARCHIVE_PATH\" --use-cmp"
    assert_success
    assert_output --partial "Files Matched: 2"
    assert_output --partial "Mismatched: 0"
    assert_output --partial "Missing: 0"
}
