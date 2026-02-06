#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    export TEMP_DIR="$(mktemp -d)"
    export MOCK_BIN="$TEMP_DIR/bin"
    export ORIGINAL_PATH="$PATH"
    
    mkdir -p "$MOCK_BIN"
    export PATH="$MOCK_BIN:$PATH"
    
    # Input
    mkdir "$TEMP_DIR/input"
    touch "$TEMP_DIR/input/file"

    if [ -z "$ZKS_BIN" ]; then
         export ZKS_BIN="./target/debug/zks-rs"
    fi
}

teardown() {
    export PATH="$ORIGINAL_PATH"
    rm -rf "$TEMP_DIR"
}

@test "Error: Bubble up fallocate/dd failures" {
    # 1. Mock du to return a size (so we try to allocate)
    cat <<EOF > "$MOCK_BIN/du"
#!/bin/sh
echo "1000 input" # positive size
exit 0
EOF
    chmod +x "$MOCK_BIN/du"

    # 2. Mock fallocate to fail with message
    cat <<EOF > "$MOCK_BIN/fallocate"
#!/bin/sh
echo "Simulated fallocate failure" >&2
exit 1
EOF
    chmod +x "$MOCK_BIN/fallocate"

    # 3. Mock dd to fail with message
    cat <<EOF > "$MOCK_BIN/dd"
#!/bin/sh
echo "Simulated dd failure" >&2
exit 1
EOF
    chmod +x "$MOCK_BIN/dd"
    
    # Mock other tools to pass
    ln -sf /bin/true "$MOCK_BIN/stat"
    
    # We also need unshare to just run arguments, OR mocks are inside PATH?
    # real unshare might clear PATH or environment?
    # If zks-rs runs unshare, and unshare runs squash_manager-rs...
    # We might need to mock unshare to just exec "$@" to keep our PATH.
    cat <<EOF > "$MOCK_BIN/unshare"
#!/bin/sh
shift 5 # skip -m -U -r --propagation private
# Verify we have a command to run
if [ -z "\$1" ]; then
    echo "Mock unshare: No command provided" >&2
    exit 1
fi
# Execute remaining command
exec "\$@"
EOF
    chmod +x "$MOCK_BIN/unshare"
    
    # 4. Mock mount (since we don't really unshare, we can't mount)
    cat <<EOF > "$MOCK_BIN/mount"
#!/bin/sh
# Always succeed
exit 0
EOF
    chmod +x "$MOCK_BIN/mount"
    # We need to ensure squash_manager-rs uses OUR mocks. 
    # Since we built zks-rs which is a multicall binary or similar?
    # No, squash_manager-rs is a separate binary or same binary?
    # The project creates `zks-rs` and `squash_manager-rs`.
    # Tests use `ZKS_BIN`. `squash_manager-rs` is expected to be in PATH.
    # We should symlink $ZKS_BIN to $MOCK_BIN/squash_manager-rs?
    # Wait, `squash_manager-rs` is where the logic is. We need the REAL binary but using MOCK subcommands (fallocate).
    # So we symlink the REAL binary to mock bin.
    ln -sf "$(realpath "$ZKS_BIN")" "$MOCK_BIN/squash_manager-rs"
    # Actually $ZKS_BIN might be `zks-rs`. Does it work as `squash_manager-rs`?
    # No, they are separate binaries in target/debug/.
    TARGET_DIR="$(dirname "$ZKS_BIN")"
    if [ -f "$TARGET_DIR/squash_manager-rs" ]; then
         ln -sf "$TARGET_DIR/squash_manager-rs" "$MOCK_BIN/squash_manager-rs"
    else
         # Fallback if names differ
         ln -sf "$ZKS_BIN" "$MOCK_BIN/squash_manager-rs"
    fi

    # Run freeze -e with fakeroot to bypass "must be run as root" check in squash_manager-rs
    # (Since we are testing error handling of fallocate, not privilege escalation)
    if command -v fakeroot >/dev/null 2>&1; then
        run fakeroot "$ZKS_BIN" freeze "$TEMP_DIR/input" "$TEMP_DIR/output.sqfs" -e --no-progress
    else
        echo "Skip: fakeroot not found"
        skip "fakeroot not found"
    fi
    
    assert_failure
    # Assert we see the detailed error
    assert_output --partial "Failed to create container file"
    assert_output --partial "fallocate error: 'Simulated fallocate failure'"
    assert_output --partial "dd error: 'Simulated dd failure'"
}
