#!/usr/bin/env fish

# --- ШАГ 0: Парсинг аргументов ---
argparse 'build' 'no-build' 'build-release' -- $argv
or exit 1

if set -q _flag_build_release
    set TEST_TARGET "release"
else
    set TEST_TARGET "debug"
end

# --- ШАГ 1: Вычисляем абсолютные пути ---
# Получаем папку, где лежит ЭТОТ скрипт (run_shell_tests.fish)
set -l script_dir (dirname (status filename))

# Вычисляем корень проекта (абсолютный путь)
# realpath уберет все ".." и симлинки
set -x ZKS_PROJECT_ROOT (realpath $script_dir)

# Сразу вычисляем путь к бинарнику, чтобы не дублировать логику в bats
set -x ZKS_SQM_BIN "$ZKS_PROJECT_ROOT/target/$TEST_TARGET/squash_manager-rs"
set -x ZKS_BIN "$ZKS_PROJECT_ROOT/target/$TEST_TARGET/zsk-rs"

echo "Project Root: $ZKS_PROJECT_ROOT"
echo "Binary Path:  $ZKS_BIN"
echo "Binary Path:  $ZKS_SQM_BIN"

# --- ШАГ 2: Сборка ---
set -l build_choice

if set -q _flag_build_release
    set build_choice "y"
else if set -q _flag_build
    set build_choice "y"
else if set -q _flag_no_build
    set build_choice "n"
else
    read -P "Do you want to build/rebuild the project? (y/N) " -l build_choice
end

if string match -qi "y" "$build_choice"
    if set -q _flag_build_release
        cargo build --release --locked
    else
        cargo build --locked
    end
    if test $status -ne 0
        echo "Build failed!"
        exit 1
    end
end

# --- ШАГ 3: Запуск тестов ---

# Тест 1 (без sudo) - переменные ZKS_* передадутся автоматически благодаря 'set -x'
bats tests/01_create.bats

# Тест 2 (C SUDO) - ВНИМАНИЕ!
# Sudo по умолчанию стирает переменные окружения ради безопасности.
# Нужно явно прокинуть их внутрь.
#sudo --preserve-env=ZKS_PROJECT_ROOT,ZKS_BIN bats tests/02_mount.bats
