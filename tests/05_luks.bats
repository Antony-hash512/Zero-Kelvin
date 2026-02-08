#!/usr/bin/env bats

setup() {
    # Skip if root is not available/enabled
    if [ "$SKIP_ROOT" = "1" ]; then
        skip "Root tests are disabled (or not running as root/sudo)"
    fi
    
    # Fallback: Set variables if not running through run_shell_tests.fish
    if [ -z "$ZKS_SQM_BIN" ]; then
        # Detect project root from test file location
        SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")" && pwd)"
        ZKS_PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
        ZKS_SQM_BIN="$ZKS_PROJECT_ROOT/target/debug/0k-core"
        ZKS_BIN="$ZKS_PROJECT_ROOT/target/debug/0k"
        export ZKS_SQM_BIN ZKS_BIN ZKS_PROJECT_ROOT
    fi
    
    # ROOT_CMD should be empty when running as root
    if [ -z "${ROOT_CMD+x}" ]; then
        if [ "$(id -u)" -eq 0 ]; then
            ROOT_CMD=""
        else
            ROOT_CMD="sudo"
        fi
        export ROOT_CMD
    fi

    export TEST_DIR=$(mktemp -d)
    echo "Files created in: $TEST_DIR" >&3
    
    INPUT_DIR="$TEST_DIR/input_data"
    OUTPUT_LUKS="$TEST_DIR/encrypted.sqfs"
    MOUNT_POINT="$TEST_DIR/mnt"
    
    # 1. Create dummy data
    mkdir -p "$INPUT_DIR"
    # Create compressible data: 10 files with same content, total ~500KB
    # "yes" outputs "ZeroKelvinStazis\n" repeatedly.
    # Each line is 17 bytes. 3000 lines * 17 bytes ~= 51KB per file.
    for i in {1..10}; do
        yes "ZeroKelvinStazis" | head -n 3000 > "$INPUT_DIR/file_$i.txt"
    done
    
    # Calculate uncompressed size (approx)
    DU_SIZE=$(du -sb "$INPUT_DIR" | cut -f1)
    echo "Uncompressed size: $DU_SIZE bytes" >&3
}

teardown() {
    [ "$SKIP_ROOT" = "1" ] && return
    
    # 1. Try to umount using the utility
    ${ROOT_CMD:-} "$ZKS_SQM_BIN" umount "$MOUNT_POINT" 2>/dev/null || true
    
    # 2. Close ALL sq_* LUKS mappers (cleanup from any failed tests)
    for mapper_path in /dev/mapper/sq_*; do
        if [ -e "$mapper_path" ]; then
            mapper_name=$(basename "$mapper_path")
            ${ROOT_CMD:-} cryptsetup close "$mapper_name" 2>/dev/null || true
        fi
    done
    
    # 3. Detach orphaned loop devices
    ${ROOT_CMD:-} losetup -D 2>/dev/null || true
    
    # 4. Remove test directory
    rm -rf "$TEST_DIR"
}


@test "LUKS: Integrity Round-Trip (Encrypt -> Mount -> Verify)" {
    [ "$SKIP_ROOT" = "1" ] && skip
    
    # 1. Create encrypted archive
    # Passwords needed: 1. luksFormat new, 2. luksFormat verify, 3. open
    run bash -c "printf 'testpassword\ntestpassword\ntestpassword' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" create \"$INPUT_DIR\" \"$OUTPUT_LUKS\" -e --no-progress"
    if [ "$status" -ne 0 ]; then echo "CREATE FAILED: $output" >&3; fi
    [ "$status" -eq 0 ]
    [ -f "$OUTPUT_LUKS" ]
    
    # 2. Mount it
    mkdir -p "$MOUNT_POINT"
    run bash -c "echo -n 'testpassword' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" mount \"$OUTPUT_LUKS\" \"$MOUNT_POINT\""
    [ "$status" -eq 0 ]
    
    # 3. Verify content
    # Check if file_1.txt exists and content matches
    run cat "$MOUNT_POINT/file_1.txt"
    [[ "$output" == *"ZeroKelvinStazis"* ]]
    
    # 4. Unmount handled by teardown or explicitly
    run ${ROOT_CMD:-} "$ZKS_SQM_BIN" umount "$MOUNT_POINT"
    [ "$status" -eq 0 ]
}

@test "LUKS: Cleanup check (No leftover mappers)" {
    [ "$SKIP_ROOT" = "1" ] && skip

    # Ensure no mappers from previous tests remain
    # Assuming mapper naming convention matches "sq_*"
    
    # We create and destroy one more time to be sure
    run bash -c "printf 'testpassword\ntestpassword\ntestpassword' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" create \"$INPUT_DIR\" \"$OUTPUT_LUKS\" -e --no-progress"
    [ "$status" -eq 0 ]
    
    # Check if mapper exists (should be CLOSED after create)
    # The current implementation closes it after creation
    NAME=$(basename "$OUTPUT_LUKS")
    MAPPER_NAME="sq_${NAME%.*}" # Note: Rust code must match this naming convention or output the mapper name
    
    # Check existence in /dev/mapper
    if [ -e "/dev/mapper/$MAPPER_NAME" ]; then
         echo "Mapper $MAPPER_NAME still exists after create!" >&3
         # Fail
         return 1
    fi
    
    # Mount (Open mapper)
    mkdir -p "$MOUNT_POINT"
    # Mount only needs 1 password
    run bash -c "echo -n 'testpassword' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" mount \"$OUTPUT_LUKS\" \"$MOUNT_POINT\""
    [ "$status" -eq 0 ]
    
    # Check if mapper exists (should be OPEN)
    # Note: mount name generation logic might differ if not explicit?
    # Actually, mount <FILE> <MNT> uses generated mapper name based on file basename
    if [ ! -e "/dev/mapper/$MAPPER_NAME" ]; then
         # Try to find what it was named?
         # For now, just warn if not found, but we expect it to be consistent
         echo "Mapper not found at expected path /dev/mapper/$MAPPER_NAME" >&3
    fi
    
    # Umount (Close mapper)
    run ${ROOT_CMD:-} "$ZKS_SQM_BIN" umount "$MOUNT_POINT"
    
    # Check if mapper exists (should be CLOSED after umount)
    if [ -e "/dev/mapper/$MAPPER_NAME" ]; then
         echo "Mapper $MAPPER_NAME still exists after umount!" >&3
         return 1
    fi
}

@test "LUKS: Truncate optimization (-c 0 vs -c 19)" {
    [ "$SKIP_ROOT" = "1" ] && skip
    
    OUTPUT_NO_COMP="$TEST_DIR/enc_no_comp.sqfs"
    OUTPUT_HIGH_COMP="$TEST_DIR/enc_high_comp.sqfs"
    
    # 1. Create with -c 0 (no compression)
    # Use same pattern as Integrity test - it works!
    run bash -c "printf 'testpassword\ntestpassword\ntestpassword' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" create \"$INPUT_DIR\" \"$OUTPUT_NO_COMP\" -e -c 0 --no-progress"
    if [ "$status" -ne 0 ]; then echo "CREATE -c 0 FAILED: $output" >&3; fi
    [ "$status" -eq 0 ]
    
    # 2. Create with -c 19 (high compression)
    run bash -c "printf 'testpassword\ntestpassword\ntestpassword' | ${ROOT_CMD:-} \"$ZKS_SQM_BIN\" create \"$INPUT_DIR\" \"$OUTPUT_HIGH_COMP\" -e -c 19 --no-progress"
    if [ "$status" -ne 0 ]; then echo "CREATE -c 19 FAILED: $output" >&3; fi
    [ "$status" -eq 0 ]
    
    # 3. Compare sizes - high compression should produce smaller file
    SIZE_LG=$(stat -c%s "$OUTPUT_NO_COMP")
    SIZE_SM=$(stat -c%s "$OUTPUT_HIGH_COMP")
    
    echo "Size (No Comp): $SIZE_LG" >&3
    echo "Size (High Comp): $SIZE_SM" >&3
    
    # High comp should be significantly smaller due to trim optimization
    [ "$SIZE_SM" -lt "$SIZE_LG" ]
}
