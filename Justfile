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

