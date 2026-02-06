#!/usr/bin/env bats
load 'test_helper/bats-support/load'
load 'test_helper/bats-assert/load'

setup() {
    # Skip if root is not available/enabled
    if [ "$SKIP_ROOT" = "1" ]; then
        skip "Root tests are disabled (or not running as root/sudo)"
    fi

    # Setup binaries and paths
    if [ -z "$ZKS_BIN" ]; then
        SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")" && pwd)"
        ZKS_PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
        ZKS_SQM_BIN="$ZKS_PROJECT_ROOT/target/debug/squash_manager-rs"
        ZKS_BIN="$ZKS_PROJECT_ROOT/target/debug/zks-rs"
        export ZKS_SQM_BIN ZKS_BIN ZKS_PROJECT_ROOT
    fi

    # ROOT_CMD setup
    if [ -z "${ROOT_CMD+x}" ]; then
        if [ "$(id -u)" -eq 0 ]; then
            ROOT_CMD=""
        else
            ROOT_CMD="sudo"
        fi
        export ROOT_CMD
    fi

    # PATH setup for helper binaries
    BIN_DIR=$(dirname "$ZKS_BIN")
    export PATH="$BIN_DIR:$PATH"

    export TEST_DIR=$(mktemp -d -t zks-luks-freeze.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "content" > "$SRC/file.txt"
    mkdir -p "$SRC/subdir"
    echo "subcontent" > "$SRC/subdir/file2.txt"

    # Mount point for verification
    export MOUNT_POINT="$TEST_DIR/mnt_verify"
}

teardown() {
    [ "$SKIP_ROOT" = "1" ] && return

    # Clean up verification mount if it exists
    if [ -d "$MOUNT_POINT" ]; then
         ${ROOT_CMD:-} "$ZKS_SQM_BIN" umount "$MOUNT_POINT" 2>/dev/null || true
    fi

    # Close ALL sq_* LUKS mappers
    for mapper_path in /dev/mapper/sq_*; do
        if [ -e "$mapper_path" ]; then
            mapper_name=$(basename "$mapper_path")
            ${ROOT_CMD:-} cryptsetup close "$mapper_name" 2>/dev/null || true
        fi
    done

    ${ROOT_CMD:-} losetup -D 2>/dev/null || true
    rm -rf "$TEST_DIR"
}

@test "LUKS Freeze: Fail if existing file (no overwrite)" {
    OUT="$TEST_DIR/archive.sqfs"
    touch "$OUT"

    # Expect failure when file exists
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$OUT\""
    assert_failure
    assert_output --partial "exists"
}

@test "LUKS Freeze: Auto-generate name if output is directory" {
    # Directory exists
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$TEST_DIR\""
    assert_success

    # Check that a .sqfs file was created inside TEST_DIR
    run find "$TEST_DIR" -maxdepth 1 -name "*.sqfs_luks.img"
    assert_line --index 0 --partial ".sqfs_luks.img"

    # 1. Find ANY file created in that dir (ignoring extension)
    # We expect exactly one file to be created
    run find "$TEST_DIR" -maxdepth 1 -type f
    assert_line --index 0 --partial "$TEST_DIR"
    local created_file="${lines[0]}"

    # 2. Verify it is a LUKS container using 'file' utility
    run file "$created_file"
    assert_output --partial "LUKS encrypted file"
}

@test "LUKS Freeze: Using -r (read from file)" {
    LIST="$TEST_DIR/list.txt"
    OUT="$TEST_DIR/list_archive.sqfs"

    # Only freeze file.txt, ignoring subdir
    echo "$SRC/file.txt" > "$LIST"

    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$OUT\" -r \"$LIST\""
    assert_success
    [ -f "$OUT" ]

    # Verify content via mount
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" mount \"$OUT\" \"$MOUNT_POINT\""
    assert_success

    # Check file exists and subdir does not
    [ -f "$MOUNT_POINT/to_restore/1/file.txt" ] || [ -f "$MOUNT_POINT/to_restore/file.txt" ] # Adapt based on actual structure
    # Checking if subdir file is absent requires knowing the structure.
    # Usually zks flattens or preserves based on input.
    # Let's search inside mountpoint.
    run find "$MOUNT_POINT" -name "file.txt"
    assert_output --partial "file.txt"

    run find "$MOUNT_POINT" -name "file2.txt"
    refute_output --partial "file2.txt"
}

@test "LUKS Freeze: Metadata & Manifest Verification" {
    OUT="$TEST_DIR/meta.sqfs"
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$OUT\""
    assert_success

    # Mount to verify list.yaml
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" mount \"$OUT\" \"$MOUNT_POINT\""
    assert_success

    [ -f "$MOUNT_POINT/list.yaml" ]

    run cat "$MOUNT_POINT/list.yaml"
    assert_output --partial "host:"
    assert_output --partial "date:"
    # Since we are running as root/sudo for LUKS
    assert_output --partial "privilege_mode: root"
    assert_output --partial "name: src"
}

@test "LUKS Freeze: Advanced Content (Symlinks, Empty Dirs, Special Chars)" {
    ADV_SRC="$TEST_DIR/advanced"
    mkdir -p "$ADV_SRC"

    # Symlinks
    ln -s "target_file" "$ADV_SRC/rel_link"
    touch "$ADV_SRC/target_file"

    # Empty Dir
    mkdir "$ADV_SRC/empty_dir"

    # Special Chars
    echo "space" > "$ADV_SRC/file with spaces.txt"
    echo "emoji" > "$ADV_SRC/emoji_😀.txt"

    OUT="$TEST_DIR/advanced.sqfs"
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$ADV_SRC\" \"$OUT\""
    assert_success

    # Verify Content via Mount
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" mount \"$OUT\" \"$MOUNT_POINT\""
    assert_success

    # Check existence
    # Note: Structure is usually list.yaml and to_restore/
    # We grep recursively to find files
    run find "$MOUNT_POINT"
    assert_output --partial "rel_link"
    assert_output --partial "empty_dir"
    assert_output --partial "file with spaces.txt"
    assert_output --partial "emoji_😀.txt"
}
