#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    # Create test environment
    export TEST_DIR="$(mktemp -d)"
    mkdir -p "$TEST_DIR"
    export ZKS_BIN="${ZKS_BIN:-$BATS_TEST_DIRNAME/../target/debug/0k}"
    
    # Ensure binaries are in PATH
    export PATH="$(dirname "$ZKS_BIN"):$PATH"
    
    # Create valid dummy source
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "content1" > "$SRC/file1.txt"

    # Create dummy archive
    export ARCHIVE="$TEST_DIR/archive.sqfs"
    run 0k freeze "$SRC" "$ARCHIVE" --no-progress
    assert_success
}

teardown() {
    rm -rf "$TEST_DIR"
}

@test "Force Delete: Trigger Safety Gate and Hint" {
    # Make local file newer (simulate change/touch)
    touch -d "next hour" "$SRC/file1.txt"
    
    # Run check --delete (should skip)
    run 0k check "$ARCHIVE" --delete
    assert_success
    assert_output --partial "SKIPPED (Newer)"
    assert_output --partial "Hint:"
    assert_output --partial "--force-delete"
    assert_output --partial "-D"
    
    # File should still exist
    assert [ -f "$SRC/file1.txt" ]
}

@test "Force Delete: Fail without --delete" {
    touch -d "next hour" "$SRC/file1.txt"
    run 0k check "$ARCHIVE" -D
    assert_failure
    # Clap error message for requires:
    assert_output --partial "the following required arguments were not provided"
    assert_output --partial "--delete"
}

@test "Force Delete: Success with -D" {
    # Make local file newer
    touch -d "next hour" "$SRC/file1.txt"
    
    # Run check --delete -D
    run 0k check "$ARCHIVE" --delete -D
    assert_success
    assert_output --partial "DELETED"
    refute_output --partial "SKIPPED (Newer)"
    
    # File should be gone
    assert [ ! -f "$SRC/file1.txt" ]
}

@test "Force Delete: Success with --force-delete" {
    # Make local file newer
    touch -d "next hour" "$SRC/file1.txt"
    
    # Run check --delete --force-delete
    run 0k check "$ARCHIVE" --delete --force-delete
    assert_success
    assert_output --partial "DELETED"
    refute_output --partial "SKIPPED (Newer)"
    
    # File should be gone
    assert [ ! -f "$SRC/file1.txt" ]
}