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
    #cargo check --locked || cargo update
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

# --- Переносимые статические сборки (musl) ---

# Собрать полностью статические бинарники под x86_64-unknown-linux-musl
build-musl:
    cargo build --profile release-static --locked --target x86_64-unknown-linux-musl

# Проверить, что все три бинарника статически слинкованы (без glibc / PT_INTERP / NEEDED)
verify-musl:
    #!/usr/bin/env bash
    set -euo pipefail
    BIN_DIR="target/x86_64-unknown-linux-musl/release-static"
    for bin in 0k 0k-core 0k-safe-rm; do
        path="$BIN_DIR/$bin"
        if [ ! -x "$path" ]; then
            echo "ERROR: Missing binary: $path (run 'just build-musl' first)" >&2
            exit 1
        fi
        file "$path"
        if readelf -l "$path" | grep -q 'INTERP'; then
            echo "ERROR: $path has a dynamic PT_INTERP header!" >&2
            exit 1
        fi
        if readelf -d "$path" 2>/dev/null | grep -q '(NEEDED)'; then
            echo "ERROR: $path has dynamic shared library dependencies!" >&2
            readelf -d "$path" | grep '(NEEDED)' >&2
            exit 1
        fi
        echo "OK: $bin is fully statically linked ($(du -h "$path" | cut -f1))"
    done

# Прогнать unit-тесты и интеграционные BATS-тесты на musl-сборке (без root)
test-musl: build-musl verify-musl
    cargo test --locked --target x86_64-unknown-linux-musl --features testing
    fish tests/run_shell_tests.fish --musl --no-root

# Прогнать unit-тесты и ВСЕ интеграционные BATS-тесты (включая LUKS с интерактивным sudo) на musl-сборке
test-musl-root: build-musl verify-musl
    cargo test --locked --target x86_64-unknown-linux-musl --features testing
    fish tests/run_shell_tests.fish --musl --with-root

# Упаковать переносимый релизный архив (.tar.gz + SHA256SUMS) в dist/
dist-musl: build-musl verify-musl
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
    ARCH="x86_64-unknown-linux-musl"
    PKG_NAME="zero-kelvin-v${VERSION}-${ARCH}"
    BIN_DIR="target/${ARCH}/release-static"
    STAGE_DIR="dist/${PKG_NAME}"

    rm -rf "$STAGE_DIR" "dist/${PKG_NAME}.tar.gz"
    mkdir -p "$STAGE_DIR/bin" "$STAGE_DIR/share/man/man1" "$STAGE_DIR/share/doc/zero-kelvin" "$STAGE_DIR/share/licenses/zero-kelvin"

    install -m755 "$BIN_DIR/0k"         "$STAGE_DIR/bin/0k"
    install -m755 "$BIN_DIR/0k-core"    "$STAGE_DIR/bin/0k-core"
    install -m755 "$BIN_DIR/0k-safe-rm" "$STAGE_DIR/bin/0k-safe-rm"

    ln -s 0k         "$STAGE_DIR/bin/zero-kelvin"
    ln -s 0k-core    "$STAGE_DIR/bin/zero-kelvin-core"
    ln -s 0k-safe-rm "$STAGE_DIR/bin/zero-kelvin-safe-rm"

    install -m644 man/0k.1      "$STAGE_DIR/share/man/man1/0k.1"
    install -m644 man/0k-core.1 "$STAGE_DIR/share/man/man1/0k-core.1"
    ln -s 0k.1      "$STAGE_DIR/share/man/man1/zero-kelvin.1"
    ln -s 0k-core.1 "$STAGE_DIR/share/man/man1/zero-kelvin-core.1"

    install -m644 README.md     "$STAGE_DIR/share/doc/zero-kelvin/README.md"
    install -m644 LICENSE.GPLv3 "$STAGE_DIR/share/licenses/zero-kelvin/LICENSE.GPLv3"
    install -m644 LICENSE.docs  "$STAGE_DIR/share/licenses/zero-kelvin/LICENSE.docs"

    tar -C dist -czf "dist/${PKG_NAME}.tar.gz" "$PKG_NAME"
    (cd dist && sha256sum "${PKG_NAME}.tar.gz" > "${PKG_NAME}.tar.gz.sha256")
    rm -rf "$STAGE_DIR"
    echo "Created portable release archive: dist/${PKG_NAME}.tar.gz"
    cat "dist/${PKG_NAME}.tar.gz.sha256"
