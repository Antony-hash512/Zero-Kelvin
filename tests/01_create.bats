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
    run file "$TEST_DIR/comp.sqfs"
    echo "DEBUG output: [$output]" >&3
    [ -f "$TEST_DIR/comp.sqfs" ]
}
