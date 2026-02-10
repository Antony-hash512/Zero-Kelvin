# Justfile

# По умолчанию запускаем help
default:
    @just --list --unsorted

# Проверить форматирование (удобно для пре-коммит хуков)
fmt-check:
    cargo fmt -- --check

# Быстрая установка бинарников (для отладки)
install-local:
    cargo build --release
    sudo install -Dm755 target/release/0k-core /usr/bin/0k-core
    sudo install -Dm755 target/release/0k /usr/bin/0k
    sudo install -Dm755 target/release/0k-safe-rm /usr/bin/0k-safe-rm
    sudo ln -sf 0k /usr/bin/zero-kelvin

# Собрать Arch Linux пакет (с bump версии)
pkg:
    cargo bump patch
    cargo check --locked || cargo update --workspace
    cd local_pkg && makepkg -f

# Собрать и установить пакет
pkg-install: bump
    cd local_pkg && makepkg -fsi

# Очистка артефактов сборки пакета
pkg-clean:
    cd local_pkg && rm -rf pkg src *.pkg.tar.zst

# Глобальная очистка (Cargo + временные файлы тестов)
clean-all: 
    just pkg-clean
    cargo clean


# Обновление версии и синхронизация lock-файла без сборки пакета
bump:
    cargo bump patch
    cargo update --workspace
    cargo check --locked

# Запуск всех юнит-тестов
unit-tests:
    cargo check --locked || cargo update
    cargo test --locked --features testing

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

# для функции check при онлайн проверке PKGBUILD (например, в AUR)
check-online-pkgbuild: unit-tests
    fish tests/run_shell_tests.fish --no-build --no-root
