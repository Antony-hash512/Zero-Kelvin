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

    export TEST_DIR=$(mktemp -d -t zks-luks-check.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC/dir"
    echo "content1" > "$SRC/file1.txt"
    echo "content2" > "$SRC/dir/file2.txt"

    export ARCHIVE="$TEST_DIR/archive.sqfs"

    # Pre-create a valid LUKS archive
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$ARCHIVE\" --no-progress"
    assert_success
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
    rm -rf "$TEST_DIR"
}

@test "LUKS Check: Detect matching files" {
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" check \"$ARCHIVE\""
    assert_success
    assert_output --partial "MATCH"
    assert_output --partial "Matched:"
    assert_output --partial "src/file1.txt"
    assert_output --partial "src/dir/file2.txt"
}

@test "LUKS Check: Detect modified content (Fast Check mismatch)" {
    # Change size to trigger metadata mismatch
    echo "longer content" > "$SRC/file1.txt"

    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" check \"$ARCHIVE\""
    assert_success # Command succeeds, but reports mismatch
    assert_output --partial "MISMATCH"
    assert_output --partial "file1.txt"
}

@test "LUKS Check: Detect missing files" {
    rm "$SRC/file1.txt"

    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" check \"$ARCHIVE\""
    assert_success
    assert_output --partial "MISSING"
    assert_output --partial "file1.txt"
}

@test "LUKS Check: Force delete with Safety Gate (Skip newer)" {
    # Make local file newer
    touch -d "next hour" "$SRC/file1.txt"

    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" check \"$ARCHIVE\" --force-delete"
    assert_success
    assert_output --partial "SKIPPED (Newer)"
    assert [ -f "$SRC/file1.txt" ]
}

@test "LUKS Check: Force delete (Success)" {
    # Ensure local file is older/same
    touch -d "last hour" "$SRC/file1.txt"

    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" check \"$ARCHIVE\" --force-delete"
    assert_success
    assert_output --partial "DELETED"
    assert [ ! -f "$SRC/file1.txt" ]
}
