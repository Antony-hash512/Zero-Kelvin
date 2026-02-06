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

setup() {
    # Создаем уникальную подпапку для каждого теста внутри глобального TMP_ENV
    # чтобы teardown мог почистить только то, что относится к текущему тесту
    export TEST_MNT_ROOT="$TMP_ENV/mnt_$(date +%s)_$RANDOM"
    mkdir -p "$TEST_MNT_ROOT"
}

teardown() {
    # Агрессивно пытаемся размонтировать всё внутри TMP_ENV
    # Это гарантирует, что даже если тест упал, дескрипторы закроются
    find "$TMP_ENV" -maxdepth 2 -type d -exec fusermount -u {} 2>/dev/null \; || true
}
@test "Smoke: Проверка статуса" {
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_smoke"
    [ "$status" -eq 0 ]
    
    # Сразу размонтируем, чтобы не портить следующий тест
    $ZKS_SQM_BIN umount "$TMP_ENV/mnt_smoke"
}


@test "Logic: Монтирование архива, автоматическое создание каталога монтирования" {
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt"
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

@test "Logic: Монтирование архива, автоматическое создание каталога монтирования (Auto-Gen)" {
    # 1. запускает: `squash_manadger-rs mount <sqfs-образ>` без указания каталога
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE"
    
    # 2. Проверяем успех
    [ "$status" -eq 0 ]
    
    # 3. Проверяем сообщение в output
    # <No mount point specified. Using secure local path for stability: <имя каталога>
    # Используем partial matching, так как путь полный
    [[ "$output" == *"No mount point specified. Using secure local path for stability:"* ]]
    
    # 4. Парсим "хвост"
    local generated_path=$(echo "$output" | grep "Using secure local path for stability:" | awk -F': ' '{print $2}' | tr -d '[:space:]')
    
    # 5. Проверяем формат имени: mount_<префикс>_<unix-время>_<случайное_число, 6 цифр>
    # Префикс = golden.sqfs
    local dirname=$(basename "$generated_path")
    local prefix="mount_golden.sqfs"
    
    # Проверка префикса
    [[ "$dirname" == "$prefix"* ]]
    
    # Проверка структуры (regex)
    regex="^${prefix}_[0-9]+_[0-9]{6}$"
    [[ "$dirname" =~ $regex ]]
    
    # 6. Проверяем что каталог создался
    [ -d "$generated_path" ]
    
    # 7. Проверяем содержимое
    [ -f "$generated_path/file.txt" ]
    run cat "$generated_path/file.txt"
    [ "$output" = "Hello" ]
    
    # Cleanup
    $ZKS_SQM_BIN umount "$generated_path"
    rmdir "$generated_path" 2>/dev/null || true
}

@test "Error: Файл образа не существует" {
    run $ZKS_SQM_BIN mount "non_existent_file.sqfs"
    [ "$status" -ne 0 ]
    
    # Check for specific error message
    # "No such file or directory" or "does not exist"
    # Case insensitive grep
    echo "$output" | grep -i -E "no such file|does not exist"
}

@test "Logic: Collision handling (Коллизия имен)" {
    # 1. Первый запуск
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE"
    [ "$status" -eq 0 ]
    local path1=$(echo "$output" | grep "Using secure local path for stability:" | awk -F': ' '{print $2}' | tr -d '[:space:]')
    
    # 2. Второй запуск сразу же
    run $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE"
    [ "$status" -eq 0 ]
    local path2=$(echo "$output" | grep "Using secure local path for stability:" | awk -F': ' '{print $2}' | tr -d '[:space:]')
    
    # 3. Проверяем что пути разные
    [ "$path1" != "$path2" ]
    
    # 4. Проверяем что оба существуют и работают
    [ -d "$path1" ]
    [ -d "$path2" ]
    
    # Cleanup
    $ZKS_SQM_BIN umount "$path1"
    $ZKS_SQM_BIN umount "$path2"
    rmdir "$path1" 2>/dev/null || true
    rmdir "$path2" 2>/dev/null || true
}
