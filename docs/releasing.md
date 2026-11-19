# Сборка, обновления upstream и откат CADCraftGOST

## Независимый продукт

- Репозиторий и канал обновлений: `mikiway902/russian-cadcraft`, не upstream CADCraft.
- Product ID: `io.github.mikiway902.cadcraftgost`; пакет/установленный executable: `cadcraftgost`, CLI: `cadcraftgost-cli`.
- Имена Cargo-пакетов остаются `cadcraft`, `cadcraft-cli` и `cadcraft-web`: это внутренние build targets, не идентичность установки.
- Версия берётся из `[workspace.package]` корневого `Cargo.toml`; `cargo xtask version` показывает её. Схема upstream `0.21.x` пока сохраняется; условные номера этапов roadmap не меняют версии установщиков автоматически.
- Наличие workflow не доказывает готовность артефактов, наличие signing secrets, успешную установку или опубликованный Alpha.

## Quality gate

```sh
cargo xtask ci
```

Восемь шагов: fmt, Clippy, workspace-тесты и doctests, отчёт i18n, атрибуция assets, слои зависимостей, WASM и функциональная проверка отключения расширений.
Последний шаг проверяет настоящие DXF/LINE/COPY/Undo и будущие opaque XDATA, а также CLI, desktop и web без default feature `gost`.
Пробелы переводов выводятся отдельно; успешный i18n gate не означает полной русификации.

Изолированный точный worker проверяется отдельно, пока не включён в root workspace:

```sh
cargo test --locked --manifest-path tools/solidcraft-bridge/Cargo.toml --all-targets
cargo clippy --locked --manifest-path tools/solidcraft-bridge/Cargo.toml --all-targets -- -D warnings
```

## Проверка обновлений CADCraft

```sh
bash scripts/check-upstream.sh
```

Скрипт получает `storytold/cadcraft` и проверяет merge tree без изменения текущего индекса, исходников или ветки.
При конфликте/ошибке он завершается ненулевым кодом, сохраняя `target/upstream-check/{revisions,merge}.txt`.
Бесконфликтный кандидат собирается в отдельном временном detached worktree; там выполняется полный `cargo xtask ci`, вывод сохраняется в `gate.log`.
Даже успешный кандидат не коммитится, не принимается и не отправляется автоматически. Неудачный gate также запрещает принятие.
Переменные `CADCRAFTGOST_UPSTREAM_URL` и `CADCRAFTGOST_UPSTREAM_BRANCH` предназначены для явно выбранных кандидатов; по умолчанию используются оригинальный репозиторий и `main`.

`.github/workflows/upstream-check.yml` запускает эту же проверку ежедневно и вручную с read-only permissions.
При сбое логи остаются в выводе job; shared gate загружает artifact только после успешной команды.
Принятие проверенной ревизии — отдельное изменение через review, с сохранением предыдущей стабильной точки и повторной приёмкой затронутых функций. Конфликты никогда не разрешаются выбором всего `ours`/`theirs`.

## Возврат к проверенной версии

Исходная реализация сохранена в `backup/pre-modular-gost-20261119` на `35cfa991f309661891adecf4b4ee44c72bc47cc1`.
Модульная версия с восемью прошедшими gate и Linux GUI-приёмкой сохранена в `stable/modular-20261119` на `7fe3b9e010a3865791823cdc5e124aba8c87f667`.
Это локальные точки исходников, не заявление о опубликованном stable-релизе.

Для проверки старой версии без сброса текущих правок создайте отдельный worktree:

```sh
git worktree add --detach ../cadcraftgost-rollback stable/modular-20261119
cd ../cadcraftgost-rollback
cargo xtask ci
cargo build --locked -p cadcraft
CADCRAFT_CONFIG_DIR=/absolute/path/to/separate-settings target/debug/cadcraft
```

Не используйте `reset --hard`, force-push или удаление пользовательских каталогов для отката.
Перед заменой установленного пакета закройте приложение и сохраните отдельные копии чертежей/настроек. Устанавливайте только ранее проверенный артефакт с проверенной SHA-256; если такого артефакта нет, соберите проверенную ревизию.
Не открывайте единственный экземпляр документа новой версии старым приложением: работайте с копией. Автоматическое понижение формата/метаданных не выполняется.

## Установщики и публикация

`.github/workflows/ci.yml` использует общий gate на Linux, Windows и macOS; успешный удалённый запуск каждой платформы подтверждается отдельно.
`.github/workflows/release.yml` зависит от acceptance gate и вызывает общий packaging workflow с собственными именами `cadcraftgost`/`CADCraftGOST` и `publish: false`.
Планируемые артефакты: Linux DEB/RPM/AppImage/tar.gz/Flatpak, Windows MSI/portable ZIP, macOS DMG/CLI ZIP, FreeBSD tar.gz, web ZIP.
Подпись и notarization возможны только при наличии собственных настроенных сертификатов; сертификаты оригинального проекта не подразумеваются.
Windows manifest — `packaging/windows/cadcraftgost.wxs`; собственные UpgradeCode, компоненты и путь установки не должны обновлять/удалять CADCraft.
Alpha публикуется только после критериев roadmap и отдельной проверки артефактов; запуск проверки upstream ничего не публикует.
