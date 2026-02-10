#!/usr/bin/env bats

setup_file() {
    export TMP_ENV=$(mktemp -d -t zks-umount.XXXXXX)
    export GOLDEN_ARCHIVE="$TMP_ENV/golden.sqfs"
    
    mkdir -p "$TMP_ENV/src"
    echo "Hello" > "$TMP_ENV/src/file.txt"
    $ZKS_SQM_BIN create "$TMP_ENV/src" "$GOLDEN_ARCHIVE" --no-progress
}

teardown_file() {
    rm -rf "$TMP_ENV"
}

setup() {
    export TEST_MNT_ROOT="$TMP_ENV/mnt_$(date +%s)_$RANDOM"
    mkdir -p "$TEST_MNT_ROOT"
}

teardown() {
    # Clean up all mounts inside temporary folder
    find "$TMP_ENV" -maxdepth 2 -type d -exec fusermount -u {} 2>/dev/null \; || true
}

@test "Smoke: Unmount by explicit path (status 0)" {
    mkdir -p "$TMP_ENV/mnt_smoke"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_smoke"
    [ -d "$TMP_ENV/mnt_smoke" ]
    
    # Actual test
    run $ZKS_SQM_BIN umount "$TMP_ENV/mnt_smoke"
    [ "$status" -eq 0 ]
    
    # Verify it's no longer mounted (e.g., file inside is inaccessible or folder is empty/deleted)
    # squashfuse usually leaves empty dir.
    # Check if file exists inside (should NOT)
    [ ! -f "$TMP_ENV/mnt_smoke/file.txt" ]
    
    # On Linux, one could also check via `mount` or `/proc/mounts`, 
    # but indirect check via file accessibility is fine for Smoke.
}

@test "Logic: Unmount by explicit path (Directory removal)" {
    mkdir -p "$TMP_ENV/mnt_rmdir"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_rmdir"
    [ -d "$TMP_ENV/mnt_rmdir" ]
    
    # Actual test
    run $ZKS_SQM_BIN umount "$TMP_ENV/mnt_rmdir"
    [ "$status" -eq 0 ]
    
    # Verify the directory WAS REMOVED
    [ ! -d "$TMP_ENV/mnt_rmdir" ]
}

@test "Logic: Unmount by explicit path (Directory preservation)" {
    mkdir -p "$TMP_ENV/mnt_keepdir"
    touch "$TMP_ENV/mnt_keepdir/file.txt"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_keepdir"
    [ -d "$TMP_ENV/mnt_keepdir" ]
    
    # Actual test
    run $ZKS_SQM_BIN umount "$TMP_ENV/mnt_keepdir"
    [ "$status" -eq 0 ]
    
    # Verify the directory WAS NOT REMOVED
    [ -d "$TMP_ENV/mnt_keepdir" ]

    # Verify that files still exist inside the directory
    [ -f "$TMP_ENV/mnt_keepdir/file.txt" ]

    # Очистка
    rm -rf "$TMP_ENV/mnt_keepdir"
}

@test "Logic: Unmount by image file path (Image Path) (status 0)" {
    mkdir -p "$TMP_ENV/mnt_img"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_img"
    
    # Test: pass path to .sqfs file instead of mount point
    run $ZKS_SQM_BIN umount "$GOLDEN_ARCHIVE"
    
    if [ "$status" -ne 0 ]; then
        echo "DEBUG: Status is $status"
        echo "DEBUG: /proc/mounts content:"
        cat /proc/mounts
    fi
    
    [ "$status" -eq 0 ]
    
    # Проверка вывода (опционально)
    # [[ "$output" == *"Unmounted"* ]]
    
    # Verify unmount success
    [ ! -f "$TMP_ENV/mnt_img/file.txt" ]
}

@test "Logic: Unmount by image file path (Image Path) (Directory removal)" {
    mkdir -p "$TMP_ENV/mnt_img_rmdir"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_img_rmdir"
    
    # Test: pass path to .sqfs file
    run $ZKS_SQM_BIN umount "$GOLDEN_ARCHIVE"
    [ "$status" -eq 0 ]
    
    # Verify the directory WAS REMOVED
    [ ! -d "$TMP_ENV/mnt_img_rmdir" ]
}

@test "Logic: Unmount by image file path (Image Path) (Directory preservation)" {
    mkdir -p "$TMP_ENV/mnt_img_keepdir"
    touch "$TMP_ENV/mnt_img_keepdir/file.txt"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_img_keepdir"
    [ -d "$TMP_ENV/mnt_img_keepdir" ]
    
    # Actual test
    run $ZKS_SQM_BIN umount "$GOLDEN_ARCHIVE"
    [ "$status" -eq 0 ]
    
    # Verify the directory WAS NOT REMOVED
    [ -d "$TMP_ENV/mnt_img_keepdir" ]

    # Verify that files still exist inside the directory
    [ -f "$TMP_ENV/mnt_img_keepdir/file.txt" ]

    # Очистка
    rm -rf "$TMP_ENV/mnt_img_keepdir"
}


@test "Logic: Unmount multiple mount points" {
    mkdir -p "$TMP_ENV/mnt_multi_1"
    mkdir -p "$TMP_ENV/mnt_multi_2"
    
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_multi_1"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_multi_2"
    
    # Ensure both are mounted
    [ -f "$TMP_ENV/mnt_multi_1/file.txt" ]
    [ -f "$TMP_ENV/mnt_multi_2/file.txt" ]
    
    # Test: Unmount all instances of this image
    run $ZKS_SQM_BIN umount "$GOLDEN_ARCHIVE"
    [ "$status" -eq 0 ]
    
    # Verify BOTH were unmounted
    [ ! -f "$TMP_ENV/mnt_multi_1/file.txt" ]
    [ ! -f "$TMP_ENV/mnt_multi_2/file.txt" ]
}

@test "Error: Attempt to unmount non-existent path" {
    run $ZKS_SQM_BIN umount "$TMP_ENV/non_existent_path"
    [ "$status" -ne 0 ]
}

@test "Error: Attempt to unmount an image that is not mounted" {
    # Create a dummy image that is definitely not mounted
    touch "$TMP_ENV/unused.sqfs"
    
    run $ZKS_SQM_BIN umount "$TMP_ENV/unused.sqfs"
    [ "$status" -ne 0 ]
    # Expect a message like "Image is not mounted" or "No mount points found"
}
