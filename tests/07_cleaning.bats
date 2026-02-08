#!/usr/bin/env bats
load 'test_helper/bats-support/load'
load 'test_helper/bats-assert/load'

setup() {
    export TEST_DIR=$(mktemp -d -t zks-cleaning.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "content" > "$SRC/file.txt"
    
    # Setup ZKS_BIN and RM_BIN
    if [ -z "$ZKS_BIN" ]; then
        if [ -f "./target/debug/0k" ]; then
            export ZKS_BIN="./target/debug/0k"
        elif [ -f "../target/debug/0k" ]; then
            export ZKS_BIN="../target/debug/0k"
        else
            export ZKS_BIN="$(git rev-parse --show-toplevel)/target/debug/0k"
        fi
    fi

    # Derive RM_BIN from ZKS_BIN location if not set
    if [ -z "$RM_BIN" ]; then
        BIN_DIR=$(dirname "$ZKS_BIN")
        export RM_BIN="$BIN_DIR/0k-safe-rm"
    fi
    
    # Add directory of ZKS_BIN to PATH for 0k-core
    BIN_DIR=$(dirname "$ZKS_BIN")
    export PATH="$BIN_DIR:$PATH"
    
    # Override XDG_CACHE_HOME to isolate tests
    export XDG_CACHE_HOME="$TEST_DIR/cache"
    mkdir -p "$XDG_CACHE_HOME"
}

teardown() {
    rm -rf "$TEST_DIR"
}

# --- Cleanup Logic Tests ---

@test "Cleanup: Staging area is removed after successful freeze" {
    OUT="$TEST_DIR/archive.sqfs"
    run $ZKS_BIN freeze "$SRC" "$OUT"
    assert_success
    
    # Check that stazis cache dir does NOT contain build_* directories
    # Note: stazis creates .../zero-kelvin-stazis/build_...
    CACHE_ROOT="$XDG_CACHE_HOME/zero-kelvin-stazis"
    
    if [ -d "$CACHE_ROOT" ]; then
        # Count dirs starting with build_
        run find "$CACHE_ROOT" -maxdepth 1 -type d -name "build_*"
        assert_output ""
    fi
}

@test "Cleanup: freeze.sh is NOT included in the archive" {
    if ! command -v unsquashfs >/dev/null; then
        skip "unsquashfs not found"
    fi

    OUT="$TEST_DIR/clean_structure.sqfs"
    run $ZKS_BIN freeze "$SRC" "$OUT"
    assert_success
    
    run unsquashfs -l "$OUT"
    assert_success
    refute_output --partial "freeze.sh"
    assert_output --partial "list.yaml"
}

@test "Cleanup: 0k-safe-rm removes empty file" {
    EMPTY="$TEST_DIR/empty_file"
    touch "$EMPTY"
    
    run "$RM_BIN" "$EMPTY"
    assert_success
    assert [ ! -f "$EMPTY" ]
}

@test "Cleanup: 0k-safe-rm removes empty directory" {
    EMPTY_DIR="$TEST_DIR/empty_dir"
    mkdir "$EMPTY_DIR"
    
    run "$RM_BIN" "$EMPTY_DIR"
    assert_success
    assert [ ! -d "$EMPTY_DIR" ]
}

@test "Cleanup: 0k-safe-rm FAILS and PRESERVES non-empty file" {
    NON_EMPTY="$TEST_DIR/data.txt"
    echo "data" > "$NON_EMPTY"
    
    run "$RM_BIN" "$NON_EMPTY"
    assert_failure
    # Logic might differ: does it return error or success if not removed?
    # Assuming success but no action if not empty.
    # Let's verify file exists.
    assert [ -f "$NON_EMPTY" ]
}

@test "Cleanup: 0k-safe-rm REMOVES directory with only empty files" {
    DIR="$TEST_DIR/dir_empty_files"
    mkdir "$DIR"
    touch "$DIR/file"
    
    run "$RM_BIN" "$DIR"
    assert_success
    assert [ ! -d "$DIR" ]
}

@test "Cleanup: 0k-safe-rm FAILS and PRESERVES directory with non-empty file" {
    DIR="$TEST_DIR/dir_data"
    mkdir "$DIR"
    echo "content" > "$DIR/file"
    
    run "$RM_BIN" "$DIR"
    assert_failure
    assert [ -d "$DIR" ]
    assert [ -f "$DIR/file" ]
    assert_output --partial "non-empty"
}

@test "Cleanup: 0k-safe-rm REMOVES recursive (empty dir + empty file)" {
    DIR="$TEST_DIR/dir_recursive_empty"
    mkdir -p "$DIR/subdir"
    touch "$DIR/file"
    
    run "$RM_BIN" "$DIR"
    assert_success
    assert [ ! -d "$DIR" ]
}

@test "Cleanup: 0k-safe-rm FAILS and PRESERVES recursive (atomic check)" {
    DIR="$TEST_DIR/dir_recursive_data"
    mkdir -p "$DIR/subdir"
    echo "content" > "$DIR/file"
    
    run "$RM_BIN" "$DIR"
    assert_failure
    assert [ -d "$DIR" ]
    # Atomic check: empty subdir must ALSO be preserved because operation aborted
    assert [ -d "$DIR/subdir" ] 
    assert [ -f "$DIR/file" ]
    assert_output --partial "non-empty"
}
