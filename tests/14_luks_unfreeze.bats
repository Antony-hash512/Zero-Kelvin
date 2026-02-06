#!/usr/bin/env bats
load 'test_helper/bats-support/load'
load 'test_helper/bats-assert/load'

setup() {
    if [ "$SKIP_ROOT" = "1" ]; then
        skip "Root tests are disabled (or not running as root/sudo)"
    fi

    if [ -z "$ZKS_BIN" ]; then
        SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")" && pwd)"
        ZKS_PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
        ZKS_SQM_BIN="$ZKS_PROJECT_ROOT/target/debug/squash_manager-rs"
        ZKS_BIN="$ZKS_PROJECT_ROOT/target/debug/zks-rs"
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

    BIN_DIR=$(dirname "$ZKS_BIN")
    export PATH="$BIN_DIR:$PATH"

    export TEST_DIR=$(mktemp -d -t zks-luks-unfreeze.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "content" > "$SRC/file.txt"
    mkdir -p "$SRC/dir"
    echo "content2" > "$SRC/dir/file2.txt"

    export MOUNT_POINT="$TEST_DIR/mnt_verify"
}

teardown() {
    [ "$SKIP_ROOT" = "1" ] && return

    if [ -d "$MOUNT_POINT" ]; then
         ${ROOT_CMD:-} "$ZKS_SQM_BIN" umount "$MOUNT_POINT" 2>/dev/null || true
    fi

    for mapper_path in /dev/mapper/sq_*; do
        if [ -e "$mapper_path" ]; then
            mapper_name=$(basename "$mapper_path")
            ${ROOT_CMD:-} cryptsetup close "$mapper_name" 2>/dev/null || true
        fi
    done
    ${ROOT_CMD:-} losetup -D 2>/dev/null || true
    rm -rf "$TEST_DIR"
}

@test "LUKS Unfreeze: Restore files successfully" {
    ARCHIVE="$TEST_DIR/archive.sqfs"
    # 1. Freeze
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$ARCHIVE\" --no-progress"
    assert_success

    # 2. Remove source to simulate loss
    rm -rf "$SRC"

    # 3. Unfreeze (1 pass)
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" unfreeze \"$ARCHIVE\""
    assert_success

    # 4. Verify
    [ -f "$SRC/file.txt" ]
    [ -d "$SRC/dir" ]
    [ -f "$SRC/dir/file2.txt" ]

    # Need sudo to read if restored as root?
    # Freeze -e implies running as root, so files might be restored as root if zks was root.
    # But files in SRC were owned by user (usually).
    # If unfreeze restores ownership, it should work.
    # We use ROOT_CMD to cat just in case.
    run ${ROOT_CMD:-} cat "$SRC/file.txt"
    assert_output "content"
}

@test "LUKS Unfreeze: Conflict detection (fail by default)" {
    ARCHIVE="$TEST_DIR/archive.sqfs"
    # 1. Freeze
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$ARCHIVE\" --no-progress"
    assert_success

    # 2. Modify source file
    echo "conflict" > "$SRC/file.txt"

    # 3. Unfreeze (should fail)
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" unfreeze \"$ARCHIVE\""
    assert_failure
    assert_output --partial "File exists"

    # Verify content unchanged
    run cat "$SRC/file.txt"
    assert_output "conflict"
}

@test "LUKS Unfreeze: Overwrite with --overwrite" {
    ARCHIVE="$TEST_DIR/archive.sqfs"
    # 1. Freeze
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$ARCHIVE\" --no-progress"
    assert_success

    # 2. Modify source file
    echo "conflict" > "$SRC/file.txt"

    # 3. Unfreeze --overwrite
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" unfreeze \"$ARCHIVE\" --overwrite"
    assert_success

    # Verify content restored
    run ${ROOT_CMD:-} cat "$SRC/file.txt"
    assert_output "content"
}
