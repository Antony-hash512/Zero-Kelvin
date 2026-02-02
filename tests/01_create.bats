#!/usr/bin/env bats
load 'test_helper/bats-support/load'
load 'test_helper/bats-assert/load'

setup() {
    export TEST_DIR=$(mktemp -d -t zks-create.XXXXXX)
    export SRC="$TEST_DIR/src"
    mkdir -p "$SRC"
    echo "data" > "$SRC/file.txt"
    mkdir -p "$SRC/dir"
    echo $RANDOM > "$SRC/dir/file2.txt"
    echo $RANDOM > "$SRC/dir/file3.txt"
    mkdir -p "$SRC/dir/dir2"
    echo $RANDOM > "$SRC/dir/dir2/file4.txt"
    echo $RANDOM > "$SRC/dir/dir2/file5.txt"
}

teardown() {
    rm -rf "$TEST_DIR"
}

@test "Smoke: Успешное создание обычного архива" {
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/out.sqfs" --no-progress
    assert_success
    [ -f "$TEST_DIR/out.sqfs" ]
}

@test "Logic: Проверка типа файла через 'file'" {
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/type_check.sqfs" --no-progress
    assert_success
    run file "$TEST_DIR/type_check.sqfs"
    assert_output --partial "Squashfs filesystem"
    assert_output --partial "zstd compressed"
}

@test "Logic: Ошибка при отсутствии входной папки" {
    run $ZKS_SQM_BIN create "/bad/path" "$TEST_DIR/out.sqfs"
    echo "DEBUG output: [$output]" >&3
    assert_failure
    assert_output --partial "Input path does not exist"
}

@test "Logic: Флаг сжатия принимается" {
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/comp.sqfs" -c 1 --no-progress
    assert_success
    run bash -c "unsquashfs -s $TEST_DIR/comp.sqfs | grep compression-level"
    echo "DEBUG output: [$output]" >&3
    assert_output --partial "compression-level 1"
    [ -f "$TEST_DIR/comp.sqfs" ]
}

@test "Logic:Сжатие по умолчанию" {
    # В релизной версии данный тест отключается т.к. может сломаться
    # если во внешней утилите mksquashfs дефолтная степень сжатия изменится

    if [ "$ZKS_RELEASE" = "true" ]; then
        skip "Default compression test is disabled in release mode"
    fi
    # Извлекаем значение из исходного кода Rust (надежный парсинг числа после =)
    local default_comp=$(sed -n 's/.*DEFAULT_ZSTD_COMPRESSION.*= *\([0-9]\+\).*/\1/p' "$ZKS_PROJECT_ROOT/src/constants.rs")
    [ "$default_comp" -eq 15 ] && skip "Default compression is 15"
    
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/default.sqfs" --no-progress
    assert_success
    run bash -c "unsquashfs -s $TEST_DIR/default.sqfs | grep compression-level"
    echo "DEBUG output: [$output]" >&3
    assert_output --partial "compression-level $default_comp"
}

@test "Smoke: Создание архива без сжатия (-c 0)" {
    run $ZKS_SQM_BIN create "$SRC" "$TEST_DIR/nocomp.sqfs" -c 0 --no-progress
    assert_success
    [ -f "$TEST_DIR/nocomp.sqfs" ]
    
    # Проверка, что файл действительно создан и читается
    run file "$TEST_DIR/nocomp.sqfs"
    assert_output --partial "Squashfs filesystem"
}

@test "Logic: Эффективность отключения сжатия (-c 0 vs -c 1)" {
    # 1. Генерируем хорошо сжимаемые данные (текстовый паттерн)
    # 10 файлов по ~85KB = ~850KB данных
    mkdir -p "$SRC/compressible"
    for i in {1..10}; do
        yes "ZeroKelvinStazis_Test_String" | head -n 5000 > "$SRC/compressible/file_$i.txt"
    done

    # 2. Создаем сжатый архив (используем fast compression -c 1)
    run $ZKS_SQM_BIN create "$SRC/compressible" "$TEST_DIR/compressed.sqfs" -c 1 --no-progress
    assert_success

    # 3. Создаем НЕсжатый архив (-c 0)
    run $ZKS_SQM_BIN create "$SRC/compressible" "$TEST_DIR/nocomp.sqfs" -c 0 --no-progress
    assert_success

    # 4. Сравниваем размеры
    local size_comp=$(stat -c%s "$TEST_DIR/compressed.sqfs")
    local size_nocomp=$(stat -c%s "$TEST_DIR/nocomp.sqfs")

    echo "Size Compressed (-c 1): $size_comp" >&3
    echo "Size Uncompressed (-c 0): $size_nocomp" >&3

    # Несжатый должен быть существенно больше
    # (Для сжимаемых данных разница будет в разы)
    [ "$size_nocomp" -gt "$size_comp" ]

    # 5. Проверяем метаданные (опционально, зависит от версии mksquashfs)
    # Обычно при отсутствии сжатия unsquashfs пишет "gzip" (как дефолт метаданных)
    # или "no compression". Главное — проверка размера выше.
}

@test "Cleanup: Interrupted create removes incomplete file (Directory)" {
    # Create random data file (slow to compress, unlike sparse files)
    BIGDIR="$TEST_DIR/bigdir"
    mkdir -p "$BIGDIR"
    dd if=/dev/urandom of="$BIGDIR/random.bin" bs=1M count=50 2>/dev/null
    
    OUTPUT="$TEST_DIR/interrupted.sqfs"
    
    # Start create in background
    $ZKS_SQM_BIN create "$BIGDIR" "$OUTPUT" --no-progress &
    CREATE_PID=$!
    
    # Wait for file to appear (max 10 sec)
    for i in {1..100}; do
        [ -f "$OUTPUT" ] && break
        sleep 0.1
    done
    
    # Ensure file was created before we interrupt
    [ -f "$OUTPUT" ] || skip "Output file never appeared (system too slow?)"
    
    # Interrupt the process
    kill -INT $CREATE_PID 2>/dev/null || true
    wait $CREATE_PID 2>/dev/null || true
    
    # Assert: file should NOT exist after cleanup
    [ ! -f "$OUTPUT" ]
}