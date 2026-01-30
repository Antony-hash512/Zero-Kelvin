# Justfile

# По умолчанию запускаем help
default:
    @just --list --unsorted

unit-tests:
    cargo test --locked

# Запуск всех интеграционных тестов
shell-tests:
    fish tests/run_shell_tests.fish --no-build

# (Ре)билд + Запуск всех интеграционных тестов
build-and-shell-tests:
    fish tests/run_shell_tests.fish --build

build-and-shell-tests-release:
    fish tests/run_shell_tests.fish --build-release

test-all:
    just unit-tests
    just build-and-shell-tests

