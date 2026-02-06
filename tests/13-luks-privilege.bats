#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    export TEMP_DIR="$(mktemp -d)"
    export MOCK_BIN="$TEMP_DIR/bin"
    export ORIGINAL_PATH="$PATH"
    
    mkdir -p "$MOCK_BIN"
    export PATH="$MOCK_BIN:$PATH"
    
    touch "$TEMP_DIR/input"

    if [ -z "$ZKS_BIN" ]; then
         export ZKS_BIN="./target/debug/zks-rs"
    fi
}

teardown() {
    export PATH="$ORIGINAL_PATH"
    rm -rf "$TEMP_DIR"
}

@test "Privilege: Auto-escalate on LUKS device-mapper error" {
    # 1. Mock sudo
    cat <<EOF > "$MOCK_BIN/sudo"
#!/bin/sh
echo "MOCK_SUDO_DETECTED"
# In a real retry, sudo would run the command. 
# Here we just want to prove we reached this point.
# We exit 0 to break the loop or let zks think it succeeded?
# If we exit 0, zks will say "Success".
exit 0
EOF
    chmod +x "$MOCK_BIN/sudo"

    # 2. Mock squash_manager-rs to fail with DM error
    # This simulates the user's reported error.
    cat <<EOF > "$MOCK_BIN/squash_manager-rs"
#!/bin/sh
echo "Initializing LUKS..."
echo "Cannot initialize device-mapper. Is dm_mod kernel module loaded?" >&2
exit 1
EOF
    chmod +x "$MOCK_BIN/squash_manager-rs"

    # 3. Run freeze with -e
    run "$ZKS_BIN" freeze "$TEMP_DIR/input" "$TEMP_DIR/out.sqfs" -e --no-progress
    
    # 4. Assert
    assert_success
    assert_output --partial "MOCK_SUDO_DETECTED"
    # assert_output --partial "Retrying with elevation" # Check engine message?
    # Actually, zks-rs binary prints "sudo ...".
}

@test "Privilege: Auto-escalate on explicit 'must be run as root' error" {
    # 1. Mock sudo
    cat <<EOF > "$MOCK_BIN/sudo"
#!/bin/sh
echo "MOCK_SUDO_DETECTED"
exit 0
EOF
    chmod +x "$MOCK_BIN/sudo"

    # 2. Mock squash_manager-rs to return the specific RootRequired error
    cat <<EOF > "$MOCK_BIN/squash_manager-rs"
#!/bin/sh
echo "Error: Operation failed: LUKS creation requires root privileges: must be run as root" >&2
exit 1
EOF
    chmod +x "$MOCK_BIN/squash_manager-rs"

    # 3. Run freeze with -e
    run "$ZKS_BIN" freeze "$TEMP_DIR/input" "$TEMP_DIR/out.sqfs" -e --no-progress
    
    # 4. Verify
    assert_success
    assert_output --partial "MOCK_SUDO_DETECTED"
}
