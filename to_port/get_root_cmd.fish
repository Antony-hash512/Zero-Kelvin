function get_root_cmd --description "Safely detect root privilege command using a whitelist"
    # 1. БЕЛЫЙ СПИСОК (WHITELIST)
    # Добавили:
    # run0 - нативный инструмент systemd (очень важно для Arch)
    # please - популярная Rust-альтернатива
    set -l allowed_commands run0 sudo doas pkexec sudo-rs please

    # 2. ПРОВЕРКА ПОЛЬЗОВАТЕЛЬСКОЙ ПЕРЕМЕННОЙ
    if set -q ROOT_COMMAND
        set -l user_cmd (string split " " $ROOT_COMMAND)[1]
        
        # Если пользователь указал одну из разрешенных команд
        if contains $user_cmd $allowed_commands
            if command -v $user_cmd >/dev/null 2>&1
                echo $user_cmd
                return 0
            end
        else
            # Если команда не в списке, игнорируем (защита)
            echo "Предупреждение: '$ROOT_COMMAND' нет в белом списке безопасных утилит. Игнорирую." >&2
        end
    end

    # 3. АВТОМАТИЧЕСКИЙ ПОИСК (В порядке приоритета для Arch)
    # Сначала проверяем классику, затем systemd-инструмент
    if command -v sudo >/dev/null 2>&1
        echo "sudo"
        return 0
    end

    if command -v doas >/dev/null 2>&1
        echo "doas"
        return 0
    end
    
    # run0 часто является симлинком или alias-ом, но в Arch это бинарник
    if command -v run0 >/dev/null 2>&1
        echo "run0"
        return 0
    end

    if command -v pkexec >/dev/null 2>&1
        echo "pkexec"
        return 0
    end
    
    # Если стоит Rust-аналог
    if command -v please >/dev/null 2>&1
        echo "please"
        return 0
    end

    echo "Ошибка: Не найдено доверенных утилит (sudo, doas, run0, pkexec)." >&2
    return 1
end
