#!/usr/bin/env bats

setup_file() {
    # Create a "Golden Archive" ONCE before all tests
    export TMP_ENV=$(mktemp -d -t zks-mount.XXXXXX)
    export GOLDEN_ARCHIVE="$TMP_ENV/golden.sqfs"
    
    mkdir -p "$TMP_ENV/src"
    echo "Hello" > "$TMP_ENV/src/file.txt"
    
    # If this fails, BATS will report "Setup failed", which is correct
    # since testing mount without an archive is impossible.
    $ZKS_SQM_BIN create "$TMP_ENV/src" "$GOLDEN_ARCHIVE" --no-progress
}

teardown_file() {
    rm -rf "$TMP_ENV"
}

setup() {
    # Create a unique subfolder for each test inside the global TMP_ENV
    # so teardown can clean up only what belongs to the current test
    export TEST_MNT_ROOT="$TMP_ENV/mnt_$(date +%s)_$RANDOM"
    mkdir -p "$TEST_MNT_ROOT"
}

teardown() {
    # Aggressively attempt to unmount everything inside TMP_ENV
    # This ensures handles are closed even if a test fails
    find "$TMP_ENV" -maxdepth 2 -type d -exec fusermount -u {} 2>/dev/null \; || true
}
@test "Smoke: Verify mount status" {
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_smoke"
    [ "$status" -eq 0 ]
    
    # Unmount immediately to avoid affecting the next test
    $ZKS_SQM_BIN umount "$TMP_ENV/mnt_smoke"
}


@test "Logic: Mount archive, automatic creation of mount directory" {
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt"
    [ -d "$TMP_ENV/mnt" ]
    # Unmount immediately to avoid affecting the next test
    $ZKS_SQM_BIN umount "$TMP_ENV/mnt"
}



@test "Logic: Verify files inside mounted archive" {
    # Here we create the directory for targeted error testing
    mkdir -p "$TMP_ENV/mnt2"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt2"
    
    run cat "$TMP_ENV/mnt2/file.txt"
    [ "$output" = "Hello" ]
    
    $ZKS_SQM_BIN umount "$TMP_ENV/mnt2"
}

@test "Logic: Mount archive, automatic creation of mount directory (Auto-Gen)" {
    # 1. Runs: `squash_manager-rs mount <sqfs-image>` without specifying a directory
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE"
    
    # 2. Verify success
    [ "$status" -eq 0 ]
    
    # 3. Verify output message
    # <No mount point specified. Using secure local path for stability: <dir_name>
    # Using partial matching as the path is absolute
    [[ "$output" == *"No mount point specified. Using secure local path for stability:"* ]]
    
    # 4. Parse the generated path (the suffix)
    local generated_path=$(echo "$output" | grep "Using secure local path for stability:" | awk -F': ' '{print $2}' | tr -d '[:space:]')
    
    # 5. Verify naming format: mount_<prefix>_<unix_time>_<random_number, 6 digits>
    # Prefix = golden.sqfs
    local dirname=$(basename "$generated_path")
    local prefix="mount_golden.sqfs"
    
    # Prefix check
    [[ "$dirname" == "$prefix"* ]]
    
    # Structure check (regex)
    regex="^${prefix}_[0-9]+_[0-9]{6}$"
    [[ "$dirname" =~ $regex ]]
    
    # 6. Verify directory was created
    [ -d "$generated_path" ]
    
    # 7. Verify content
    [ -f "$generated_path/file.txt" ]
    run cat "$generated_path/file.txt"
    [ "$output" = "Hello" ]
    
    # Cleanup
    $ZKS_SQM_BIN umount "$generated_path"
    rmdir "$generated_path" 2>/dev/null || true
}

@test "Error: Image file does not exist" {
    run $ZKS_SQM_BIN mount "non_existent_file.sqfs"
    [ "$status" -ne 0 ]
    
    # Check for specific error message
    # "No such file or directory" or "does not exist"
    # Case insensitive grep
    echo "$output" | grep -i -E "invalid path|no such file|does not exist"
}

@test "Logic: Name collision handling" {
    # 1. First run
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE"
    [ "$status" -eq 0 ]
    local path1=$(echo "$output" | grep "Using secure local path for stability:" | awk -F': ' '{print $2}' | tr -d '[:space:]')
    
    # 2. Second run immediately after
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE"
    [ "$status" -eq 0 ]
    local path2=$(echo "$output" | grep "Using secure local path for stability:" | awk -F': ' '{print $2}' | tr -d '[:space:]')
    
    # 3. Verify paths are unique
    [ "$path1" != "$path2" ]
    
    # 4. Verify both exist and work
    [ -d "$path1" ]
    [ -d "$path2" ]
    
    # Cleanup
    $ZKS_SQM_BIN umount "$path1"
    $ZKS_SQM_BIN umount "$path2"
    rmdir "$path1" 2>/dev/null || true
    rmdir "$path2" 2>/dev/null || true
}
