# Justfile

# По умолчанию запускаем help
default:
    @just --list --unsorted

# Запуск всех юнит-тестов
unit-tests:
    cargo test --locked

# Запуск всех интеграционных шелл-тестов
shell-tests:
    fish tests/run_shell_tests.fish --no-build

# (Ре)билд + Запуск всех интеграционных шелл-тестов
build-and-shell-tests:
    fish tests/run_shell_tests.fish --build

# (Ре)билд в режиме релиза + Запуск всех интеграционных шелл-тестов
build-and-shell-tests-release:
    fish tests/run_shell_tests.fish --build-release

# Запуск вообще всех тестов
test-all:
    just unit-tests
    just build-and-shell-tests

