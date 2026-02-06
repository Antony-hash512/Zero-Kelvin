#!/usr/bin/env fish

# --- ШАГ 0: Парсинг аргументов ---
argparse 'build' 'no-build' 'build-release' 'no-build-release' 'no-root' -- $argv
or exit 1

if set -q _flag_build_release; or set -q _flag_no_build_release
    set TEST_TARGET "release"
else
    set TEST_TARGET "debug"
end

# ... (skip to ShAG 2.5) ...

# --- ШАГ 2.5: Определение прав Root для тестов ---
# По умолчанию пропускаем
set -x SKIP_ROOT "1"
set -x ROOT_CMD ""

if set -q _flag_build_release; or set -q _flag_no_build_release
    echo "Release mode: Root tests disabled for safety."
else if set -q _flag_no_root
    echo "Flag --no-root detected: Root tests DISABLED."
else if test (id -u) -eq 0
    echo "Running as Root: Root tests ENABLED."
    set -x SKIP_ROOT "0"
    set -x ROOT_CMD ""
else
    # Пробуем sudo без пароля
    if sudo -n true 2>/dev/null
        echo "Sudo (nopasswd) available: Root tests ENABLED."
        set -x SKIP_ROOT "0"
        set -x ROOT_CMD "sudo"
    else
        echo "Root/Sudo not available: Root tests DISABLED."
    end
end

# --- ШАГ 1: Вычисляем абсолютные пути ---
# Получаем папку, где лежит ЭТОТ скрипт (run_shell_tests.fish)
set -l script_dir (dirname (status filename))

# Вычисляем корень проекта (абсолютный путь)
# realpath уберет все ".." и симлинки
set -x ZKS_PROJECT_ROOT (realpath $script_dir/..)

# Сразу вычисляем путь к бинарнику, чтобы не дублировать логику в bats
set -x ZKS_SQM_BIN "$ZKS_PROJECT_ROOT/target/$TEST_TARGET/squash_manager-rs"
set -x ZKS_BIN "$ZKS_PROJECT_ROOT/target/$TEST_TARGET/zks-rs"

echo "Project Root: $ZKS_PROJECT_ROOT"
echo "Binary Path:  $ZKS_BIN"
echo "Binary Path:  $ZKS_SQM_BIN"

# --- ШАГ 2: Сборка ---
set -l build_choice

if set -q _flag_build_release
    set build_choice "y"
else if set -q _flag_build
    set build_choice "y"
else if set -q _flag_no_build; or set -q _flag_no_build_release
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

function run_colored_bats
    bats $argv --formatter pretty | sed -u -e "s/✓/✅ 👍 🤩/" -e "s/✗/❌ 👎 😭/"

    # В fish массив $pipestatus хранит коды выхода всех команд пайпа.
    # $pipestatus[1] — это код выхода bats.
    # Если bats упал (код != 0), мы тоже возвращаем ошибку.
    if test $pipestatus[1] -ne 0
        return 1
    end
end



# Тест 0 (Help)
run_colored_bats tests/00_help.bats

# Тест 1 (без sudo) - переменные ZKS_* передадутся автоматически благодаря 'set -x'
and run_colored_bats tests/01_create.bats

# Тест 2
# Sudo больше не нужен т.к. под капотом squash_manager-rs должен использоваться squashfuse
# вместо системных утилит mount/umount, в отличие от них он не требует root прав
and run_colored_bats tests/02_mount.bats

and run_colored_bats tests/03_umount.bats

and run_colored_bats tests/04_unpack.bats

and run_colored_bats tests/05_luks.bats

and run_colored_bats tests/06_freeze.bats

and run_colored_bats tests/07_cleaning.bats

and run_colored_bats tests/08_unfreeze.bats

and run_colored_bats tests/09_check.bats

and run_colored_bats tests/10_privilege.bats

and run_colored_bats tests/11_fullcycle.bats

and run_colored_bats tests/12-custom-errors.bats

and run_colored_bats tests/13-luks-privilege.bats

