#!/usr/bin/env bats

setup_file() {
    # Создаем "Эталонный архив" ОДИН РАЗ перед всеми тестами
    export TMP_ENV=$(mktemp -d -t zks-mount.XXXXXX)
    export GOLDEN_ARCHIVE="$TMP_ENV/golden.sqfs"
    
    mkdir -p "$TMP_ENV/src"
    echo "Hello" > "$TMP_ENV/src/file.txt"
    
    # Если это упадет — BATS скажет "Setup failed", и это правильно,
    # так как тестировать монтирование без архива невозможно.
    $ZKS_SQM_BIN create "$TMP_ENV/src" "$GOLDEN_ARCHIVE" --no-progress
}

teardown_file() {
    rm -rf "$TMP_ENV"
}
@test "Smoke: Проверка статуса" {
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_smoke"
    [ "$status" -eq 0 ]
    
    # Сразу размонтируем, чтобы не портить следующий тест
    $ZKS_SQM_BIN umount "$TMP_ENV/mnt_smoke"
}


@test "Logic: Монтирование архива, автоматическое создание каталога монтирования" {
    run sudo $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt"
    [ -d "$TMP_ENV/mnt" ]
    # Сразу размонтируем, чтобы не портить следующий тест
    $ZKS_SQM_BIN umount "$TMP_ENV/mnt"
}



@test "Logic: Проверка файлов внутри" {
    # Тут каталог создаём для раздельного тестирования возможных ошибок
    mkdir -p "$TMP_ENV/mnt2"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt2"
    
    run cat "$TMP_ENV/mnt2/file.txt"
    [ "$output" = "Hello" ]
    
    $ZKS_SQM_BIN umount "$TMP_ENV/mnt2"
}

