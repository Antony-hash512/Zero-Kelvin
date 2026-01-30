# Justfile

# По умолчанию запускаем help
default:
    @just --list

# Запуск всех интеграционных тестов
shell-tests:
    fish tests/run_shell_tests.fish --no-build

# (Ре)билд + Запуск всех интеграционных тестов
build-and-shell-tests:
    fish tests/run_shell_tests.fish --build

