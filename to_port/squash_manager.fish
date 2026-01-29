function squash_manager --description "Smartly manage SquashFS: create (optional encryption), mount, and umount"
    # Аргументы для создания/шифрования
    argparse 'c/compression=!_validate_int' 'no-progress' 'e/encrypt' 'h/help' -- $argv
    or return 1

    set -l action $argv[1]
    set -l root_cmd (functions -q get_root_cmd; and get_root_cmd; or echo "sudo")

    # --- СПРАВКА ---
    if set -q _flag_help; or test (count $argv) -eq 0
        echo "Usage: squash_manager create [OPTIONS] <input_path> [output_path]"
        echo "       squash_manager mount <image> <mount_point>"
        echo "       squash_manager umount <mount_point>"
        echo ""
        echo "Options for 'create':"
        echo "  -e, --encrypt         Create an encrypted LUKS container"
        echo "  -c, --compression=N   Zstd compression level (default: 15)"
        echo "  --no-progress         Disable progress bar"
        echo ""
        echo "Description:"
        echo "  Converts a directory OR an archive (tar.zst, zip, 7z, etc.) into SquashFS."
        echo "  With -e, it creates a LUKS container and streams data without cleartext on disk."
        return 0
    end

    switch "$action"
        case mount
            set -l img $argv[2]
            set -l mnt $argv[3]
            if test -z "$img"; or test -z "$mnt"
                echo "Error: Usage: squash_manager mount <image> <mount_point>"
                return 1
            end

            set -l mapper_name "sq_"(string replace -a (string escape --style=regex ".") "_" (basename $img))
            mkdir -p $mnt

            if $root_cmd cryptsetup isLuks $img 2>/dev/null
                # Check for existing mapper
                if test -e /dev/mapper/$mapper_name
                    echo "Mapper device exists. Attempting to mount..."
                    if $root_cmd mount -t squashfs /dev/mapper/$mapper_name $mnt 2>/dev/null
                        echo "Mounted at $mnt"
                        return 0
                    else
                        echo "Mount failed (Stale mapper?). Closing and retrying..."
                        $root_cmd cryptsetup close $mapper_name
                    end
                end

                echo "Opening encrypted container..."
                if not $root_cmd cryptsetup open $img $mapper_name
                    echo "Error: Failed to open encrypted container."
                    return 1
                end

                if $root_cmd mount -t squashfs /dev/mapper/$mapper_name $mnt
                    echo "Mounted at $mnt"
                    return 0
                else
                    echo "Error: Mount failed."
                    $root_cmd cryptsetup close $mapper_name
                    return 1
                end
            else
                echo "Mounting standard SquashFS..."
                if $root_cmd mount -t squashfs -o loop $img $mnt
                    echo "Mounted at $mnt"
                    return 0
                else
                    echo "Error: Mount failed."
                    return 1
                end
            end


        case umount
            set -l mnt $argv[2]
            if test -z "$mnt"
                echo "Error: Usage: squash_manager umount <mount_point>"
                return 1
            end

            set -l dev ($root_cmd findmnt -n -o SOURCE $mnt)
            echo "Unmounting $mnt..."
            $root_cmd umount $mnt

            if string match -q "/dev/mapper/sq_*" "$dev"
                set -l mapper_name (basename $dev)
                echo "Closing LUKS container $mapper_name..."
                $root_cmd cryptsetup close $mapper_name
            end

            rmdir $mnt 2>/dev/null
            echo "Done."
            return 0

        case create
            set -l input_path $argv[2]
            set -l output_path $argv[3]
            if test -z "$input_path"
                echo "Error: Input path required."
                return 1
            end

            if test -z "$output_path"
                set -l clean_name (string trim -r -c / $input_path)
                set clean_name (string replace -r '\.(tar\.zst|tar\.gz|tgz|tar\.xz|txz|tar\.bz2|tbz|tar|7z|zip|rar)$' '' $clean_name)
                set output_path "$clean_name.squashfs"
            end

            set -l comp_level (set -q _flag_compression; and echo $_flag_compression; or echo 15)

            # Определение декомпрессора
            set -l decompress_cmd
            if not test -d $input_path
                switch $input_path
                    case '*.tar.zst' '*.tzst'; set decompress_cmd zstd -dcf
                    case '*.tar.gz' '*.tgz'; set decompress_cmd gzip -dcf
                    case '*.tar.xz' '*.txz'; set decompress_cmd xz -dcf
                    case '*.tar.bz2' '*.tbz'; set decompress_cmd bzip2 -dcf
                    case '*.tar'; set decompress_cmd cat
                    case '*.7z' '*.zip' '*.rar' '*.iso'
                        type -q bsdtar; or begin; echo "Error: bsdtar required"; return 1; end
                        set decompress_cmd bsdtar -c -f - --format=tar "@-"
                    case '*'; echo "Error: Unknown format"; return 1; end
            end

            set -l tmp_map "sq_v_"(random)

            # Trap для очистки при прерывании (Ctrl+C)
            trap "
                # Если это шифрованный контейнер и маппер существует - закрываем
                if test -e /dev/mapper/$tmp_map
                    echo 'Aborted. Closing container...'
                    $root_cmd cryptsetup close $tmp_map 2>/dev/null
                end
                
                # Если файл был создан (зашифрованный или обычный) - удаляем
                if test -f \"$output_path\"
                    echo 'Removing incomplete file...'
                    rm \"$output_path\" 2>/dev/null
                end
                exit 1
            " INT TERM

            if set -q _flag_encrypt
                # --- ЛОГИКА С ШИФРОВАНИЕМ ---
                set -l raw_size (test -d $input_path; and du -sb $input_path | cut -f1; or stat -c %s $input_path)
                set -l container_size (math -s0 "$raw_size / 1024 / 1024 + ($raw_size / 1024 / 1024 / 10) + 32")

                # Проверка места
                set -l free_space (df -m . | tail -1 | awk '{print $4}')
                if test $container_size -gt $free_space
                    echo "Error: Not enough space. Need $container_size MB, but only $free_space MB available."
                    return 1
                end


                echo "Preparing encrypted stream ($container_size MB)..."
                dd if=/dev/zero of="$output_path" bs=1M count=$container_size status=progress 2>/dev/null

                # Форматирование LUKS
                if not $root_cmd cryptsetup luksFormat "$output_path"
                    echo "Operation aborted."
                    rm "$output_path"
                    return 1
                end

                # Открытие контейнера
                if $root_cmd cryptsetup open "$output_path" $tmp_map
                    
                    echo "Packing data (Zstd $comp_level)... This may take a while."
                    
                    if test -d $input_path
                        # Пишем напрямую в mapper устройство (блок-девайс)
                        # Это предотвращает "destination not block device" и проблемы с пайпами
                        set -l mk_opts -comp zstd -Xcompression-level $comp_level -b 1M -no-recovery -noappend
                        set -q _flag_no_progress; and set mk_opts $mk_opts -quiet; or set mk_opts $mk_opts -info
                        
                        $root_cmd mksquashfs "$input_path" /dev/mapper/$tmp_map $mk_opts
                    else
                        # Для архивов через tar2sqfs
                        # tar2sqfs умеет писать в блок-девайс
                        set -l source_cmd (type -q pv; and not set -q _flag_no_progress; and echo "pv \"$input_path\""; or echo "cat \"$input_path\"")
                        fish -c "$source_cmd | $decompress_cmd | $root_cmd tar2sqfs -c zstd -X level=$comp_level -b 1M --force -o /dev/mapper/$tmp_map"
                    end
                    set -l sq_status $status

                    # Если создание squashfs прошло успешно, вычисляем размер для обрезки
                    set -l trim_size ""
                    if test $sq_status -eq 0
                        if type -q unsquashfs
                             # Capture output (combine stdout and stderr)
                             set -l sq_info ($root_cmd unsquashfs -s /dev/mapper/$tmp_map 2>&1)
                             
                             # Extract bytes directly using regex with flexible spacing
                             set -l matches (echo "$sq_info" | string match -r "Filesystem size\s+([0-9]+)\s+bytes")
                             set -l fs_size_bytes $matches[2]
                             
                             # Extract Payload Offset (Handle LUKS2 bytes and LUKS1 sectors)
                             set -l luks_dump ($root_cmd cryptsetup luksDump $output_path)
                             set -l offset_bytes ""
                             
                             # Try LUKS2 format: "offset: 16777216 [bytes]"
                             set -l luks2_matches (echo "$luks_dump" | string match -r "offset:\s+([0-9]+)\s+\[bytes\]")
                             if test -n "$luks2_matches[2]"
                                 set offset_bytes $luks2_matches[2]
                             else
                                 # Try LUKS1 format: "Payload offset: 4096" (sectors)
                                 set -l luks1_matches (echo "$luks_dump" | string match -r "Payload offset:\s+([0-9]+)")
                                 if test -n "$luks1_matches[2]"
                                     set offset_bytes (math "$luks1_matches[2] * 512")
                                 end
                             end
                             
                             if test -n "$fs_size_bytes"; and test -n "$offset_bytes"
                                 # Calculate raw size: FS + Header + 1MB buffer
                                 set -l raw_trim_size (math "ceil($fs_size_bytes) + $offset_bytes + 1048576")
                                 # Align to 4096 bytes (sector size safety) to avoid "device size is not multiple of sector size"
                                 set trim_size (math "ceil($raw_trim_size / 4096) * 4096")
                             else
                                 echo "Warning: Could not determine optimal size. Skipping trim."
                                 if test -z "$fs_size_bytes"; echo "Minning fs_size_bytes. unsquashfs raw: $sq_info"; end
                                 if test -z "$offset_bytes"; echo "Missing payload offset. luksDump raw: $luks_dump"; end
                             end
                        else
                             echo "Warning: unsquashfs not found, cannot optimize size."
                        end
                    end

                    $root_cmd cryptsetup close $tmp_map
                    
                    # Если создание squashfs упало, удаляем файл
                    if test $sq_status -ne 0
                        echo "Error during packing. cleaning up..."
                        rm "$output_path"
                        return 1
                    end
                    
                    # Обрезаем лишнее место
                    if test -n "$trim_size"
                        set -l current_size (stat -c %s $output_path)
                        if test $trim_size -lt $current_size
                             echo "Optimizing container size: $(math -s1 $current_size/1024/1024)MB -> $(math -s1 $trim_size/1024/1024)MB"
                             truncate -s (math -s0 $trim_size) "$output_path"
                        end
                    end

                    trap - INT TERM
                else
                    echo "Failed to open container."
                    rm "$output_path"
                    return 1
                end
            else
                # --- ОБЫЧНОЕ СОЗДАНИЕ ---
                if test -d $input_path
                     set -l mk_opts -comp zstd -Xcompression-level $comp_level -b 1M -no-recovery
                     set -q _flag_no_progress; and set mk_opts $mk_opts -quiet; or set mk_opts $mk_opts -info
                     mksquashfs "$input_path" "$output_path" $mk_opts
                 else
                    set -l source_cmd (type -q pv; and not set -q _flag_no_progress; and echo "pv \"$input_path\""; or echo "cat \"$input_path\"")
                    fish -c "$source_cmd | $decompress_cmd | tar2sqfs -c zstd -X level=$comp_level -b 1M --force -o \"$output_path\""
                end
            end

            if test $status -eq 0
                set_color green; echo "Success: $output_path"; set_color normal
                ls -lh $output_path
            else
                return 1
            end

        case '*'
            echo "Error: Unknown command '$action'. Available: create, mount, umount."
            return 1
    end
end
