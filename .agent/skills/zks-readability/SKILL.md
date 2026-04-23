---
Name: "zks-readability"
Description: "Стандарты читабельности кода: борьба с глубокой вложенностью, ранние возвраты, декомпозиция функций, устранение дублирования."
user-invocation: true
---
# zks-readability

### Проблема: Глубокая вложенность и монолитные файлы
Текущая кодовая база страдает от нескольких связанных проблем:
- **`0k-core.rs`** — 2368 строк в одном файле (из которых ~570 — тесты). Функция `run()` охватывает **~1100 строк** с match-ветками `Create` (~680 строк!), `Mount` (~190 строк), `Umount` (~250 строк). Это «God Function».
- **`engine.rs`** — 1766 строк. Функции `check`, `restore_from_mount`, `freeze` имеют глубокую вложенность.
- Повторяющийся паттерн `root_cmd.clone()` → сборка `Vec<String>` → `remove(0)` → `iter().map(|s| s.as_str()).collect()` встречается **минимум 10 раз** в `0k-core.rs`.
- 170 вызовов `to_string()`/`.clone()` только в `0k-core.rs`, многие из которых — ненужные промежуточные аллокации.

Перед добавлением нового функционала или при изменении существующего, обязательно применяй следующие правила повышения читабельности.

---

### Правило 1: Ранний возврат (Early Return / Guard Clauses)
Вместо того чтобы оборачивать основную логику в `if condition { ... }`, инвертируй условие и возвращай результат (или ошибку) как можно раньше.

**❌ Плохо:**
```rust
if path.exists() {
    if let Ok(file) = fs::File::open(path) {
        // ... 50 строк логики ...
    }
}
```

**✅ Хорошо:**
```rust
if !path.exists() {
    return Ok(()); // или Err(...)
}
let file = fs::File::open(path)?;
// ... логика без лишних отступов ...
```

---

### Правило 2: Декомпозиция функций (Function Extraction)
* Функции не должны превышать разумных пределов (идеально 50-100 строк).
* **Тело сложного цикла** (`for` / `while`), содержащее вложенные `if`/`match`, должно быть вынесено в отдельную вспомогательную функцию.
* **Интеграция с TDD:** При декомпозиции сложной функции на мелкие, не забывай покрывать новые вспомогательные функции Unit-тестами (Red-Green-Refactor).
* **Context Structs и Zero-Copy:** Группируй передаваемые аргументы в структуры, чтобы не передавать по 10 параметров. **ВНИМАНИЕ:** Проект нацелен на максимальную производительность (Zero-Copy). При создании таких структур обязательно используй ссылки и лайфтаймы (например, `struct Context<'a> { executor: &'a E, path: &'a Path }`), чтобы не плодить лишнее клонирование и выделение памяти (`.clone()`, `.to_string()`, `PathBuf`).

**Конкретные кандидаты на декомпозицию в проекте:**

| Файл | Функция/Блок | Строки | Проблема |
|---|---|---|---|
| `0k-core.rs` | `run()` → match `Commands::Create` | ~680 | «God match arm». Разбить на `handle_create()`, внутри — `create_encrypted()`, `create_plain()`, `repack_archive()` |
| `0k-core.rs` | `run()` → match `Commands::Mount` | ~190 | Разбить на `handle_mount()` → `mount_luks()`, `mount_plain()` |
| `0k-core.rs` | `run()` → match `Commands::Umount` | ~250 | Разбить на `handle_umount()` → `find_squashfuse_mounts()`, `find_luks_mounts()`, `umount_target()` |
| `0k-core.rs` | LUKS trim logic (lines 1077-1163) | ~90 | Вынести в `trim_luks_container()` |
| `engine.rs` | `check_item()` | ~145 | 10 мутабельных аргументов-счётчиков → заменить на `struct CheckStats` |
| `engine.rs` | `restore_from_mount()` | ~260 | Тело `for entry` цикла → вынести в `restore_single_entry()` |

---

### Правило 3: Использование оператора `?`
Избегай явного `match` для обработки ошибок, если ошибку всё равно нужно пробросить наверх. Используй `map_err` + `?`.

**❌ Плохо:**
```rust
let f = match fs::File::open(&manifest_path) {
    Ok(file) => file,
    Err(e) => return Err(ZkError::IoError(e)),
};
```

**✅ Хорошо:**
```rust
let f = fs::File::open(&manifest_path).map_err(ZkError::IoError)?;
```

---

### Правило 4: Уплощение через `and_then` и `let-else`
Используй современные возможности Rust (например, конструкцию `let else` из Rust 1.65+) для плоской обработки `Option` и `Result`.

**❌ Плохо:**
```rust
if let Some(parent) = path.parent() {
    if let Some(name) = parent.file_name() {
        println!("{}", name.to_string_lossy());
    }
}
```

**✅ Хорошо:**
```rust
let Some(parent) = path.parent() else { return };
let Some(name) = parent.file_name() else { return };
println!("{}", name.to_string_lossy());
```

---

### Правило 5: Лимит вложенности (Arrow Anti-Pattern)
**Максимально допустимый уровень вложенности: 3.** (например: `Функция` -> `Цикл` -> `If`).
Если твой код требует 4-го или 5-го уровня вложенности (16-20 пробелов отступа), ты **ОБЯЗАН** прерваться и вынести внутренний блок в отдельную функцию или применить Guard Clause.

---

### Правило 6: Устранение повторяющихся паттернов (DRY)

В текущем коде есть повторяющийся паттерн вызова команд через `root_cmd`:

**❌ Плохо (повторяется 10+ раз в `0k-core.rs`):**
```rust
let mut args = root_cmd.clone();
args.extend(vec!["cryptsetup".to_string(), "close".to_string(), mapper_name.clone()]);
let prog = args.remove(0);
let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
let output = executor.run(&prog, &refs)?;
```

**✅ Хорошо — вынести в хелпер:**
```rust
fn run_with_root<E: CommandExecutor>(
    executor: &E,
    root_cmd: &[String],
    program: &str,
    args: &[&str],
) -> std::io::Result<Output> {
    let mut full_args: Vec<&str> = root_cmd.iter().skip(1).map(|s| s.as_str()).collect();
    full_args.push(program);
    full_args.extend(args);
    let prog = if root_cmd.is_empty() { program } else { &root_cmd[0] };
    executor.run(prog, &full_args)
}
```

Аналогично для `run_interactive_with_root`. Это исключит десятки строк бойлерплейта и устранит класс ошибок, связанных с `remove(0)` на пустом `Vec`.

---

### Правило 7: Разделение монолитных файлов

Файл `0k-core.rs` (2368 строк) — это **бинарник**, а не библиотека. Вся бизнес-логика `create`, `mount`, `umount` живёт прямо в `main()` → `run()`. Это нарушает принцип разделения ответственности и делает код практически невозможным для повторного использования или юнит-тестирования без полного mock-стека.

**Рекомендуемая стратегия:**
1. Вынести логику команд в `src/` (например, `src/core_create.rs`, `src/core_mount.rs`, `src/core_umount.rs`).
2. В `src/bin/0k-core.rs` оставить только: CLI-парсинг → signal handlers → вызов функций из `src/`.
3. Хелперы (`CompressionMode`, `LuksTransaction`, `CreateTransaction`, `generate_mapper_name`, `open_luks_container`, `get_effective_root_cmd`) перенести в `src/` (например, `src/luks.rs`, `src/compression.rs`).

Это позволит:
- Тестировать `create_encrypted()` без mock'а всего CLI-стека.
- Переиспользовать `LuksTransaction` и `open_luks_container` из `0k` (верхнеуровневая утилита).
- Уменьшить файл `0k-core.rs` с 2368 до ~200-300 строк.

---

### Правило 8: Автоматизация и проверки (Clippy)
Чтобы не гадать, насколько функция сложная, используй встроенные линтеры.
Ты можешь запустить `cargo clippy`, чтобы автоматически найти слишком сложные функции:

```bash
cargo clippy -- -W clippy::cognitive_complexity -W clippy::too_many_arguments
```
* **Cognitive Complexity:** Clippy предупредит, если функция слишком сложна для понимания.
* **Too Many Arguments:** Укажет, где нужно применить Context Structs.
