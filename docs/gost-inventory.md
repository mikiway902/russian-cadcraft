# Инвентаризация ГОСТ до модульной переработки

Исходная реализация: `35cfa991f309661891adecf4b4ee44c72bc47cc1`. Каталог получен реально работающим `cadcraft-cli commands gost`; наличие команды не означает приёмку всего нового roadmap.

| Команда | Назначение |
|---|---|
| `gost.setup` | GOST Drawing Setup |
| `gost.check` | GOST Drawing Check |
| `gost.sheet` | GOST Drawing Sheet |
| `gost.document` | GOST Document Properties |
| `gost.sheet.set` | GOST Sheet Properties |
| `gost.book.sync` | Synchronize GOST Sheets |
| `gost.sheet.order` | Reorder GOST Sheet |
| `gost.center` | GOST Center Axes |
| `gost.section` | GOST Cutting Plane |
| `gost.view` | GOST View Designation |
| `gost.hatch` | GOST Metal Hatch |
| `gost.roughness` | GOST Roughness |
| `gost.weld` | GOST Weld |
| `gost.thread` | GOST Symbolic Thread |
| `gost.symbol.edit` | Edit GOST Engineering Symbol |
| `gost.symbol.inspect` | Inspect GOST Engineering Symbol |
| `gost.tolerance` | GOST Geometric Tolerance |
| `gost.check.show` | Show GOST Finding |
| `gost.requirements` | GOST Technical Requirements |
| `gost.requirements.inspect` | Inspect Technical Requirements |
| `gost.requirements.templates` | Technical Requirement Templates |
| `gost.specification` | GOST Specification |
| `gost.specification.edit` | Edit GOST Specification |
| `gost.specification.inspect` | Inspect GOST Specification |
| `gost.position` | GOST Position |
| `gost.position.edit` | Edit GOST Position |
| `gost.position.inspect` | Inspect GOST Position |
| `gost.solid` | GOST Polyhedral Solid |
| `gost.projection` | GOST Associative View |
| `gost.section.view` | GOST Associative Section |
| `gost.projection.inspect` | Inspect Associative View |
| `gost.projection.dimension` | GOST View Dimension |

## Данные и границы

Команды используют нативные сущности CADCraft и версионированные записи `CADCRAFTGOST` в DXF xdata. UI вызывает тот же engine; JSON не открывает диалоги.

- Листы A4–A0, формы 1/2а, общие реквизиты и нумерация книги. Спецификация создаёт собственные A4-таблицы с основными надписями 2/2а.
- Знаки шероховатости, сварки, условной метрической резьбы и геометрического допуска; резьба не является 3D-винтовым телом.
- Требования с переносами, колонками и явной ошибкой переполнения.
- Спецификация по атрибутам ссылок INSERT и ручным строкам; позиции и оси ассоциативны с нативной 2D-геометрией.
- Многогранные виды с удалением невидимых линий, реальное плоское рассечение и размеры по устойчивым ID вершин/рёбер. Это не точные B-Rep-кривые, STEP/IGES и не связь с SolidCraft.
- Нормоконтроль ограничен документированной поддерживаемой геометрией и бюджетами. Сложные блоки и импортированные размерные блоки требуют ручного контроля; положительный сертификат не выдаётся.

Полный CI и исходные регрессии фиксируются в [PROGRESS.md](../PROGRESS.md). Ограничения пользовательских команд: [gost.md](gost.md).
