# Архитектура CADCraftGOST

## Действующее основание

Исходная архитектура CADCraft описана в [docs/architecture.md](docs/architecture.md). `geom` → `doc/color` → `fonts/render/constraints` → `io` → `engine` → `ui-egui/mcp` → приложения. Документ — copy-on-write `Drawing`; команды JSON и интерактивные машины используют один `Session`. Отмена сохраняет состояние документа, а не выполняет обратную геометрию.

## Текущее разделение модулей

| Компонент | Расположение | Назначение |
|---|---|---|
| Engine Extension API | `crates/engine/src/extensions.rs` | Неизменяемые дескрипторы, команды/меню через существующий `CommandSpec`, события, точные пары COPY, подготовка clipboard |
| Оформление ГОСТ | `extensions/gost.rs`, `extensions/gost/*.rs` | Профиль ЕСКД, книга листов, рамки, инженерные знаки и технические требования |
| Версионируемые нормы | `data/gost/*`, `extensions/gost/standards*`, `crates/engine/build.rs` | Каталог источников и typed policy; проверка выбранных редакций при сборке, нормативная идентичность документа |
| Геометрический нормоконтроль | `extensions/norms.rs` | Отдельный read-only анализ пересечений/интервалов с ограниченными бюджетами; нормативная политика вызывается из профиля |
| Спецификации | `extensions/specification.rs` | Состав, редактируемые таблицы, пагинация и ассоциативные позиции |
| Drawing Extension | `extensions/drawing.rs`, `crates/geom/src/drafting3d.rs` | Сохранённые многогранные виды/сечения и привязки размеров; пока не B-Rep |
| Общие метаданные | `extensions/data.rs` | Ограниченные версионированные записи DXF xdata `CADCRAFTGOST` |
| UI Extension API | `crates/ui-egui/src/extensions.rs`, `extensions/gost/*` | Команда → имя диалога → render function; только вызовы без параметров открывают окно |
| CLI/MCP | `apps/cadcraft-cli`, `crates/mcp` | Тот же реестр engine, без отдельного языка команд |
| Изолированный B-Rep worker | `tools/solidcraft-bridge` | Публичные STEP/IGES, `.sc`, точный HLR и ограниченный display mesh SolidCraft/SolidCore |
| Host transport и native curves | `crates/io/src/model_worker.rs`, `native_models.rs`, `model_projection.rs`, `model_preview.rs` | Ограниченный child-process JSON-RPC, fingerprint источника, четыре кэшированные модели; без зависимости engine от SolidCraft |
| Связанные модели | `extensions/models.rs`, `engine/model_io.rs`, `doc/model_{projection,preview}.rs` | JSON-команды, NOD XRECORD, frozen native curves и подготовленные транзакции без I/O на UI completion |
| Менеджер и 3D-просмотр | `ui-egui/extensions/models*` | Фоновые UI-задания с UID/revision guards; временная Drawing, существующие Frame/renderer и отдельный GPU layer |
| Шрифт | `assets/fonts/opengost`, `crates/fonts` | Офлайн OpenGost Type B, OFL-1.1, разрешённое исключение форка |

Это compile-time Extension API, не динамический Plugin ABI. Порядок регистрации: оформление → спецификации → связи видов. `DocumentChange` заимствует `before: Arc<Drawing>`, UID исходного документа и ID команды. Callbacks выполняются внутри существующего guard/Undo команды; ошибка откатывает всю транзакцию. COPY передаёт точные пары старых/новых handles, собираемые только для объектов с обслуживаемыми связями. Нативные команды не ищут ГОСТ-генераторы и не знают их имён: подготовленное имя листа передаётся через `CommandContext`.

### Изоляция продуктовых значений по умолчанию

`Session::empty`, `Session::new` и `Session::new_drawing(metric)` сохраняют нативные контракты CADCraft. `Session::new_document(Drawing)` принимает подготовленный документ с чистой базой Undo/сохранения. Продуктовые хосты выбирают `engine::extensions::session()` (Start) или `new_session()` (CLI/MCP). `NEW` использует зарегистрированный `DrawingPreset`; явные дюймовые документы и открытые файлы не преобразуются.

Feature `gost` включена по умолчанию и явно передаётся через engine, ui-egui, mcp и три приложения. `cargo build -p cadcraft --no-default-features` и аналогичная CLI/web-сборка исключают ГОСТ-команды, бизнес-модули, callbacks и диалоги, сохраняя нативное черчение и opaque XDATA. Функциональные DXF/Undo-сценарии и проверка трёх хостов входят в `cargo xtask extensions`, также добавленный в общий CI. Восемь gate прошли; настоящие GUI с расширениями и без них проверены на Linux. Результаты: `tests/benchmarks/modular-extensions-20261119.json`.

### Desktop-хранилище

Workspace, логи, support, autosave и eframe `app.ron` выбираются одним `Env::config_dir`. Сам `NativeOptions::persistence_path = None` означает глобальный store eframe, а не отключение: поэтому host всегда задаёт явный файл. Пустой `CADCRAFT_CONFIG_DIR` использует приватный временный store с RAII-удалением; постоянные пользовательские каталоги не читаются и не изменяются. Пользовательский `SAVEFILEPATH`, сохранённый внутри выбранного профиля, остаётся явной настройкой владельца.

### Нормативная идентичность

Профиль `rf-2026-v1` и каталог — внешние JSON-данные исходного дерева. Build script
проверяет их схему, комплектность, применимость редакций в РФ на дату снимка,
отсутствие преждевременного выбора будущих редакций и соответствие потребляемых
правил выбранным редакциям. Он генерирует typed constructor и исходную запись
профиля для infallible фабрики документа; неверные bundled данные не допускаются
к сборке. При отключённой feature `gost` генерация не выполняется.

Документ хранит только версионируемый выбор и алфавит в NOD XRECORD
`CADCRAFT_GOST_PROFILE`. Неизвестные записи расширений сохраняются через
`Drawing::foreign`; префикс `CADCRAFT` не означает, что reader уже моделирует запись.
Ранее открытые документы без записи остаются `legacy-unversioned`. Выбор профиля
не преобразует единицы или геометрию, а неизвестная версия возвращает ошибку.
Политика оформления, полнота нормативного охвата и официальный контроль документа —
разные свойства; каталог источников не является сертификатом.

### Граница с upstream

Нативные команды сохраняют владение проверками параметров, транзакцией и Undo. Точки подключения ограничены реестром команд, post-command событием, точными парами COPY, подготовкой clipboard и двумя overrides (`layout.new`, `centermark`); бизнес-реализации и окна находятся в `extensions/*`, без обратных re-export. Feature-off CI проверяет сохранность нативного поведения и неизвестных данных DXF. Настройка бренда/пакетов отделена от этих инженерных модулей.

Проверка upstream строит только временный кандидат (`scripts/check-upstream.sh`), никогда не продвигает ветки автоматически. Процедура review, стабильные локальные ревизии и недеструктивный откат описаны в `docs/releasing.md`.

## Целевая ответственность

- SolidCraft/SolidCore: параметрическое моделирование, B-Rep, STEP/IGES, детали и сборки.
- Drawing Extension: получение точных проекций, разрезов, геометрических ссылок и измерений; отображение редактируемыми сущностями CADCraft.
- GOST Extension: нормативные профили, оформление, аннотации, требования, спецификации и нормоконтроль.
- CADCraft: 2D-документ, существующий рендерер, обмен, команды, транзакции и пользовательский интерфейс.

Нативный B-Rep-путь — отдельный Rust-worker без переноса GUI. Протокол
`cadcraft-model-worker/1` использует `craft-jsonrpc`, сохраняет рациональные коники,
явные поля качества, нормализованные единицы и fingerprint источника. `.sc`
разбирается публичными `solidcraft-doc`/`solidcraft-format`: исходный Document,
миграции, Model evaluation и world placement, без доверия preview/cache.
Материалы и обозначения передаются только при наличии сохранённых значений.

Host `model_worker::Client` ограничивает кадры/ожидание, отличает remote error от
поломанного transport и завершает собственный процесс. `model_projection::decode`
проверяет весь результат до создания native geometry; conic становится rational
quadratic SPLINE, cubic/Bezier — clamped SPLINE, круг/дуга/отрезок остаются native.
Сохраняются source indices, интервал, направление дуги, качество и видимость;
тесселяция или подмена неопределённых линий точными не выполняется.

Native HATCH хранит взаимоисключающие polyline/bulge vertices либо directed
line/arc/ellipse/rational-SPLINE edges. DXF codec сохраняет точные границы;
clockwise complementary angles и geometric ellipse angles преобразуются явно.
Rendering имеет общий point budget и не меняет хранимую геометрию. AREA сохраняет
аналитические line/arc/ellipse terms, SPLINE использует существующую numerical
span policy. Старый polyline refill не подменяет curved paths: source edit явно
снимает неподдерживаемую ассоциативность с сообщением. REGION остаётся
polyline/bulge-only и отвергает curve-path payload при десериализации.

Host подключён к desktop и CLI через семь `ModelHooks`: загрузка, точный вид,
точный разрез, preview, проверка fingerprint, discovery references и resolve point.
Saved records хранят полный provenance, ревизии и замороженные редактируемые
native curves; preview никогда не попадает в документ,
размеры или измерения. Refresh меняет ревизию источника, но не подменяет старые виды:
они явно становятся stale. Проверка SHA до/после точного вида запрещает новую
геометрию из изменённого файла даже при наличии старой модели в кэше.

Raw quality и covered/uncertain provenance не переписываются. Обычный вид может
исключить uncertain piece только при подтверждённом upstream coverage contract,
когда тот уже покрыт видимой геометрией; uncovered uncertainty требует явного
разрешения неполной геометрии. Это не формальное доказательство точности HLR.

Настоящие native/STEP/IGES CLI → Drawing → DXF сценарии, native GUI import,
orbit и Front view, Undo/Redo и изменение источника проверены на Linux.
Полная ассоциативная перестройка, межревизионная топология и ресурсная изоляция
требуют отдельной приёмки; payload-local indices не являются постоянными именами.
Reference RPC использует реальные upstream NameCells и явные статусы
потери/неоднозначности. Host валидирует bounded discovery/point audit и SHA до/после
вычислений; engine не читает файлы. `models::dimensions` создаёт native DIMENSION
по двум точным world anchors. Document event перепроецирует только кэшированные
точки при изменении размещения вида, не вызывает worker. Связь и audit сохраняются
в XDATA; stale/missing/nested view видимо помечает размер `?`. COPY/clipboard
переносит пару «вид + размер» через настоящую карту handles, без nearest matching.
GUI discovery/resolve выполняются в фоне, completion проверяет UID/revision/space
и ревизию linked source перед guarded transaction. Полная автоматическая
межревизионная перестройка видов и размеров этим не подменяется.

`model_section::Section` отделяет исходные body placements от result-local
топологии boolean. IO переиспользует exact projection converter, проверяет
плоскость/basis, compact mapping, topology bounds, provenance и directed cap loops;
входные кривые не аппроксимируются. Worker вычитает исходные outward coplanar
области из merged cut faces перед выдачей крышек.
`models::sections` добавляет HATCH overlay в общий installer, сохраняет audit
в SavedView.provenance.section и явно возвращает empty/no_intersection без мутации.
3 мм бумажного шага хранятся как model-space pattern delta с reciprocal INSERT
scale. Малый отдельный style marker переживает clipboard-remapping; document event
отклоняет нарушающие стиль generic/nested transforms без чтения большого Store.
Source-anchor validation отвергает удалённые/исключённые тела и точки вне kept side.
GUI section completion использует тот же UID/revision/space/source guard, что и вид.
Presentation `cutaway` сохраняет прежний вид за плоскостью; `detached`/`imposed`
устанавливают только native cap outlines и HATCH с основным/тонким контуром.
Общий installer всё равно проверяет полный projection payload и сохраняет raw
quality/provenance, даже если retained-body curves не рисуются. Структурная
валидация и отказ на uncertainty не обходятся переключением presentation.
Cross-section dimensions дополнительно ограничены рассечённым body и плоскостью.
Два новых представления не вводят иной worker protocol и не подменяют отсутствующее
автоматическое оформление/перестроение; требования ручной проверки сохраняются.

`models::projections` хранит first-angle DAG в `SavedView.provenance.projection_link`
(version 1). Активная ссылка — `parent`; `original_parent` остаётся только audit.
Метаданные проверяются при загрузке/сохранении Store: source identity, отсутствие
циклов и допустимая глубина. Создание и document events проверяют бумажные INSERT
transforms. Малый handle-free XDATA marker позволяет пропустить несвязанные правки
без декодирования Store.
MOVE родителя транзакционно переносит потомки; MOVE потомка ограничен свободной
осью. Scale/rotate/mirror/layout/nested transformations активной семьи отклоняются,
не подменяя сохранённые exact curves. Удалённый родитель даёт явный missing_parent.
Clipboard remap использует общую grammar и только подтверждённую карту реально
перенесённых видов; placeholder handles и audit никогда не становятся связями.
GUI вычисляет новую проекцию в прежнем bounded worker и применяет прежние
UID/revision/space/source guards. Frame completion исполняется в locale scope
выбранного интерфейса, включая закрытый менеджер.

## Инварианты перехода

1. Работающие команды и сохранённые DXF не удаляются до равноценной проверенной замены.
2. Изменение модели, видов, аннотаций и спецификации — одна транзакция с откатом при ошибке.
3. Нет циклов зависимостей и UI-зависимостей ниже UI-слоя.
4. Входные данные и бюджеты геометрии ограничены; неоднозначная ссылка не перепривязывается молча.
5. Нет собственного параметрического 3D-редактора, нового твердотельного ядра или замены рендерера.
6. Работа офлайн; облачный ИИ не обязателен.
7. Точная геометрия и редакции стандартов подтверждаются отдельными проверками, не названием профиля.
