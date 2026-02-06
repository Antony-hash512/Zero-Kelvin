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
        # Assume running from project root
        if [ -f "./target/debug/zks-rs" ]; then
            export ZKS_BIN="$(pwd)/target/debug/zks-rs"
        elif [ -f "../target/debug/zks-rs" ]; then
            export ZKS_BIN="$(readlink -f ../target/debug/zks-rs)"
        else
             # Try to locate
             ROOT_DIR="$(git rev-parse --show-toplevel)"
             export ZKS_BIN="$ROOT_DIR/target/debug/zks-rs"
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
    # проверяем и тип файла и расширение т.к.
    # имя файла вместе с раширением автогенерируется
    # прогой, а не произвольно задаётся пользователем
    run find "$TEST_DIR" -maxdepth 1 -name "*.sqfs"
    assert_line --index 0 --partial ".sqfs"

    # 1. Find ANY file created in that dir
    run find "$TEST_DIR" -maxdepth 1 -type f
    assert_line --index 0 --partial "$TEST_DIR"
    local created_file="${lines[0]}"

    # 2. Verify it is a SquashFS archive using 'file' utility
    run file "$created_file"
    assert_output --partial "Squashfs filesystem"
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
    assert_output --partial "Error"
}

# --- Phase 6.5: Comprehensive Testing ---

@test "Freeze: Metadata & Manifest Verification" {
    if ! command -v unsquashfs >/dev/null; then
        skip "unsquashfs not found"
    fi

    OUT="$TEST_DIR/meta.sqfs"
    run $ZKS_BIN freeze "$SRC" "$OUT"
    assert_success

    # Extract list.yaml content
    run unsquashfs -cat "$OUT" list.yaml
    assert_success

    # Verify Metadata
    assert_output --partial "host:"
    assert_output --partial "date:"
    if [ "$(id -u)" -eq 0 ]; then
        assert_output --partial "privilege_mode: root"
    else
        assert_output --partial "privilege_mode: user"
    fi

    # Verify File Entry
    assert_output --partial "name: src"
    # assert_output --partial "Filesystem" # Removed: unsquashfs -cat only outputs file content
    # Actually unsquashfs -cat prints file content to stdout.
    # The output variable will contain the file content.
}

@test "Freeze: Advanced Content (Symlinks, Empty Dirs, Special Chars)" {
    if ! command -v unsquashfs >/dev/null; then
        skip "unsquashfs not found"
    fi

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
    run $ZKS_BIN freeze "$ADV_SRC" "$OUT"
    assert_success

    # Verify Content Listing
    run unsquashfs -l "$OUT"
    assert_success
    assert_output --partial "rel_link"
    assert_output --partial "empty_dir"
    assert_output --partial "file with spaces.txt"
    assert_output --partial "emoji_😀.txt"
}

@test "Freeze: Path Resolution (Relative, Dot, Multiple)" {
    OUT="$TEST_DIR/path_res.sqfs"

    # Relative Path input
    pushd "$TEST_DIR"
    run $ZKS_BIN freeze "src" "out_rel.sqfs"
    assert_success
    [ -f "out_rel.sqfs" ]
    popd

    # Dot Target
    pushd "$SRC"
    run $ZKS_BIN freeze "$PWD" "../dot_archive.sqfs"
    assert_success
    [ -f "../dot_archive.sqfs" ]
    popd

    # Multiple Targets
    mkdir -p "$TEST_DIR/t1" "$TEST_DIR/t2"
    touch "$TEST_DIR/t1/f1" "$TEST_DIR/t2/f2"

    run $ZKS_BIN freeze "$TEST_DIR/t1" "$TEST_DIR/t2" "$TEST_DIR/multi.sqfs"
    assert_success

    if command -v unsquashfs >/dev/null; then
        run unsquashfs -cat "$TEST_DIR/multi.sqfs" list.yaml
        assert_output --partial "name: t1"
        assert_output --partial "name: t2"
    fi
}
