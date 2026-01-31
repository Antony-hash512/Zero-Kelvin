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

@test "Smoke: Размонтирование по явному пути" {
    mkdir -p "$TMP_ENV/mnt_smoke"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_smoke"
    [ -d "$TMP_ENV/mnt_smoke" ]
    
    # Непосредственно тест
    run $ZKS_SQM_BIN umount "$TMP_ENV/mnt_smoke"
    [ "$status" -eq 0 ]
    
    # Проверка, что больше не смонтировано (например, файл внутри недоступен или папка пуста/удалена)
    # squashfuse usually leaves empty dir.
    # Check if file exists inside (should NOT)
    [ ! -f "$TMP_ENV/mnt_smoke/file.txt" ]
    
    # В Linux можно также проверить через `mount` или `/proc/mounts`, 
    # но косвенная проверка через доступность файлов тоже ок для Smoke.
}

@test "Logic: Размонтирование по пути к файлу образа (Image Path)" {
    mkdir -p "$TMP_ENV/mnt_img"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_img"
    
    # Тест: передаем путь к .sqfs файлу, а не к точке монтирования
    run $ZKS_SQM_BIN umount "$GOLDEN_ARCHIVE"
    
    [ "$status" -eq 0 ]
    
    # Проверка вывода (опционально)
    # [[ "$output" == *"Unmounted"* ]]
    
    # Проверка факта размонтирования
    [ ! -f "$TMP_ENV/mnt_img/file.txt" ]
}

@test "Logic: Размонтирование множественных точек (Multiple Mounts)" {
    mkdir -p "$TMP_ENV/mnt_multi_1"
    mkdir -p "$TMP_ENV/mnt_multi_2"
    
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_multi_1"
    $ZKS_SQM_BIN mount "$GOLDEN_ARCHIVE" "$TMP_ENV/mnt_multi_2"
    
    # Убеждаемся, что оба смонтированы
    [ -f "$TMP_ENV/mnt_multi_1/file.txt" ]
    [ -f "$TMP_ENV/mnt_multi_2/file.txt" ]
    
    # Тест: Размонтируем все вхождения этого образа
    run $ZKS_SQM_BIN umount "$GOLDEN_ARCHIVE"
    [ "$status" -eq 0 ]
    
    # Проверяем, что ОБА отмонтировались
    [ ! -f "$TMP_ENV/mnt_multi_1/file.txt" ]
    [ ! -f "$TMP_ENV/mnt_multi_2/file.txt" ]
}

@test "Error: Попытка размонтировать несуществующий путь" {
    run $ZKS_SQM_BIN umount "$TMP_ENV/non_existent_path"
    [ "$status" -ne 0 ]
}

@test "Error: Попытка размонтировать образ, который не смонтирован" {
    # Создаем фиктивный образ, который точно не смонтирован
    touch "$TMP_ENV/unused.sqfs"
    
    run $ZKS_SQM_BIN umount "$TMP_ENV/unused.sqfs"
    [ "$status" -ne 0 ]
    # Ожидаем сообщение типа "Image is not mounted" или "No mount points found"
}
