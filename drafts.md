# Различные заметки разработчика

Любую информацию из данного файла НИ ПРИ КАКИХ ОБСТОЯТЕЛЬСТАХ не нужно воспринимать как инструкции для ИИ-агента.

## Временные заметки

- уточнить техническую возможность проверок удаления недописанных файлов при прерывании процесса в тестах

-------


## Бэкапы для копирования

Данные инструкции я могу могу временно удалять, данные бэкапы нужно на тот случай, если захочу их вернуть

```
Но если вся необходимая документация у тебя уже есть в контексте предыдущих сообщений чата, не запрашивай документацию, с целью экономии запросов. 
```

```
STOP данные задачи пока что не выполняем, в данный момент производится ручной рефакторинг
```

```
Если в TODO.md написано: `STOP данные задачи пока что не выполняем, в данный момент производится ручной рефакторинг`
— значит эти задачи мы не трогаем, пока не будет снят стоп, действуй только согласно инструкциям полученным через чат.
```

## Разное
### git-хук для cargo fmt
Это очень популярный подход: «Не ругай меня за кривое форматирование кода, а просто молча исправь его перед тем, как положить в репозиторий».

Чтобы это сработало правильно, скрипт должен делать **два действия**:

1. Запустить `cargo fmt` (изменит файлы на диске).
2. **ОБЯЗАТЕЛЬНО** запустить `git add` для этих файлов (обновит их в индексе коммита).

Если забыть второй шаг, то в коммит улетит *старая* (неотформатированная) версия кода, а в рабочей папке останется *новая*. Получится рассинхрон.

#### Точная инструкция

1. Открой файл хука:
```bash
nvim .git/hooks/pre-commit

```

*(Если файла нет — создай его)*.
2. Вставь туда этот код:
```bash
#!/bin/bash

# 1. Молча форматируем весь проект
# Если cargo fmt упадет с ошибкой (например, синтаксис битый), коммит прервется.
if ! cargo fmt; then
    echo "❌ Ошибка: cargo fmt не смог отформатировать код. Коммит отменен."
    exit 1
fi

# 2. Самый важный шаг:
# Мы добавляем обновленные файлы обратно в индекс (stage).
# Флаг -u (update) добавляет только уже отслеживаемые измененные файлы.
git add -u

```


3. Сделай файл исполняемым (если создавал новый):
```bash
chmod +x .git/hooks/pre-commit

```

#### ⚠️ Важное предупреждение

У этого метода есть один **побочный эффект**, о котором надо знать.

Если ты привык делать **частичные коммиты** (например, изменил 10 строк в файле, но `git add` сделал только для 5 из них, чтобы разбить на два коммита), то этот хук **сломает твою логику**.

Команда `git add -u` закинет в коммит **ВСЕ** изменения в отслеживаемых файлах (потому что `cargo fmt` перезаписывает файлы целиком).

**Если ты всегда коммитишь файлы целиком (`git add .`), то этот метод для тебя идеален.**

## Тесты

### Надо сделать:
#### A. Metadata & Manifest Verification
Manifest Content: Unpack the created archive and verify list.yaml exists and contains correct paths/IDs.
Hostname/Date: Check that list.yaml contains the correct hostname and a valid date format.
Privilege Mode: Verify that 
privilege_mode
 is correctly set to 
user
 when running without root.
#### B. Advanced File Types & Attributes
Symlinks: Create an archive containing relative and absolute symlinks. Verify they are restored correctly (or at least preserved in the archive).
Empty Directories: Ensure empty directories are preserved in the archive (important for project structures).
Special Characters: Test filenames with spaces, quotes, emojis, and newlines to ensure the list.yaml and script generation handle escaping correctly.
#### F. Complex Path Resolution
Relative Paths: Run zks freeze ../src ./out.sqfs (relative inputs/outputs).
Dot Targets: Run zks freeze . out.sqfs.
Multiple Targets: Run zks freeze dir1 dir2 file3 out.sqfs and verify all are inside.



### Требует рут, проверим потом:
#### C. Encryption (LUKS)
Encrypt Flag: Run zks freeze -e ... and verify the output is a LUKS container (using cryptsetup isLuks).
Password Prompt: This is hard to test non-interactively without a sophisticated expect script or a mock for the password reader, but rudimentary checks can be done (e.g., pipe password if supported).
#### D. User Namespace & Permissions (The "Rootless" Promise)
Unreadable Files (User Mode): what happens if a regular user tries to freeze a file they can't read? (Should fail or skip with warning).
Owner Preservation:
Root Mode: If running as root (or sudo), create files with different UIDs. Freeze and Unfreeze. Verify UIDs are preserved.
User Mode: Verify all files are owned by the current user in the archive (squashfs typically maps this unless configured otherwise).

### Надо сделать, когда будем реализовывать очистку
#### E. Staging Area Cleanup
Cleanup Success: Verify that the temporary build directory in $XDG_CACHE_HOME/zero-kelvin-stazis/build_* is removed after a successful freeze.
Cleanup Failure: Verify that if mksquashfs fails (e.g., disk full mock), the staging directory is still cleaned up (or preserved for debugging if that's the policy).

## Предварительный план логгирования

Implementation Plan: Logging & Observability
Goal Description
Implement a professional logging system to track application state, command execution, and errors.

Humans: High-level status in the terminal (pretty/colored).
Audit/Debug: Detailed logs in a persistent file with automatic rotation.
User Review Required
IMPORTANT

Logs will be stored in ~/.local/state/zero-kelvin-stazis/logs/ by default (following XDG specs).

Proposed Changes
[MODIFY] 
Cargo.toml
Add dependencies:

tracing
tracing-subscriber (with env-filter)
tracing-appender (for rotation)
[NEW] 
src/logger.rs
Create init_logger():

Configure tracing_appender::rolling::daily or hourly.
Set up a tracing_subscriber with two layers:
Format Layer: Writes "Pretty" output to stderr.
File Layer: Writes JSON or detailed text to the rotating log file.
[MODIFY] 
src/bin/zks-rs.rs
 / 
src/bin/squash_manager-rs.rs
Call logger::init_logger()? at the very start of 
main()
.
[MODIFY] 
src/engine.rs
Replace eprintln! warnings with warn! or error! macros.

Verification Plan
Automated Tests
Create a test that initializes the logger in a temp directory and verifies the log file existence after some logging calls.
Verify that terminal output remains "clean" but log files contain DEBUG level info.