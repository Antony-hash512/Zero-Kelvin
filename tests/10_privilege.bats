#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    export TEMP_DIR="$(mktemp -d)"
    export TEST_FILE="$TEMP_DIR/secret.txt"
    export MOCK_BIN="$TEMP_DIR/bin"
    
    # Create a secret file that is unreadable
    echo "TOP SECRET" > "$TEST_FILE"
    chmod 000 "$TEST_FILE"
    
    # Create mock sudo
    mkdir -p "$MOCK_BIN"
    original_path="$PATH"
    export PATH="$MOCK_BIN:$PATH"
    
    # Mock sudo to just print a marker
    cat <<EOF > "$MOCK_BIN/sudo"
#!/bin/sh
echo "MOCK_SUDO_DETECTED \$@"
exit 0
EOF
    chmod +x "$MOCK_BIN/sudo"
    
    # Ensure zks is built
    if [ -z "$ZKS_BIN" ]; then
        export ZKS_BIN="./target/debug/zks-rs"
    fi
}

teardown() {
    # Restore permissions to allow deletion
    chmod 600 "$TEST_FILE" || true
    rm -rf "$TEMP_DIR"
}

@test "Privilege: Auto-escalate on read permission denied" {
    # Run zks freeze on the unreadable file
    # We expect it to fail first, then retry with sudo (our mock)
    
    # Skip if running as root (root can always read)
    if [ "$(id -u)" -eq 0 ]; then
        skip "Running as root, cannot test permission denied"
    fi

    run "$ZKS_BIN" freeze "$TEST_FILE" "$TEMP_DIR/out.sqfs"
    
    # Check that it tried to escalate
    # Our mock sudo prints MOCK_SUDO_DETECTED
    assert_output --partial "Permission denied during freeze. Retrying with elevation..."
    assert_output --partial "MOCK_SUDO_DETECTED"
}

@test "Privilege: No escalation if permissions OK" {
    chmod 600 "$TEST_FILE"
    run "$ZKS_BIN" freeze "$TEST_FILE" "$TEMP_DIR/out_ok.sqfs"
    
    refute_output --partial "Retrying with elevation"
    assert_success
}
