# 🧊 Zero-Kelvin Stazis (Rust)

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE.GPLv3)
[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/) 

/ [English](#english) / [Русский](#русский) /

---

## English

- **Zero-Kelvin Stazis (zks-rs)** is a high-performance utility for data "conservation" that ensures full preservation of integrity, file attributes (permissions, ownership, timestamps), hierarchy, and location relative to the filesystem root.
A Rust port of the `zero-kelvin-store` function set (originally written for Fish shell), the utility packs projects into compressed, mountable **SquashFS** images. It supports optional transparent encryption via standard **LUKS** (`cryptsetup`).
- **Primary Goal:** To free up disk space while maintaining instant read-only access without the need for decompression, with the ability to extract individual files or the entire archive.

---

### 🚀 Key Features

- **Offloading (Data Evacuation):** The primary goal is to transfer data to "cold" storage with safe (auto-verified) deletion of original files to free up space on the workstation.
- **Rootless Freeze:** Uses User Namespaces (`unshare`) to archive sensitive files without requiring privilege escalation whenever possible.
   > **Note on Encryption:** Using `-e`/`--encrypt` is the operation that strictly mandates `root`. This is because `cryptsetup` must create mappings in the kernel's Device Mapper to manage encrypted block devices, which is a privileged operation unavailable to standard users.
- **Instant Access:** Archives are SquashFS images. Mount them instantly to browse files without lengthy waiting for full extraction.
- **Zero-Knowledge Privacy:** Supports LUKS-encrypted containers for creating secure "black box" archives.
- **Contextual Sets:** Bundle logically related parts for offloading, spread across various file system paths, into a single atomic archive.
- **Prescriptive Restore:** Restoration based on an internal manifest: the final archives created by the utility remember where files were originally located; all paths are recorded in a small internal YAML service file within the archive.

---

### FAQ:

Q: Does it support deduplication of different backup versions?
A: The purpose of this utility is to "freeze" and move data that is not currently needed to external storage. For periodic backups of data you are actively working with, I recommend other ready-made solutions:
- If your data is mainly text-based rather than binary, you can use Git. It can also be used in conjunction with this utility for offloading not just directories, but directories containing Git repositories.
- If you have many binary files, Git may not be suitable. In that case, I recommend:
  - Snapshots in file systems like Btrfs or ZFS (an excellent solution for quick access to older versions of your data, even in binary format, but this solution is not ideal for freeing up space by offloading to an external HDD/NAS).
  - If your file system (e.g., ext4) does not support snapshots, or you need not only deduplication of different versions but also the ability to free up space, use ready-made backup solutions such as Borg, Restic, Kopia. These utilities are better suited for use as a "time machine."

Q: Why do I need this utility if Borg, Restic, Kopia exist?
A: The advantage of this utility over these solutions is: One file versus a folder with small files. Borg/Restic create repositories of thousands of small files (chunks). Moving them (copying to/from external storage/network storage) is more difficult than moving one monolithic .sqfs image file: not a bunch of small files in a complex database, but 1 autonomous, portable, self-sufficient file. The SquashFS format is a Linux standard: to restore or view data, you do not need the `zks-rs` utility itself; standard system tools are sufficient (the utility simply makes it easier and faster). The image mounts instantly, without long indexing. To open a Borg or Restic repository in 10 years, you absolutely need the `borg` or `restic` program installed (and if their format suddenly changes, then "we're in trouble..."). To open a SquashFS archive, you just need Linux (the format is supported by the kernel).

---

### 🛠 Installation

Currently in development. To build from source:

```bash
cargo build --release
```

The build produces two main binaries:
- `zks-rs`: The primary high-level orchestrator.
- `squash_manager-rs`: Low-level tool for SquashFS and LUKS management.

---

### 📖 Usage

#### Freeze (Archive/Offload)
Move logically grouped paths into a "frozen" state.

```bash
# Basic freeze
zks-rs freeze ~/projects/old-work /mnt/nas/archives/old-work.sqfs

# Encrypted freeze
zks-rs freeze --encrypt ~/secret-data /mnt/nas/archives/secure.sqfs_luks.img

# Freeze multiple targets
zks-rs freeze /etc/nginx/sites-available /var/www/html /mnt/nas/backup/web-server.sqfs
```

#### Unfreeze (Restore)
Return data to its original location instantly.

```bash
zks-rs unfreeze /mnt/nas/archives/old-work.sqfs
```

#### Check (Verify)
Compare an archive against the live system.

```bash
# Verify integrity
zks-rs check /mnt/nas/archives/old-work.sqfs

# Verify and safely delete local files that match the archive (Offloading)
zks-rs check --force-delete /mnt/nas/archives/old-work.sqfs
```

---

### 🔧 Core Philosophy

1. **Low Friction:** Restoration should be as simple as `Ctrl+Z`.
2. **KISS & Native:** Uses standard Linux tools (`rsync`, `mksquashfs`, `zstd`, `cryptsetup`) so your data is never locked into a proprietary format.
3. **User-Space Friendly:** Prefers FUSE (`squashfuse`) and namespaces over root privileges for regular mounts.


---

### 📜 License

This project is licensed under the **GPLv3 License**. See [LICENSE.GPLv3]([LICENSE.GPLv3]) for details.

---

## Русский

### 🧊 Zero-Kelvin Stazis (Rust)


- **Zero-Kelvin Stazis (zks-rs)** — это высокопроизводительная утилита для «консервации» данных с полным сохранением их целостности, атрибутов (прав доступа, владельцев, временных меток), иерархии и распольожения относительно корня файловой системы.
Являясь портом на Rust набора функций `zero-kelvin-store` (изначально написанных для Fish shell), утилита упаковывает проекты в сжатые, монтируемые образы **SquashFS**. Поддерживается опциональное прозрачное шифрование через стандартный **LUKS** (`cryptsetup`).
- **Главная цель**: освобождение дискового пространства при сохранении мгновенного доступа к данным без необходимости распаковки (read-only) и вожможностью распакавать к любые файлы как по отдельности так и весь архив целиком.

---

### 🚀 Ключевые особенности

- **Выгрузка (Оффлоадинг):** Основная цель утилиты: перенос данных в «холодное» хранилище с безопасным (автосверка) удалением оригиналов файлов для освобожения места на рабочей станции.
- **Безрутовая заморозка:** Использует User Namespaces (`unshare`) для архивации файлов без необходимости повышения привилегий, когда это возможно.
   > **Примечание о шифровании:** Использование флага `-e`/`--encrypt` — операция, которая требует прав `root`. Это связано с тем, что `cryptsetup` должен создавать маппинги в Device Mapper ядра для управления зашифрованными блочными устройствами, что является привилегированной операцией, недоступной обычным пользователям.
- **Мгновенный доступ:** Архивы представляют собой образы SquashFS. Их можно мгновенно примонтировать и просматривать файлы без длительного ожидания полной распаковки.
- **Приватность в Zero-Knowledge-style:** Поддержка LUKS-шифрованных контейнеров для создания защищенных архивов («черных ящиков»).
- **Сборка воедино:** Объединение логически связанных частей для оффлоада, разбросанных по разным путям файловой системы, в единый атомарный архив.
- **Умное восстановление:** Восстановление на основе внутреннего манифеста: конечные архивы, создаваемые утилитой сами помнят где ранее находились файлы: все пути прописаны в небольшом служебном yaml-файле внутри архива.

---
### ЧаВо:
В: поддерживается ли дедупликация разных версий бекапов
О: Цель данной утилиты это заморзка с перемещением во внешнее хранение тех данных которые в данный момент не нужны. Для переодических бекапов данных и с которыми вы сейчас работаете, я рекомендую другие уже готовые решения:
- Если ваши данные в основном текстовые, а не бинарные, то вы можете использовать git. Его также можно использовать совместно с данной утилитой для оффлоад-выгрузки не просто каталогов, а каталогов с git-репозиториями.
- Если много бинарных файлов, git подойдёт не очень, тогда рекомендую:
  - Снапшоты в таких файловых системах как btrfs или ZFS (отличное решение для быстрого доступа к старым версиях ваших данных даже в бинарном формате, но данное решение не очень подходит для освобождения места в системе путём выгрузки на внешний HDD/NAS)
  - Если ваша файловая система (например ext4) не поддерживает снапшоты или вам нужна не только дедупликая различных версий, но и возможность освободить место, используйте готовые решения для бекапов, такие как: Borg, Restic, Kopia. Именно эти утилты лучше подходят для использования в качестве "машины времени".
  
В: Зачем нужна эта утилита, если есть Borg, Restic, Kopia.
О: Преимущество же данной утилиты перед, этими решенинями: Один файл против папки с мелкими файлами: Borg/Restic создают репозитории из тысяч мелких файлов (чанков). Перемещать их (копировать с/на внешний носитель/сетевое хранилище) сложнее, чем один монолитный файл образа .sqfs: не кучу мелких файлов сложной бд, а 1 автономный, переносимый, самодостаточный файл. Формат SquashFS является стандартом Linux: для восстановления или просмотра данных не требуется наличие самой утилиты zks, а достаточно стандартных системных инструментов (утилита всего-лишь позволяет делать это проще и быстрее). Образ монтируется мгновенно, без долгого индексирования. Чтобы открыть репозиторий Borg или Restic через 10 лет, вам обязательно нужна установленная программа borg или restic (а если вдруг у них поменяется формат, то "вот, ну и приплыли..."). Чтобы открыть SquashFS-архив, нужен просто Linux (формат поддерживается ядром).

---

### 🛠 Установка

В настоящий момент находится в разработке. Для сборки из исходного кода:

```bash
cargo build --release
```

В результате сборки создаются два основных бинарных файла:
- `zks-rs`: Основной высокоуровневый оркестратор.
- `squash_manager-rs`: Низкоуровневый инструмент для управления SquashFS и LUKS.

---

### 📖 Использование

#### Freeze (Архивация/Заморозка)
Перемещение логически сгруппированных путей в «замороженное» состояние.

```bash
# Базовая заморозка
zks-rs freeze ~/projects/old-work /mnt/nas/archives/old-work.sqfs

# Шифрованная заморозка
zks-rs freeze --encrypt ~/secret-data /mnt/nas/archives/secure.sqfs_luks.img

# Заморозка нескольких целей
zks-rs freeze /etc/nginx/sites-available /var/www/html /mnt/nas/backup/web-server.sqfs
```

#### Unfreeze (Разморозка/Восстановление)
Мгновенный возврат данных на их исходные места.

```bash
zks-rs unfreeze /mnt/nas/archives/old-work.sqfs
```

#### Check (Проверка)
Сравнение архива с живой системой.

```bash
# Проверка целостности
zks-rs check /mnt/nas/archives/old-work.sqfs

# Проверка и безопасное удаление локальных файлов, совпадающих с архивом (Offloading)
zks-rs check --force-delete /mnt/nas/archives/old-work.sqfs
```

---

### 🔧 Философия ядра

1. **Low Friction (Минимум трения):** Процесс восстановления должен быть таким же простым, как нажатие `Ctrl+Z`.
2. **KISS & Native:** Используются стандартные инструменты Linux (`rsync`, `mksquashfs`, `zstd`, `cryptsetup`), поэтому ваши данные никогда не окажутся заперты в проприетарном формате.
3. **User-Space Friendly:** Предпочтение отдается FUSE (`squashfuse`) и пространствам имен (namespaces) вместо прав root для обычных монитирований.


---

### 📜 Юридические условия использования

Этот проект лицензирован под **GPLv3 License**. Текст условий использования: [LICENSE.GPLv3]([LICENSE.GPLv3]).
