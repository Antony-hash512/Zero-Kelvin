#!/usr/bin/env bats

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
    run ./target/debug/squash_manager-rs create "$SRC" "$TEST_DIR/out.sqfs" --no-progress
    [ "$status" -eq 0 ]
    [ -f "$TEST_DIR/out.sqfs" ]
}

@test "Logic: Ошибка при отсутствии входной папки" {
    run ./target/debug/squash_manager-rs create "/bad/path" "$TEST_DIR/out.sqfs"
    [ "$status" -ne 0 ]
}

@test "Logic: Флаг сжатия принимается" {
    run ./target/debug/squash_manager-rs create "$SRC" "$TEST_DIR/comp.sqfs" -c 1 --no-progress
    [ "$status" -eq 0 ]
}
