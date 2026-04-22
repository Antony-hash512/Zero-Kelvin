# zks-manifest

### Флаги команды
Данные флаги и команды должны быть портированы на Rust c обеспечением полной обратной совместимости (но с некоторыми улучшениями, от которых будет сказано позже в соотвущем разделе).:

```
fireice@katana ~/src/rust/Zero-Kelvin (dev) 
🦀🐟 squash_manager --help
Usage: squash_manager create [OPTIONS] <input_path> [output_path]
       squash_manager mount <image> <mount_point>
       squash_manager umount <mount_point>

Options for 'create':
  -e, --encrypt         Create an encrypted LUKS container
  -c, --compression=N   Zstd compression level (default: 15)
  --no-progress         Disable progress bar

Description:
  Converts a directory OR an archive (tar.zst, zip, 7z, etc.) into SquashFS.
  With -e, it creates a LUKS container and streams data without cleartext on disk.
fireice@katana ~/src/rust/Zero-Kelvin (dev) 
🦀🐟 zero-kelvin-store --help
Usage: zero-kelvin-store (zks) <command> [options]

Commands:
  freeze [targets...] [archive_path]    Offload data to a SquashFS archive
                                        If [archive_path] is a directory, prompts for filename

  unfreeze <archive_path>               Restore data from an archive

  check <archive_path>                  Verify archive integrity vs live system

Freeze Options:
  -e, --encrypt                         Encrypt the archive using LUKS (via squash_manager)
  -r, --read <file>                     Read list of targets from a file

Check Options:
  --use-cmp                             Verify file content (byte-by-byte) in addition to size
  --delete                        Delete local files if they match the archive (Destructive!)
Examples:
  zks freeze /home/user/project /mnt/nas/data/backup.sqfs
  zks freeze -e /secret/data /mnt/nas/data/secret.sqfs_luks.img
  zks unfreeze /mnt/nas/data/backup.sqfs

```
чтобы не было путаницы с уже существующими в системе командами, бинарники портированной версии должны называться 0k и 0k-core

### Структура создаваемых файлов
 - файл, созданный оригинальной функцией zero-kelvin-store.fish должен открываться портом 0k
 - файл, созданный  портом 0k должен открываться оригинальной функцией zero-kelvin-store.fish
 - тоже самое касается и вспомогательной утилиты squash_manager

#### Структура дерева файлов и катологов внутри создаваемых squashfs-образов
Sqfs-образ, который создаётся при помощи zks/0k, должен иметь следующую структуру дерева файлов и каталогов:
```
.
├── list.yaml
└── to_restore
    ├── 1
    │   └── etc
    │        ├── classic
    │        ├── geth
    │        └── keystore
    ├── 2    
    │   └── eth-vanil
    │        ├── geth
    │        └── keystore
    ├── 3
    │   └── ethw
    │        ├── geth
    │        └── keystore
    └── 4
        └── mnr
            ├── bitmonero.log
            ├── lmdb
            ├── p2pstate.bin
            ├── rpc_ssl.crt
            └── rpc_ssl.key
```

list.yaml, to_restore, 1, 2, 3, 4 и т.д. -- фиксированные имена, все остальные для примера.

#### Формат yaml-файла
```
metadata:
  date: "Tue Jan 27 08:09:58 PM +04 2026"
  host: "katana"
  privilege_mode: "user" # или "root"
files:
  - id: 1
    name: "etc"
    restore_path: "/home/user/data/chains"
    type: directory # или file
  - id: 2
    name: "eth-vanil"
    restore_path:  "/home/user/data/chains"
    type: directory
  - id: 3
    name: "ethw"
    restore_path: "/home/user/data/chains"
    type: directory
  - id: 4
    name: "mnr"
    restore_path: "/home/user/data/chains"
    type: directory
```

#### Legacy-формат
Данный формат должен также корректно обрабатываться коммандами unfreeze и check, но командой freeze он не создаётся
```
metadata:
  date: "Tue Jan 27 08:09:58 PM +04 2026"
  host: "katana"
files:
  - id: 1
    original_path: "/home/share/data/extra/chains/etc"
    type: directory # или file
  - id: 2
    original_path: "/home/share/data/extra/chains/eth-vanil"
    type: directory
  - id: 3
    original_path: "/home/share/data/extra/chains/ethw"
    type: directory
  - id: 4
    original_path: "/home/share/data/extra/chains/mnr"
    type: directory

```

```
.
├── list.yaml
└── to_restore
    ├── 1
    │   ├── classic
    │   ├── geth
    │   └── keystore
    ├── 2
    │   ├── geth
    │   └── keystore
    ├── 3
    │   ├── geth
    │   └── keystore
    └── 4
        ├── bitmonero.log
        ├── lmdb
        ├── p2pstate.bin
        ├── rpc_ssl.crt
        └── rpc_ssl.key

```

