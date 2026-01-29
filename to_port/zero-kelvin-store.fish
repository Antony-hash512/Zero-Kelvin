function zero-kelvin-store --description "Zero-Kelvin Store: Freeze data to SquashFS and Unfreeze back"
    # Alias: zks
    
    function _zks_help
        echo "Usage: zero-kelvin-store (zks) <command> [options]"
        echo ""
        echo "Commands:"
        echo "  freeze [targets...] [archive_path]    Offload data to a SquashFS archive"
        echo "                                        If [archive_path] is a directory, prompts for filename"
        echo ""
        echo "  unfreeze <archive_path>               Restore data from an archive"
        echo ""
        echo "  check <archive_path>                  Verify archive integrity vs live system"
        echo ""
        echo "Freeze Options:"
        echo "  -e, --encrypt                         Encrypt the archive using LUKS (via squash_manager)"
        echo "  -r, --read <file>                     Read list of targets from a file"
        echo ""
        echo "Check Options:"
        echo "  --use-cmp                             Verify file content (byte-by-byte) in addition to size"
        echo "  --force-delete                        Delete local files if they match the archive (Destructive!)"
        echo ""
        echo "Examples:"
        echo "  zks freeze /home/user/project /mnt/nas/data/backup.sqfs"
        echo "  zks freeze -e /secret/data /mnt/nas/data/secret.sqfs_luks.img"
        echo "  zks unfreeze /mnt/nas/data/backup.sqfs"
        return 0
    end

    if test (count $argv) -eq 0
        or contains -- -h $argv
        or contains -- --help $argv
        _zks_help
        return 0
    end

    set -l command $argv[1]
    set -e argv[1]

    switch $command
        case freeze
            # --- FREEZE LOGIC ---
            argparse 'e/encrypt' 'r/read=' 'h/help' -- $argv
            or return 1

            if set -q _flag_help
                _zks_help
                return 0
            end

            set -l targets
            set -l output_archive

            # 1. Collect targets
            # From file
            if set -q _flag_read
                if test -f "$_flag_read"
                    # Read non-empty lines, ignoring comments if any
                    set targets $targets (cat "$_flag_read" | string trim | string match -r -v '^$')
                else
                    echo "Error: Target file '$_flag_read' not found."
                    return 1
                end
            end

            # From arguments (remaining argv)
            set -l args_count (count $argv)
            if test $args_count -lt 1
                echo "Error: Output archive path is required."
                return 1
            end

            set output_archive $argv[$args_count]
            
            # Handle if output is a directory (interactive filename generation)
            if test -d "$output_archive"
                set -l timestamp (date +%Y-%m-%d_%H%M%S)
                
                echo "Output is a directory: $output_archive"
                read -P "Enter filename prefix (default: zks): " -l user_prefix
                
                if test -z "$user_prefix"
                    set user_prefix "zks"
                end
                
                set -l new_name "$user_prefix"_"$timestamp".sqfs
                # Strip trailing slash if present then append filename
                set output_archive (string trim -r -c / -- "$output_archive")"/$new_name"
                echo "Saving to: $output_archive"
            end
            
            # If there are targets in argv, add them
            if test $args_count -gt 1
                set -l arg_targets $argv[1..-2]
                set targets $targets $arg_targets
            end

            if test (count $targets) -eq 0
                echo "Error: No targets specified for freezing."
                return 1
            end

            # Verify targets exist and convert to absolute paths
            set -l abs_targets
            for t in $targets
                if not test -e "$t"
                    echo "Error: Target '$t' does not exist."
                    return 1
                end
                set -a abs_targets (realpath "$t")
            end
            set targets $abs_targets

            # Check dependencies
            if not functions -q squash_manager
                echo "Error: 'squash_manager' function not found."
                return 1
            end
            if not functions -q rm-if-empty
                echo "Error: 'rm-if-empty' function not found."
                return 1
            end

            # Prepare for isolation
            # Use timestamp + random for uniqueness to avoid collisions
            set -l build_uuid (date +%s)"_"(random)
            # FIX 1: Generate build path in HOST to avoid race condition in cleanup
            set -l host_build_dir "/tmp/zks_build_$build_uuid"
            
            set -l target_list_file "/tmp/zks_targets_$build_uuid.txt"
            string join \n -- $targets > $target_list_file

            # FIX 3: Resolve absolute path
            set -l sq_man_path (realpath (functions --details squash_manager))
            
            # Export variables for the subshell
            set -lx ZKS_TARGET_LIST $target_list_file
            set -lx ZKS_OUTPUT (realpath -m $output_archive)
            set -lx ZKS_SQ_PATH $sq_man_path
            set -lx ZKS_ENCRYPT $_flag_encrypt
            set -lx ZKS_HOSTNAME (uname -n)
            set -lx ZKS_BUILD_DIR $host_build_dir

            echo "🧊 Freezing data..."

            # Execute functionality inside a new mount namespace
            # Passing variables explicitly because some sudo configs reject -E
            sudo \
                ZKS_TARGET_LIST="$ZKS_TARGET_LIST" \
                ZKS_OUTPUT="$ZKS_OUTPUT" \
                ZKS_SQ_PATH="$ZKS_SQ_PATH" \
                ZKS_ENCRYPT="$ZKS_ENCRYPT" \
                ZKS_HOSTNAME="$ZKS_HOSTNAME" \
                ZKS_BUILD_DIR="$ZKS_BUILD_DIR" \
                unshare -m --propagation private fish -c '
                # --- INSIDE NAMESPACE ---
                
                # 1. Load dependency
                source $ZKS_SQ_PATH

                # 2. Create Skeleton (Use specific path passed from host)
                mkdir -p $ZKS_BUILD_DIR
                set -l restore_root "$ZKS_BUILD_DIR/to_restore"
                mkdir -p $restore_root

                # 3. Create Manifest
                set -l manifest "$ZKS_BUILD_DIR/list.yaml"
                echo "metadata:" > $manifest
                echo "  date: \"$(date)\"" >> $manifest
                echo "  host: \"$ZKS_HOSTNAME\"" >> $manifest
                echo "files:" >> $manifest

                # 4. Bind Mount Targets
                set -l counter 1
                cat $ZKS_TARGET_LIST | while read -l target_path
                    if test -z "$target_path"; continue; end

                    set -l container_dir "$restore_root/$counter"
                    mkdir -p $container_dir

                    # Refactored Logic: Preserve basename in structure
                    set -l t_name (basename "$target_path")
                    set -l t_dir (dirname "$target_path")
                    
                    # Create the mount point inside the numeric dir
                    # e.g. /to_restore/1/etc
                    if test -d "$target_path"
                         mkdir "$container_dir/$t_name"
                         mount --bind "$target_path" "$container_dir/$t_name"
                    else
                         touch "$container_dir/$t_name"
                         mount --bind "$target_path" "$container_dir/$t_name"
                    end

                    echo "  - id: $counter" >> $manifest
                    echo "    name: \"$t_name\"" >> $manifest
                    echo "    restore_path: \"$t_dir\"" >> $manifest
                    
                    if test -d "$target_path"
                        echo "    type: directory" >> $manifest
                    else
                        echo "    type: file" >> $manifest
                    end
                    set counter (math $counter + 1)
                end

                # 5. Pack
                set -l enc_arg
                if test -n "$ZKS_ENCRYPT"
                    set enc_arg "--encrypt"
                end

                echo "📦 Packing to $ZKS_OUTPUT..."
                # Use $enc_arg unquoted so it expands to nothing if empty
                squash_manager create $enc_arg --no-progress "$ZKS_BUILD_DIR" "$ZKS_OUTPUT"
                
                if test $status -eq 0
                    exit 0
                else
                    exit 1
                end
            '
            set -l exit_code $status

            # Cleanup in host system
            rm -f $target_list_file
            
            # FIX 1: Cleanup specific dir
            if test -d "$host_build_dir"
                if test $exit_code -eq 0
                    # Fix permissions so user tools can work
                    sudo chown -R (id -un):(id -gn) "$host_build_dir"

                    if functions -q rm-if-empty
                        rm-if-empty "$host_build_dir/to_restore"
                    end
                    
                    rm -f "$host_build_dir/list.yaml"
                end
                
                # Check directly with rmdir to be safe (mkdir'ом logic from user likely meant rmdir)
                rmdir "$host_build_dir" 2>/dev/null
            end

            if test $exit_code -eq 0
                set_color green
                echo "✅ Archive created successfully: $output_archive"
                set_color normal
            else
                echo "❌ Failed to create archive."
                return 1
            end


        case unfreeze
            argparse 'h/help' -- $argv
            if set -q _flag_help
                _zks_help
                return 0
            end

            if test (count $argv) -lt 1
                echo "Error: Archive path required."
                return 1
            end
            set -l archive_path $argv[1]

            if not test -f "$archive_path"
                echo "Error: Archive '$archive_path' not found."
                return 1
            end

            # Need get_root_cmd for writing to protected dirs
            set -l root_cmd (functions -q get_root_cmd; and get_root_cmd; or echo "sudo")

            # Temporary mount point
            set -l mount_point "/tmp/zks_mnt_"(random)
            
            echo "🔓 Mounting archive..."
            squash_manager mount "$archive_path" "$mount_point"
            or return 1

            set -l manifest "$mount_point/list.yaml"
            if not test -f "$manifest"
                echo "Error: Invalid archive format (list.yaml not found)."
                squash_manager umount "$mount_point"
                return 1
            end

            echo "📖 Reading manifest..."
            
            # Parse manifest (Supports new 'name/restore_path' and legacy 'original_path')
            set -l ids
            set -l paths
            set -l names
            set -l restore_paths
            set -l types
            set -l is_legacy 0
            
            set -l current_id ""
            cat $manifest | while read -l line
                if string match -q "*id: *" -- $line
                    set current_id (string replace -r ".*id: " "" -- $line)
                    set -a ids $current_id
                    # placeholders for safety
                    set -a names ""
                    set -a restore_paths ""
                    set -a paths ""
                else if string match -q "*original_path: *" -- $line
                     # Legacy support
                     set is_legacy 1
                     if test -n "$current_id"
                        set -l p (string replace -r ".*original_path: \"" "" -- $line | string replace -r "\"" "" )
                        set paths[-1] $p
                     end
                else if string match -q "*name: *" -- $line
                     if test -n "$current_id"
                        set -l n (string replace -r ".*name: \"" "" -- $line | string replace -r "\"" "" )
                        set names[-1] $n
                     end
                else if string match -q "*restore_path: *" -- $line
                     if test -n "$current_id"
                        set -l rp (string replace -r ".*restore_path: \"" "" -- $line | string replace -r "\"" "" )
                        set restore_paths[-1] $rp
                     end
                else if string match -q "*type: *" -- $line
                    if test -n "$current_id"
                        set -l t (string replace -r ".*type: " "" -- $line)
                        set -a types $t
                    end
                end
            end

            # FIX 5: Interactive Logic Flags
            set -l restore_all_subsequent 0
            set -l restore_count 0

            for i in (seq (count $ids))
                set -l id $ids[$i]
                set -l type $types[$i]
                
                set -l orig_path
                set -l src_path
                
                # Determine paths based on manifest version
                if test -n "$names[$i]"
                    # New format
                    set orig_path "$restore_paths[$i]/$names[$i]"
                    set src_path "$mount_point/to_restore/$id/$names[$i]"
                else
                    # Legacy format
                    set orig_path "$paths[$i]"
                     # Legacy structure was flat inside $id/ or contained files
                    if test "$type" = "directory"
                        set src_path "$mount_point/to_restore/$id/"
                    else
                        # For legacy files, we need to find the file inside
                        set -l f (ls -A "$mount_point/to_restore/$id/" | head -1)
                        set src_path "$mount_point/to_restore/$id/$f"
                    end
                end
                
                # Check for subsequent 'all' flag
                set -l do_restore 0
                
                if test $restore_all_subsequent -eq 1
                    set do_restore 1
                    echo "Auto-restoring #$id -> $orig_path"
                else
                    echo ""
                    echo "Entry #$id:"
                    echo "  Path: $orig_path"
                    echo "  Type: $type"
                    
                    read -P "Restore this? [y/n/a(all)/q(quit)] " -l choice
                    
                    switch $choice
                        case y Y yes
                            set do_restore 1
                        case a A all
                            set do_restore 1
                            set restore_all_subsequent 1
                        case q Q quit
                            break
                        case n N no
                            set do_restore 0
                        case '*'
                            # Default to no
                            set do_restore 0
                    end
                end

                if test $do_restore -eq 1
                    echo "  Restoring..."
                    
                    set -l dest_dir (dirname "$orig_path")
                    if not test -d "$dest_dir"
                        # Create parent dir if needed
                         if test -w (dirname "$dest_dir")
                            mkdir -p "$dest_dir"
                         else
                            $root_cmd mkdir -p "$dest_dir"
                         end
                    end
                    
                    # FIX 2: Check Permissions
                    set -l rsync_cmd "rsync"
                    if not test -w "$dest_dir"
                         echo "  Notice: Use $root_cmd to write to $dest_dir"
                         set rsync_cmd "$root_cmd rsync"
                    end

                    # Generic rsync for both files and dirs (works for new structure)
                    # For legacy dirs, src ends in slash? No, we used rsync -av src/ dst/
                    
                    if test -n "$names[$i]"
                         # New Format: copy unit (dir or file) into parent dir
                         $rsync_cmd -av "$src_path" "$dest_dir/"
                    else
                         # Legacy Format Restoration
                         if test "$type" = "directory"
                             if not test -d "$orig_path"
                                 if test -w (dirname "$orig_path"); mkdir -p "$orig_path"; else; $root_cmd mkdir -p "$orig_path"; end
                             end
                             $rsync_cmd -av "$src_path" "$orig_path/"
                         else
                             $rsync_cmd -av "$src_path" "$orig_path"
                         end
                    end
                    
                    set restore_count (math $restore_count + 1)
                end
            end

            echo ""
            echo "Restoration complete. ($restore_count items processed)"
            
            squash_manager umount "$mount_point"

        case check
            argparse 'use-cmp' 'force-delete' 'h/help' -- $argv
            if set -q _flag_help
                _zks_help
                return 0
            end

            if test (count $argv) -lt 1
                echo "Error: Archive path required."
                return 1
            end
            set -l archive_path $argv[1]

            if not test -f "$archive_path"
                echo "Error: Archive '$archive_path' not found."
                return 1
            end
            
            # Need get_root_cmd
            set -l root_cmd (functions -q get_root_cmd; and get_root_cmd; or echo "sudo")

            # Temporary mount point
            set -l mount_point "/tmp/zks_chk_"(random)
            
            echo "🔍 Mounting archive for check..."
            squash_manager mount "$archive_path" "$mount_point"
            or return 1
            
            # Helper to ensure cleanup on exit/interrupt
            function _zks_check_cleanup --inherit-variable mount_point
                if mountpoint -q "$mount_point"
                    squash_manager umount "$mount_point" >/dev/null 2>&1
                end
            end
            trap "_zks_check_cleanup" INT TERM EXIT

            set -l manifest "$mount_point/list.yaml"
            if not test -f "$manifest"
                echo "Error: Invalid archive format (list.yaml not found)."
                return 1
            end
            
            # Parse manifest (Same logic as unfreeze for compatibility)
            set -l ids
            set -l paths
            set -l names
            set -l restore_paths
            set -l types
            
            set -l current_id ""
            cat $manifest | while read -l line
                if string match -q "*id: *" -- $line
                    set current_id (string replace -r ".*id: " "" -- $line)
                    set -a ids $current_id
                    # placeholders
                    set -a names ""
                    set -a restore_paths ""
                    set -a paths ""
                else if string match -q "*original_path: *" -- $line
                     if test -n "$current_id"; set paths[-1] (string replace -r ".*original_path: \"" "" -- $line | string replace -r "\"" "" ); end
                else if string match -q "*name: *" -- $line
                     if test -n "$current_id"; set names[-1] (string replace -r ".*name: \"" "" -- $line | string replace -r "\"" "" ); end
                else if string match -q "*restore_path: *" -- $line
                     if test -n "$current_id"; set restore_paths[-1] (string replace -r ".*restore_path: \"" "" -- $line | string replace -r "\"" "" ); end
                else if string match -q "*type: *" -- $line
                    if test -n "$current_id"; set -a types (string replace -r ".*type: " "" -- $line); end
                end
            end
            
            set -l error_count 0
            set -l deleted_count 0
            
            for i in (seq (count $ids))
                set -l id $ids[$i]
                set -l type $types[$i]
                
                set -l sys_path
                set -l arc_path
                
                # Resolving paths
                if test -n "$names[$i]"
                    set sys_path "$restore_paths[$i]/$names[$i]"
                    set arc_path "$mount_point/to_restore/$id/$names[$i]"
                else
                    # Legacy
                    set sys_path "$paths[$i]"
                    if test "$type" = "directory"
                        set arc_path "$mount_point/to_restore/$id/"
                    else
                        set -l f (ls -A "$mount_point/to_restore/$id/" | head -1)
                        set arc_path "$mount_point/to_restore/$id/$f"
                    end
                end
                
                # --- Verification ---
                set -l check_failed 0
                set -l reason ""
                
                if not test -e "$sys_path"
                    set check_failed 1
                    set reason "Missing in system"
                else if not test -e "$arc_path"
                    set check_failed 1
                    set reason "Missing in archive"
                else
                    if test "$type" = "file"
                        # --- File Verification ---
                        set -l s_sys (stat -c %s "$sys_path" 2>/dev/null)
                        set -l s_arc (stat -c %s "$arc_path" 2>/dev/null)
                        
                        if test "$s_sys" != "$s_arc"
                            set check_failed 1
                            set reason "Size mismatch (Sys: $s_sys, Arc: $s_arc)"
                        else if set -q _flag_use_cmp
                            set -l cmp_cmd "cmp"
                            if not test -r "$sys_path"; set cmp_cmd "$root_cmd cmp"; end
                            
                            $cmp_cmd -s "$sys_path" "$arc_path"
                            if test $status -ne 0
                                set check_failed 1
                                set reason "Content mismatch (cmp)"
                            end
                        end
                    else
                        # --- Directory Verification (Recursive) ---
                        # 1. Compare structure and sizes using rsync dry-run
                        # Use a list for arguments to avoid fish passing them as a single quoted string
                        set -l rsync_flags -r -n --size-only
                        if set -q _flag_use_cmp
                            set rsync_flags -r -c -n # -c uses checksums
                        end
                        
                        # We use -v to get names of differing files
                        # If output is non-empty (excluding header/footer), there are differences
                        set -l diffs (rsync $rsync_flags --dry-run --out-format="%n" "$arc_path/" "$sys_path/" | string match -v -r '^$|/$')
                        
                        if test -n "$diffs"
                            set check_failed 1
                            set reason "Content/Size mismatch in files: "(string join ", " $diffs | string collect | string shorten --char 50)
                        else
                            # 2. Check for "extra" files in system that are not in archive
                            # This is important for --force-delete safety
                            set -l extra (rsync -r -n --dry-run --out-format="%n" --ignore-existing "$sys_path/" "$arc_path/" | string match -v -r '^$|/$')
                            if test -n "$extra"
                                set_color yellow
                                echo "Notice: [$sys_path] contains extra files not in archive"
                                set_color normal
                                # We don't mark as FAILED unless it's a conflict, 
                                # but if extra files exist, we MUST NOT delete the whole dir.
                                set -l has_extra 1
                            end
                        end
                    end
                end
                
                if test $check_failed -eq 1
                    set_color red
                    echo "WARNING: [$sys_path] -> $reason"
                    set_color normal
                    set error_count (math $error_count + 1)
                else
                    # Success Verification
                    if set -q _flag_force_delete
                        # Safety check: if it's a directory with extra files, don't delete recursively
                        # Re-run strict check for extra files before delete
                        if test "$type" = "directory"; and rsync -r -n --dry-run --out-format="%n" --ignore-existing "$sys_path/" "$arc_path/" | string match -v -r '^$|/$' >/dev/null
                            set_color yellow
                            echo "SKIP: $sys_path has extra files, won't delete recursively."
                            set_color normal
                        else
                            echo "MATCH: $sys_path (Deleting...)"
                            set -l rm_cmd "rm"
                            if not test -w (dirname "$sys_path"); or not test -w "$sys_path"; set rm_cmd "$root_cmd rm"; end
                            $rm_cmd -rf "$sys_path"
                            if test $status -eq 0; set deleted_count (math $deleted_count + 1); else; set_color red; echo "  Error deleting $sys_path"; set_color normal; end
                        end
                    end
                end
            end
            
            echo ""
            if test $error_count -eq 0
                set_color green
                echo "Check complete: All checked items match."
                set_color normal
                if set -q _flag_force_delete
                    echo "Deleted $deleted_count items."
                end
                return 0
            else
                set_color red
                echo "Check complete: Found $error_count mismatches/errors!"
                if set -q _flag_force_delete
                    echo "Deleted $deleted_count matched items."
                end
                set_color normal
                return 1
            end

        case '*'
            echo "Error: Unknown command '$command'"
            _zks_help
            return 1
    end
end
