# LibreRAM (Tauri)

Менеджер RAM-дисков для Windows на Rust + Tauri v2: фронтенд в системном WebView2, драйвер ImDisk (форк DavidXanatos, v3.0.3, подпись WHQL) ставится из встроенных файлов. Один exe + MSI ~3 МБ; Python в комплект не входит.

## Возможности

- Список устройств ImDisk, создание RAM-диска (свободная буква из списка, размер, NTFS/FAT32/exFAT, кластер, метка, папки)
- Загрузка существующего сырого образа в память
- Сохранение образа тома (блокировка тома, без перезаписи)
- Отключение диска с fallback на принудительное
- «Чистый диск при входе»: задача планировщика `LibreRAM-Session`, окно в трее
- Совместимый формат `session.json` с Python-версией LibreRAM

## Сборка

```powershell
# Rust (MSVC) + Node.js; CLI: npm i -g @tauri-apps/cli
cd src-tauri
npm exec tauri build   # MSI: src-tauri/target/release/bundle/msi/
cargo test             # unit-тесты
```

## Установка

MSI из раздела Releases. Приложение запускается с правами администратора (манифест requireAdministrator); при отсутствии imdisk.exe предложит установить драйвер.

## Лицензии

- Код LibreRAM: GPL-2.0-only (`LICENSE`)
- ImDisk: GPL, файлы и исходники форка — https://github.com/DavidXanatos/ImDisk (`src-tauri/resources/imdisk-setup/gpl.txt`)
