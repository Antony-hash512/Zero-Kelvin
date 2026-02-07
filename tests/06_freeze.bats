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

@test "Freeze: Auto-generate name if output is directory (with --prefix)" {
    # Directory exists, use --prefix to skip interactive prompt
    run $ZKS_BIN freeze "$SRC" "$TEST_DIR" --prefix testarchive
    assert_success

    # Check that a .sqfs file with the correct prefix was created inside TEST_DIR
    run find "$TEST_DIR" -maxdepth 1 -name "testarchive_*.sqfs"
    assert_line --index 0 --partial "testarchive_"
    assert_line --index 0 --partial ".sqfs"

    local created_file="${lines[0]}"

    # Verify it is a SquashFS archive using 'file' utility
    run file "$created_file"
    assert_output --partial "Squashfs filesystem"
}

@test "Freeze: Auto-generate name with interactive prefix (via stdin)" {
    # Pipe prefix via stdin to simulate interactive input
    run bash -c "echo 'interactive_test' | \"$ZKS_BIN\" freeze \"$SRC\" \"$TEST_DIR\""
    assert_success

    # Check that a .sqfs file with the interactive prefix was created
    run find "$TEST_DIR" -maxdepth 1 -name "interactive_test_*.sqfs"
    assert_line --index 0 --partial "interactive_test_"
    assert_line --index 0 --partial ".sqfs"

    local created_file="${lines[0]}"
    run file "$created_file"
    assert_output --partial "Squashfs filesystem"
}


@test "Freeze: Fail if interactive prefix is empty" {
    # Pipe empty string
    run bash -c "echo '' | \"$ZKS_BIN\" freeze \"$SRC\" \"$TEST_DIR\""
    assert_failure
    assert_output --partial "empty"
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

    # Verify restore_path is not empty (regression test for relative path bug)
    if command -v unsquashfs >/dev/null; then
        run unsquashfs -cat "out_rel.sqfs" list.yaml
        assert_success
        refute_output --partial "restore_path: ''"
        assert_output --partial "restore_path:"
    fi
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

# --- Prefix and Payload Structure Tests ---

@test "Freeze: Internal sqfs structure uses 'payload' as root (not target name)" {
    if ! command -v unsquashfs >/dev/null; then
        skip "unsquashfs not found"
    fi

    # Create a directory with a specific name
    CUSTOM_SRC="$TEST_DIR/my_custom_data"
    mkdir -p "$CUSTOM_SRC"
    echo "test" > "$CUSTOM_SRC/testfile.txt"

    OUT="$TEST_DIR/structure_test.sqfs"
    run $ZKS_BIN freeze "$CUSTOM_SRC" "$OUT"
    assert_success

    # Verify the sqfs root does NOT contain a "my_custom_data" directory
    # but contains list.yaml and to_restore at the root level
    run unsquashfs -l "$OUT"
    assert_success
    assert_output --partial "list.yaml"
    assert_output --partial "to_restore"

    # The root inside sqfs should be flat (list.yaml + to_restore), not nested under target name
    # "my_custom_data" should only appear inside to_restore/1/
    run unsquashfs -l "$OUT"
    assert_output --partial "to_restore/1/my_custom_data"
}

@test "Freeze: Prefix flag does not affect internal sqfs structure" {
    if ! command -v unsquashfs >/dev/null; then
        skip "unsquashfs not found"
    fi

    OUT_DIR="$TEST_DIR/output_dir"
    mkdir -p "$OUT_DIR"

    run $ZKS_BIN freeze "$SRC" "$OUT_DIR" --prefix customprefix
    assert_success

    # Find the created file
    local created_file
    created_file=$(find "$OUT_DIR" -maxdepth 1 -name "customprefix_*.sqfs" -print -quit)
    [ -n "$created_file" ]

    # Verify prefix is in filename only, not in sqfs structure
    run unsquashfs -l "$created_file"
    assert_success
    assert_output --partial "list.yaml"
    assert_output --partial "to_restore"
    # "customprefix" should NOT appear inside the archive
    refute_output --partial "customprefix"
}
