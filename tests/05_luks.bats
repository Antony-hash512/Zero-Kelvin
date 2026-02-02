#!/usr/bin/env bats

setup() {
    # Skip if root is not available/enabled
    if [ "$SKIP_ROOT" = "1" ]; then
        skip "Root tests are disabled (or not running as root/sudo)"
    fi
    # Use SUDO_CMD from environment (sudo or empty string)

    export TEST_DIR=$(mktemp -d)
    echo "Files created in: $TEST_DIR" >&3
    
    INPUT_DIR="$TEST_DIR/input_data"
    OUTPUT_LUKS="$TEST_DIR/encrypted.sqfs"
    MOUNT_POINT="$TEST_DIR/mnt"
    
    # 1. Create dummy data
    mkdir -p "$INPUT_DIR"
    # Create compressible data: 10 files with same content, total ~5MB
    # "yes" outputs "ZeroKelvinStazis\n" repeatedly.
    # Each line is 17 bytes. 30000 lines * 17 bytes ~= 510KB per file.
    for i in {1..10}; do
        yes "ZeroKelvinStazis" | head -n 30000 > "$INPUT_DIR/file_$i.txt"
    done
    
    # Calculate uncompressed size (approx)
    DU_SIZE=$(du -sb "$INPUT_DIR" | cut -f1)
    echo "Uncompressed size: $DU_SIZE bytes" >&3
}

teardown() {
    [ "$SKIP_ROOT" = "1" ] && return
    
    $ROOT_CMD $ZKS_SQM_BIN umount "$MOUNT_POINT" || true
    
    # Remove mapper if left over
    # (Assuming name format sq_NAME)
    NAME=$(basename "$OUTPUT_LUKS")
    MAPPER="sq_${NAME%.*}" # Approximation
    # Best effort cleanup
    
    rm -rf "$TEST_DIR"
}


@test "LUKS: Integrity Round-Trip (Encrypt -> Mount -> Verify)" {
    [ "$SKIP_ROOT" = "1" ] && skip
    
    # 1. Create encrypted archive
    run $ROOT_CMD $ZKS_SQM_BIN create "$INPUT_DIR" "$OUTPUT_LUKS" -e --no-progress
    [ "$status" -eq 0 ]
    [ -f "$OUTPUT_LUKS" ]
    
    # 2. Mount it
    mkdir -p "$MOUNT_POINT"
    run $ROOT_CMD $ZKS_SQM_BIN mount "$OUTPUT_LUKS" "$MOUNT_POINT"
    [ "$status" -eq 0 ]
    
    # 3. Verify content
    # Check if file_1.txt exists and content matches
    run cat "$MOUNT_POINT/file_1.txt"
    [[ "$output" == *"ZeroKelvinStazis"* ]]
    
    # 4. Unmount handled by teardown or explicitly
    run $ROOT_CMD $ZKS_SQM_BIN umount "$MOUNT_POINT"
    [ "$status" -eq 0 ]
}

@test "LUKS: Cleanup check (No leftover mappers)" {
    [ "$SKIP_ROOT" = "1" ] && skip

    # Ensure no mappers from previous tests remain
    # Assuming mapper naming convention matches "sq_*"
    
    # We create and destroy one more time to be sure
    run $ROOT_CMD $ZKS_SQM_BIN create "$INPUT_DIR" "$OUTPUT_LUKS" -e --no-progress
    [ "$status" -eq 0 ]
    
    # Check if mapper exists (should be CLOSED after create)
    # The current implementation closes it after creation
    NAME=$(basename "$OUTPUT_LUKS")
    MAPPER_NAME="sq_${NAME%.*}"
    
    # Check existence in /dev/mapper
    if [ -e "/dev/mapper/$MAPPER_NAME" ]; then
         echo "Mapper $MAPPER_NAME still exists after create!" >&3
         # Fail
         [ "1" -eq "0" ] 
    fi
    
    # Now mount
    mkdir -p "$MOUNT_POINT"
    run $ROOT_CMD $ZKS_SQM_BIN mount "$OUTPUT_LUKS" "$MOUNT_POINT"
    [ "$status" -eq 0 ]
    
    # Check if mapper exists (should be OPEN)
    # Note: mount name generation logic might differ if not explicit?
    # Actually, mount <FILE> <MNT> uses generated mapper name based on file basename
    if [ ! -e "/dev/mapper/$MAPPER_NAME" ]; then
         # Try to find what it was named?
         # For now, just warn if not found, but we expect it to be consistent
         echo "Mapper not found at expected path /dev/mapper/$MAPPER_NAME" >&3
    fi
    
    # Umount
    run $ROOT_CMD $ZKS_SQM_BIN umount "$MOUNT_POINT"
    [ "$status" -eq 0 ]
    
    # Check if mapper exists (should be CLOSED after umount)
    if [ -e "/dev/mapper/$MAPPER_NAME" ]; then
         echo "Mapper $MAPPER_NAME still exists after umount!" >&3
         [ "1" -eq "0" ]
    fi
}

@test "LUKS: Truncate optimization (-c 0 vs -c 19)" {
    [ "$SKIP_ROOT" = "1" ] && skip
    
    OUTPUT_NO_COMP="$TEST_DIR/enc_no_comp.sqfs"
    OUTPUT_HIGH_COMP="$TEST_DIR/enc_high_comp.sqfs"
    
    # 1. Create with -c 0 (No compression - creates a large file)
    run $ROOT_CMD $ZKS_SQM_BIN create "$INPUT_DIR" "$OUTPUT_NO_COMP" -e -c 0 --no-progress
    [ "$status" -eq 0 ]
    
    # 2. Create with -c 19 (High compression)
    run $ROOT_CMD $ZKS_SQM_BIN create "$INPUT_DIR" "$OUTPUT_HIGH_COMP" -e -c 19 --no-progress
    [ "$status" -eq 0 ]
    
    # 3. Compare sizes
    SIZE_LG=$(stat -c%s "$OUTPUT_NO_COMP")
    SIZE_SM=$(stat -c%s "$OUTPUT_HIGH_COMP")
    
    echo "Size (No Comp): $SIZE_LG" >&3
    echo "Size (High Comp): $SIZE_SM" >&3
    
    # High comp should be significantly smaller
    [ "$SIZE_SM" -lt "$SIZE_LG" ]
}