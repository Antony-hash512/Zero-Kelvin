function rm-if-empty --description "Safely remove directory ONLY if it contains 0-byte files or empty directories"
    #Мелкий технический нюанс (Edge Case): Если в папке окажется файл с переводом строки в названии
    # (например, bad\nfile), который при этом не пустой, то переменная $dangerous_files будет содержать список, 
    #и конструкция if test -n "$dangerous_files" может выдать ошибку синтаксиса matches в fish (too many arguments),
    # из-за чего блок if может быть пропущен.
    #Последствие: Пользователь не увидит красивое сообщение "CRITICAL ERROR".
    #Безопасность: Файл НЕ будет удален все равно, так как следующая команда удаления также имеет фильтр -size 0c. 
    #Так что данные в безопасности. Это проблема чисто косметическая и крайне маловероятная в реальных сценариях
    # (особенно в build-директориях).
    
    set -l target $argv[1]

    if test -z "$target"
        echo "Error: No target specified for rm-if-empty"
        return 1
    end

    if not test -d "$target"
        echo "Error: Target '$target' is not a directory or does not exist."
        return 1
    end

    # ЭТАП 1: ПРОВЕРКА БЕЗОПАСНОСТИ (Safety Check)
    # Ищем файлы, которые НЕ 0 байт ИЛИ не являются обычными файлами/каталогами
    # -type f : файлы
    # ! -size 0c : размер НЕ 0 байт
    set -l dangerous_files (find "$target" -type f ! -size 0c -print -quit)
    
    if test -n "$dangerous_files"
        echo "⛔ CRITICAL ERROR: Found non-empty file inside build dir!"
        echo "File: $dangerous_files"
        echo "Operation aborted. Nothing was deleted."
        return 1
    end

    # Доп. проверка: есть ли что-то, что не файл и не каталог (например, socket, device, symlink)
    # Симлинки в build-dir особенно опасны, если они ведут наружу
    set -l alien_objects (find "$target" ! -type f ! -type d -print -quit)
    if test -n "$alien_objects"
        echo "⛔ CRITICAL ERROR: Found non-file/non-dir object (symlink/device)!"
        echo "Object: $alien_objects"
        return 1
    end

    # ЭТАП 2: УДАЛЕНИЕ (Cleanup)
    # Сначала удаляем пустые файлы
    find "$target" -type f -size 0c -delete
    
    # Затем удаляем пустые каталоги (ключ -depth важен, чтобы удалять вложенные папки перед родительскими)
    find "$target" -depth -type d -empty -delete

    # Проверяем, исчез ли сам целевой каталог
    if test -d "$target"
        echo "Warning: Directory structure cleaned, but root '$target' remains (maybe not empty?)"
        return 1
    else
        return 0
    end
end