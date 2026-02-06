#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    export TEMP_DIR="$(mktemp -d)"
    export MOCK_BIN="$TEMP_DIR/bin"
    export ORIGINAL_PATH="$PATH"
    
    mkdir -p "$MOCK_BIN"
    export PATH="$MOCK_BIN:$PATH"
    
    # Needs a real file to start freeze
    touch "$TEMP_DIR/input"

    # Ensure zks is built
    if [ -z "$ZKS_BIN" ]; then
         export ZKS_BIN="./target/debug/zks-rs"
    fi
}

teardown() {
    export PATH="$ORIGINAL_PATH"
    rm -rf "$TEMP_DIR"
}

@test "Friendly Error: No space left on device" {
    # We mock mksquashfs to fail.
    # We cannot easily mock the IO error code 28 from a shell script,
    # because that comes from the system call.
    # However, if we mock mksquashfs to exit 1 and print nothing, 
    # zks will just say "Operation Failed".
    #
    # Wait, ZksError::IoError(e) comes from Rust's std::fs calls or Command::spawn errors.
    # To trigger "No Space" (Error 28) we'd need to fill a disk.
    # 
    # ALTERNATIVE: We can mock a command that zks runs, but that returns an exit code? 
    # No, exit code isn't an IO Error 28.
    #
    # REALISTIC TEST: Creates a tiny loopback? Too complex and requires root.
    #
    # COMPROMISE for this test: We will verify "Bad Password" which matches on STRING content.
    # For "No Space", we might need to rely on unit tests in Rust where we can construct the Error manually.
    # 
    # Let's skip No Space in BATS and do it in Rust unit test if possible, 
    # OR accept we can't easily test it in BATS without loopback.
    skip "Cannot easily simulate OS Error 28 (No Space) in BATS without root/loopback"
}

@test "Friendly Error: Incorrect passphrase" {
    # This test uses -e, which requires ROOT. 
    # If not root, skip to avoid password prompt.
    if [ "$(id -u)" -ne 0 ]; then
        skip "Test requires root (for -e) to simulate passphrase failure without prompt"
    fi

    # Mock squash_manager-rs.
    # Since zks calls squash_manager-rs for the heavy lifting.
    
    # Create mock squash_manager-rs
    cat <<EOF > "$MOCK_BIN/squash_manager-rs"
#!/bin/sh
echo "Simulating failure..."
echo "No key available with this passphrase." >&2 
exit 1
EOF
    chmod +x "$MOCK_BIN/squash_manager-rs"

    # Run freeze with -e to trigger encrypted path
    run "$ZKS_BIN" freeze "$TEMP_DIR/input" "$TEMP_DIR/out.sqfs" -e --no-progress
    
    # Assert
    assert_failure
    assert_output --partial "Suggestion: Incorrect passphrase provided."
}
