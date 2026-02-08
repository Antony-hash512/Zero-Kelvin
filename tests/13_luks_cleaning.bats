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

    export TEST_DIR=$(mktemp -d -t zks-luks-clean.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "content" > "$SRC/file.txt"

    # Override XDG_CACHE_HOME to isolate tests
    export XDG_CACHE_HOME="$TEST_DIR/cache"
    mkdir -p "$XDG_CACHE_HOME"

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

@test "LUKS Cleanup: Staging area is removed after successful freeze" {
    OUT="$TEST_DIR/archive.sqfs"
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$OUT\""
    assert_success

    # Check that stazis cache dir does NOT contain build_* directories
    CACHE_ROOT="$XDG_CACHE_HOME/zero-kelvin-stazis"

    if [ -d "$CACHE_ROOT" ]; then
        # Count dirs starting with build_
        run find "$CACHE_ROOT" -maxdepth 1 -type d -name "build_*"
        assert_output ""
    fi
}

@test "LUKS Cleanup: freeze.sh is NOT included in the archive" {
    OUT="$TEST_DIR/clean_structure.sqfs"
    run bash -c "printf 'testpass\ntestpass\n' | ${ROOT_CMD:-} \"$ZKS_BIN\" freeze -e \"$SRC\" \"$OUT\""
    assert_success

    # Mount to verify
    run bash -c "printf 'testpass\n' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" mount \"$OUT\" \"$MOUNT_POINT\""
    assert_success

    run find "$MOUNT_POINT"
    refute_output --partial "freeze.sh"
    assert_output --partial "list.yaml"
}
