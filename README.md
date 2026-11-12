# LibreRAM (Tauri)

Менеджер RAM-дисков для Windows на Rust + Tauri v2. Интерфейс работает в системном WebView2. MSI включает автономный установщик WebView2 и драйвер ImDisk (форк DavidXanatos v3.0.3 с подписью WHQL). Python в комплект не входит.

## Возможности

- Список устройств ImDisk, создание RAM-диска (свободная буква из списка, размер, NTFS/FAT32/exFAT, кластер, метка, папки)
- Загрузка существующего сырого образа в память
- Сохранение образа тома (блокировка тома, без перезаписи)
- Отключение диска. При ошибке блокировки тома принудительное отключение требует отдельного подтверждения.
- «Чистый диск при входе»: задача планировщика `LibreRAM-Session`, окно в трее
- Совместимый формат `session.json` с Python-версией LibreRAM
- Проверка обновлений через GitHub. Приложение открывает страницу релизов после подтверждения. Установка выполняется вручную.

## Сборка

```powershell
# Rust (MSVC) + Node.js; CLI: npm i -g @tauri-apps/cli
cd src-tauri
npm exec tauri build   # MSI: src-tauri/target/release/bundle/msi/
cargo test             # unit-тесты
```

Для UI-тестов нужны Node.js 20+ и Microsoft Edge. Выполните из корня репозитория:

```powershell
npm ci --ignore-scripts
npm test
```

## Установка

MSI из раздела Releases. Приложение запускается с правами администратора (манифест requireAdministrator); при отсутствии imdisk.exe предложит установить драйвер.

MSI включает [Microsoft Visual C++ Redistributable x64](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist). LibreRAM устанавливает его перед ImDisk, если в системе отсутствуют нужные библиотеки.

## Лицензии

- Код LibreRAM: GPL-2.0-only (`LICENSE`)
- ImDisk: GPL, файлы и исходники форка — https://github.com/DavidXanatos/ImDisk (`src-tauri/resources/imdisk-setup/gpl.txt`)
