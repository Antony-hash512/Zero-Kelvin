# Justfile

# По умолчанию запускаем help
default:
    @just --list --unsorted

# Запуск вообще всех тестов
test-all:
    just unit-tests
    just build-and-shell-tests

# Запуск вообще всех тестов без root
test-all-noroot:
    just unit-tests
    just build-and-shell-tests-noroot

# Проверить форматирование (удобно для пре-коммит хуков)
fmt-check:
    cargo fmt -- --check

# Быстрая установка бинарников (для отладки)
install-local:
    cargo build --release
    sudo install -Dm755 target/release/squash_manager-rs /usr/bin/squash_manager-rs
    sudo install -Dm755 target/release/zks-rs /usr/bin/zks-rs

# Собрать Arch Linux пакет (с bump версии)
pkg:
    cargo bump patch
    cargo check --locked || cargo update --workspace
    cd local_pkg && makepkg -f

# Собрать и установить пакет
pkg-install:
    cargo bump patch
    cargo check --locked || cargo update --workspace
    just pkg-clean
    cd local_pkg && makepkg -fsi

# Очистка артефактов сборки пакета
pkg-clean:
    cd local_pkg && rm -rf pkg src *.pkg.tar.zst

# Обновление версии и проверка зависимостей без сборки пакета
bump:
    cargo bump patch
    cargo check --locked || cargo update --workspace


# Запуск всех юнит-тестов
unit-tests:
    cargo check --locked || cargo update
    cargo test --locked

# Запуск всех интеграционных шелл-тестов
shell-tests:
    fish tests/run_shell_tests.fish --no-build

# Запуск всех интеграционных шелл-тестов без root
shell-tests-noroot:
    fish tests/run_shell_tests.fish --no-build --no-root

# (Ре)билд + Запуск всех интеграционных шелл-тестов
build-and-shell-tests:
    fish tests/run_shell_tests.fish --build

# (Ре)билд + Запуск всех интеграционных шелл-тестов без root
build-and-shell-tests-noroot:
    fish tests/run_shell_tests.fish --build --no-root

# (Ре)билд в режиме релиза + Запуск всех интеграционных шелл-тестов
build-and-shell-tests-release:
    fish tests/run_shell_tests.fish --build-release --no-root

