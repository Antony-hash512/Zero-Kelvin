#!/usr/bin/env bats

setup_file() {
    # Создаем "Эталонный архив" ОДИН РАЗ перед всеми тестами
    export TMP_ENV=$(mktemp -d -t zks-mount.XXXXXX)
    export GOLDEN_ARCHIVE="$TMP_ENV/golden.sqfs"
    
    mkdir -p "$TMP_ENV/src"
    echo "Hello" > "$TMP_ENV/src/file.txt"
    
    # Если это упадет — BATS скажет "Setup failed", и это правильно,
    # так как тестировать монтирование без архива невозможно.
    ./target/debug/squash_manager-rs create "$TMP_ENV/src" "$GOLDEN_ARCHIVE" --no-progress
}

teardown_file() {
    rm -rf "$TMP_ENV"
}

@test "Smoke: Монтирование архива" {
    mkdir -p "$TMP_ENV/mnt"
    run sudo ./target/debug/squash_manager-rs mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt"
    [ "$status" -eq 0 ]
    
    # Сразу размонтируем, чтобы не портить следующий тест
    sudo ./target/debug/squash_manager-rs umount "$TMP_ENV/mnt"
}

@test "Logic: Проверка файлов внутри" {
    mkdir -p "$TMP_ENV/mnt2"
    sudo ./target/debug/squash_manager-rs mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt2"
    
    run cat "$TMP_ENV/mnt2/file.txt"
    [ "$output" = "Hello" ]
    
    sudo ./target/debug/squash_manager-rs umount "$TMP_ENV/mnt2"
}
