#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    # Create test environment
    export TEST_DIR="$(mktemp -d)"
    mkdir -p "$TEST_DIR"
    export ZKS_BIN="${ZKS_BIN:-$BATS_TEST_DIRNAME/../target/debug/zks-rs}"
    
    # Ensure binaries are in PATH
    export PATH="$(dirname "$ZKS_BIN"):$PATH"
    
    # Create valid dummy source
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC/dir"
    echo "content1" > "$SRC/file1.txt"
    echo "content2" > "$SRC/dir/file2.txt"

    # Create dummy archive
    export ARCHIVE="$TEST_DIR/archive.sqfs"
    run zks-rs freeze "$SRC" "$ARCHIVE" --no-progress
    assert_success
}

teardown() {
    rm -rf "$TEST_DIR"
}

@test "Check: Detect matching files" {
    run zks-rs check "$ARCHIVE"
    assert_success
    assert_output --partial "MATCH"
    assert_output --partial "Matched:"
    assert_output --partial "src/file1.txt"
    assert_output --partial "src/dir/file2.txt"
}

@test "Check: Detect modified content (Fast Check mismatch)" {
    # Change size to trigger metadata mismatch
    echo "longer content" > "$SRC/file1.txt"
    
    run zks-rs check "$ARCHIVE"
    assert_success # Command succeeds, but reports mismatch
    assert_output --partial "MISMATCH"
    assert_output --partial "file1.txt"
}

@test "Check: Detect missing files" {
    rm "$SRC/file1.txt"
    
    run zks-rs check "$ARCHIVE"
    assert_success
    assert_output --partial "MISSING"
    assert_output --partial "file1.txt"
}

@test "Check: Detect content mismatch (Fast Check false positive)" {
    # Same size, different content (simulate simple fast check bypass if timestamps ignored or identical)
    # Note: Modern fast check uses mtime, so we'd need to touch to match mtime to fool it fully,
    # but here we just want to see if --use-cmp catches what fast check MIGHT miss if sizes match.
    printf "content1" > "$SRC/file1.txt" 
    # Just to be sure, let's pretend mtime is same or check doesn't rely solely on strict mtime match for "match" status without cmp
    
    # Actually, for this test, let's change content but keep size same
    echo -n "cont1ntX" > "$SRC/file1.txt" 
    
    # Without --use-cmp, it might pass if only checking size (and we ignore mtime difference for a moment or mtime is close)
    # But usually mtime differs. 
    # Let's verify --use-cmp specifically catches content change:
    
    run zks-rs check "$ARCHIVE" --use-cmp
    assert_success
    assert_output --partial "MISMATCH"
}

@test "Check: Force delete with Safety Gate (Skip newer)" {
    # Make local file newer
    touch -d "next hour" "$SRC/file1.txt"
    
    run zks-rs check "$ARCHIVE" --delete
    assert_success
    assert_output --partial "SKIPPED (Newer)"
    assert [ -f "$SRC/file1.txt" ]
}

@test "Check: Force delete (Success)" {
    # Ensure local file matches (old enough)
    # We just created archive, so local might be "newer" by microseconds or same.
    # Let's enforce local is OLDER to allow deletion (or same)
    touch -d "last hour" "$SRC/file1.txt"
    
    run zks-rs check "$ARCHIVE" --delete
    assert_success
    assert_output --partial "DELETED"
    assert [ ! -f "$SRC/file1.txt" ]
}
