# Выполнение CADCraftGOST_ROADMAP

## Правила учёта

Полный план: [ROADMAP.md](ROADMAP.md). Предыдущий семиэтапный план из `docs/gost-roadmap.md` не является приёмкой этой дорожной карты. Пункт отмечается выполненным только при наличии проверки актуального состояния. Непроверенные свойства и платформы не считаются готовыми.

## Исходная точка — 19.11.2026

- Сохранённая реализация: `35cfa991f309661891adecf4b4ee44c72bc47cc1`.
- Резервная ветка: `backup/pre-modular-gost-20261119`; рабочая: `roadmap/modular-alpha-20261119`.
- Оригинальный репозиторий добавлен как `upstream`: `https://github.com/storytold/cadcraft.git`.
- Получен `upstream/main` на `a770fa5a5e5f9d4ffe4dc0f60db5eacb6665c980`.
- Общий предок: `11d66cc3cd6a3676aad79401c9d390f3ccbf3aa7`; 3 коммита форка и 2 новых upstream-коммита. Автоматическое слияние не выполнялось.
- Изменения форка относительно общего предка: 92 файла, +13020/−329 строк; полный список получен через `git diff --name-status BASE HEAD` до переработки.
- Полный исходный `cargo xtask ci` завершён после исправления выявленных регрессий; результаты ниже.
- Аудит реализации и исследование публичных модулей SolidCraft/SolidCore выполнены; рабочий изолированный прототип проверен отдельно.

## Известные границы исходной реализации

- Собственные многогранные `gost.solid`/`gost.projection` не заменяют точное B-Rep-ядро SolidCraft. Работающие команды сохраняются до проверенной равноценной замены.
- ГОСТ-команды, нормы, спецификации и многогранные виды перенесены в независимые engine-модули `extensions/*`; UI использует отдельный compile-time реестр. Отключённая сборка проверена через реальные DXF/Undo-сценарии, native GUI и CLI/desktop/web compilation.
- Точные STEP/IGES-модели, аналитические силуэты, сборки SolidCraft и ссылки на точную геометрию ещё не приняты.
- Сборки на Windows/macOS, установщики рядом с CADCraft и собственный Alpha-релиз ещё не приняты.
- Наличие тестов предыдущей реализации не подтверждает прохождение полного набора на текущем состоянии.

## Следующая проверка

Завершить удаление фирменных ресурсов и приёмку собственного канала обновлений/пакетов. Затем подключить точный SolidCraft worker к пользовательскому приложению; отдельный worker-прототип не закрывает этот этап.

## Проверки и исправления исходного аудита

- Полный `cargo xtask ci` выявил три ошибки Clippy в тестовых целях: ненужное копирование сетки, неиспользуемый импорт веб-теста и чрезмерно сложный тест листа. Копирование и импорт удалены; тест разделён на проверку бумажных единиц и геометрию формы. Форматирование и полный Clippy после исправления прошли.
- Полный workspace-прогон выявил 185 отказов до прерывания: основная общая причина — продуктовый ГОСТ-пресет был встроен в низкоуровневые `Session::new`/`new_drawing`, меняя чужие слои, handles, стили и Layout1/Layout2. Нативные конструкторы восстановлены; desktop/web/CLI/MCP явно выбирают `extensions::session`/`new_session`. Удалена временная тестовая обёртка вместо массового ослабления проверок.
- Исходный аудит: [gost-baseline-audit.md](docs/gost-baseline-audit.md). Зафиксировано 14 конкретных регрессий, не заглушки; ограничения сеточного 3D не выдаются за точный STEP/B-Rep.
- Проверено `cargo test --locked -p cadcraft-geom --lib drafting3d`: **9 пройдено**. Разрезы через существующие рёбра, большие координаты и тонкие сечения, миллиметровое перекрытие длинного ребра и отказ на NaN/Inf публичного базиса.
- Проверено `cargo test --locked -p cadcraft-engine --lib cmd::gost`: **88 пройдено**. Добавлены сценарии коллизий блоков при вставке между документами (включая старые именованные блоки), удаления первой рамки, атомарного отказа на сдвиг/частичное удаление страниц спецификации, копирования вида с размером и DXF-перестроения, скрытых диагоналей POLYFACE и запрета молчаливой смены листа существующего вида.
- F1–F9 подтверждены указанными проверками. F10–F14 дополнительно проверены в настоящем Linux GUI: повторное изменение тела 40→50 сохраняет IDs 1–8; изменение положения позиции сохраняет владельца; лист существующего вида недоступен; смена выделенного вида обновляет редактор, конфликт черновика блокирует применение, повторное открытие загружает актуальную редакцию. После независимого листа формы 2а настоящая кнопка выпуска создаёт первый автоматический лист формы 1 и сохраняет TITLE/MATERIAL/ORG. [Результаты и хеши снимков](tests/benchmarks/gost-ui-audit-20261119.json).
- Инвентаризация: [32 зарегистрированные команды ГОСТ](docs/gost-inventory.md).
- Исходные измерения: [JSON](tests/benchmarks/pre-modular-20261119.json), по 5 запусков целого CLI-процесса. Медианы: sample-open **2,98 мс**, sample-normcontrol **5,65 мс**, box-isometric **4,53 мс**. Наблюдаемый максимум RSS дочерних CLI-процессов — **39812 КиБ**. Это не время отдельного геометрического алгоритма и не проверка крупных сборок.
- Исследование SolidCraft/SolidCore: [точные API, лицензии и ограничения](docs/solidcraft-reuse.md), ревизия `c5f47e501169c7f0f9e8191b2541e689e2f018e8`. Стоковый drawing snapshot аппроксимирует некруговые коники; рабочий изолированный прототип подтвердил возможность использовать публичный HLR API без переноса GUI.
- В новых редакторах устранены три production-`expect`: отсутствие документа обрабатывается через `Option` и штатное окно «нет чертежа». Исправлены реальные предусловия GUI-тестов, владение egui texture deltas и индексация объектных координат. Устаревшие проверки строк бренда/storage-ключа удалены, а MCP проверяет неизменность всего документа после отвергнутых вызовов.
- Изолированный прототип `tools/solidcraft-bridge`: **8 тестов пройдено** (`--locked --all-targets`), форматирование и Clippy `-D warnings` прошли. Реальный STEP-цилиндр сохраняет рациональные коники наклонной проекции и численно проверенные объём/площадь; проверены STEP в сантиметрах, IGES-параллелепипед, fingerprint исходника, ограничения путей/файлов и настоящий JSON-RPC-процесс. Это основание для отдельного native worker, но не готовый Bridge в приложении: отсутствуют `.sc`, 3D-просмотр, материалы и устойчивое сопоставление топологии между редакциями.
- **Этап 0 принят на Linux:** полный `cargo xtask ci` прошёл все 7 шагов — fmt, Clippy, workspace-тесты с doctests, i18n, assets, layers, WASM. **3983 пройдено, 0 отказов, 26 исходно игнорируемых сценариев**; 143 тестовые цели. [Машиночитаемый результат](tests/benchmarks/ci-baseline-20261119.json). F1–F14 исправлены, работающие ГОСТ-команды сохранены.
- Ограничения gate: WASM имеет 4 предупреждения dead-code; i18n — отчёт о пробелах, в русском словаре ещё **346 English fallback-строк**. Ни игнорируемые тесты, ни полная русификация, ни Windows/macOS этим прогоном не приняты.

## Этап 1 — модульное подключение

- Созданы неизменяемые engine-дескрипторы `Extension`, заимствованные события `DocumentChange`, overrides нативных команд и точные пары COPY. Существующие guard/Undo владеют всей транзакцией; бизнес-модули не управляют отдельной историей.
- Вынесены оформление ГОСТ, спецификации, геометрический нормоконтроль и Drawing Extension; старые `cmd/gost*` удалены переносом, без re-export-совместимости. UI-диалоги физически перенесены из native managers и подключаются таблицей command/name/render.
- Cargo feature `gost` явно передаётся шестью пакетами; отключение исключает команды, callbacks и GUI. В CI добавлен восьмой шаг `extensions`: настоящие LINE/COPY/Undo, будущие XDATA через DXF и feature-off CLI/desktop/web.
- **Этап 1 принят на Linux:** полный `cargo xtask ci` прошёл все **8 шагов**. Основной набор: **3983 пройдено, 26 исходно ignored**; дополнительно **2 feature-off DXF/Undo-теста** и проверки CLI/desktop/web без расширений. Старые GOST-модули не компилируются при отключении feature.
- Настоящий GUI обоих режимов проверен: включённые команды меню открывают перенесённые окна и создают тело/вид; отключённая сборка показывает Layout1/Layout2 и нативные стили, чертит LINE на слое 0 и отменяет её. [Результаты, геометрия и хеши снимков](tests/benchmarks/modular-extensions-20261119.json).

## Этап 2 — совместимость с upstream

- `scripts/check-upstream.sh` получает upstream, проверяет merge tree и запускает полный gate только во временном detached worktree. При конфликте или ошибке возвращает ненулевой код; ни один путь не коммитит, не продвигает и не отправляет текущую ветку.
- **Настоящий кандидат проверен:** `7fe3b9e` + upstream `a770fa5` объединились без конфликтов; прошли все **8 gate**, суммарно **4060 успешных тестов** (включая feature-off), **26 ignored**, 156 test targets. Upstream не принят автоматически. [Результат](tests/benchmarks/upstream-compatibility-20261119.json).
- CI/upstream/release YAML прошли `actionlint v1.7.12`; удалённого запуска Actions на момент проверки API ещё не было. Определены Linux/Windows/macOS gate и блокировка упаковки до acceptance; публикация отключена.
- Сохранена локальная стабильная ветка `stable/modular-20261119`; ревизии, ручное принятие и недеструктивный откат описаны в [docs/releasing.md](docs/releasing.md).
- Реальная пост-проверка выявила отдельную регрессию: общий target переиспользовал `xtask` с `CARGO_MANIFEST_DIR` удалённого кандидата. Исправлено: стабильный source-path, отдельный candidate target и run lock. Два Unix regression-теста используют настоящие локальные Git/Cargo-проекты: повторный успешный кандидат, merge-конфликт и реальная ошибка Rust-компиляции; caller продолжает читать свои файлы после удаления кандидата. Оба теста и Clippy прошли; root `assets` восстановлен и подтвердил 27 атрибутированных assets.

## Изоляция desktop-настроек

- Обнаружено по настоящему GUI: новый `CADCRAFT_CONFIG_DIR` изолировал workspace/логи, но eframe продолжал читать глобальный `app.ron`; оттуда попадали старые профиль и путь восстановления.
- Host теперь задаёт `persistence_path` явно. Пустой override получает приватный временный store с RAII-удалением вместо глобального профиля.
- **Проверено:** 70 desktop-тестов, Clippy `-D warnings`, desktop build. Настоящие отдельные GUI A/B сохранили разные темы и восстановили их после перезапуска. Временная light-сессия не изменила следующую временную dark-сессию; выбранные глобальные файлы не изменились, временный каталог удалился. Окна и отсутствие постороннего recovery проверены снимками.
- Доказательства: [settings-isolation-20261119.json](tests/benchmarks/settings-isolation-20261119.json). Windows/macOS и установка рядом с оригинальным executable пока не приняты.

## Продолжение — 21.11.2026

- По прямому запросу владельца `main` отправлен через SSH: сначала `74889621`,
  затем проверенный cache-fix `e1452557`. Force-push и замена пользовательских
  настроек remote не выполнялись.
- [Удалённый CI для `74889621`](https://github.com/mikiway902/russian-cadcraft/actions/runs/44897897118):
  Ubuntu 24.04 и Windows gate прошли; macOS gate завершился ошибкой, точная
  причина пока недоступна (API logs требует token, browser relay недоступен).
  Это не успешная приёмка трёх ОС и не проверка последующих ревизий.
- Создан каталог 18 нормативных семейств, 88 извлечённых правил и профиль
  `rf-2026-v1`. Источники, даты, поправки, будущие редакции и непроверенные
  официальные тексты разделены явно. Наличие правила в каталоге не означает его
  реализацию; все 18 нормативных acceptance-пунктов остаются открытыми.
- Добавлены API `gost.profile`, `gost.standards`, document-owned XRECORD,
  алфавит обозначений и связь их высоты с размерным текстом. Исправлена потеря
  профиля при DXF reopen: неизвестный `CADCRAFT*` key не считается автоматически
  моделируемой native записью. Проверка repeated reopen/Undo/Redo прошла.
- **101 engine extension-тест пройден.** После этого удалены оставшиеся
  production-`expect` из factory и lineweight conversions; Clippy engine/UI/IO
  all-targets прошёл. Typed policy и исходная identity-запись генерируются при
  сборке, неверный bundle отклоняется с понятной ошибкой; feature-off не читает
  policy. Проверены настоящий build-script на валидном bundle, отказ с exit 1
  на отдельном неполном fixture и успешный feature-off для того же fixture.
- Полный root gate выявил два настоящих i18n отказа: повторный русский ключ и
  два непереведённых китайских command labels. Исправлены; повторный gate прошёл
  все **8 шагов, 4005 тестов, 0 отказов, 26 ignored, 157 result-suites**.
  Остались 346 русских English fallback-строк и 4 исходных WASM dead-code warning.
  [Точные границы и результаты](tests/benchmarks/ci-profiles-bridge-20261121.json).
  Последующие точечные изменения CLI argv и Arc-кэша GUI проверяются отдельно.
- **SolidCraft worker: 12 тестов пройдено**, включая STEP/IGES, native JSON и
  контейнер, точное размещение сборки/свойства, испорченный authoritative blob,
  атомарность отказов и настоящий bounded JSON-RPC. Clippy all-targets прошёл.
  Лицензия embedded upstream Liberation Sans сохранена отдельно.
- **Настоящий host-client smoke:** native цилиндр R5 × 20 мм — объём
  `1570.7963267948958` мм³; наклонная проекция содержит 10 rational conic segments
  и `analytic_complete:true`; remote error не уничтожает модель, изменение
  файла обнаружено. Временные smoke-программы удалены.
- **Точный native curve adapter: 3 теста пройдены** — rational midpoint/weights,
  signed arcs и near-full gaps, malformed/nonfinite/oversized rejection.
  Подключение worker к пользовательскому документу, preview и persistent
  topology matching ещё не выполнены.
- End-to-end proof `Client → exact Seg2 → native SPLINE/LINE → DXF → reopen`
  прошёл для того же цилиндра: 10 рациональных квадратичных сплайнов, максимальная
  ошибка выборочных точек DXF **0 мм**, невязка аналитического уравнения эллипсов
  **1,9984·10⁻¹⁵**. Native render повторно открытого DXF просмотрен.
  [Результаты и ограничения](tests/benchmarks/exact-model-bridge-20261121.json).
- При подготовке Linux-пакетов найдена неверная исходная декларация glibc:
  ELF этой Fedora-сборки требует `GLIBC_2.44`, а DEB объявлял `>=2.35`.
  Packaging теперь извлекает минимум из реальных staged ELF для DEB/RPM.
  DEB/RPM пересобраны; их control/requires действительно содержат минимум 2.44.
- Настоящие official CADCraft 0.20.1 и упакованный CADCraftGOST запущены
  одновременно с общими XDG roots, без `CADCRAFT_CONFIG_DIR`: original сохранил
  light-тему, fork использовал dark. RPM не имеют общих недиректориальных
  путей; совпадают только общие системные каталоги icons/mime.
- В настоящем GUI профиль открытого original DXF был `legacy-unversioned`;
  реальная кнопка Apply выбрала `rf-2026-v1`, Undo/Redo восстановили оба состояния,
  сохранённый DXF после reopen сохранил выбор и исходную окружность R5 в (34,71).
  После закрытия original его файлы настроек/данных остались побайтно неизменными
  при последующем save/exit форка. Раннее сравнение при работающем original
  не было однозначным: original сам сохранил recentFiles после reopen.
  [Протокол приёмки и ограничения](tests/benchmarks/linux-coexistence-profile-20261121.json).
- Созданы все четыре локальных Linux-формата и zsync, appstream validation прошёл.
  DEB и RPM действительно объявляют минимум glibc 2.44; AppImage указывает собственный
  GitHub zsync channel. Это не системная установка RPM/DEB, не обновление готового
  релиза и не переносимая Ubuntu 22.04 сборка.
- Packaging smoke выявил второй CLI branding-дефект: `--version` уже был правильным,
  но parser оставлял `Usage: cadcraft-cli`. Оба argv пути используют `identity::CLI_BINARY`.
  Два help-теста, all-targets Clippy, release build и повторная упаковка прошли;
  настоящий установленный CLI показывает canonical argv в main/run/convert help,
  включая ошибочный `--script` без значения перед `--help`.
- Неизменяемые данные диалога норм разделяются через `Arc`, без глубокого копирования
  каждый кадр. В пересобранном AppImage восстановились dark theme и `rf-2026-v1`;
  настоящий диалог и нажатие Reload проверены визуально. All-targets Clippy прошёл.
- Настоящий собственный updater завершил запрос: branded CADCraftGOST dialog сообщил,
  что новой версии нет. Это проверка канала и UI, не установка обновления.
- Изучены публичные функции КОМПАС-3D: связанные виды/размеры/оси,
  местные разрезы и исключения штриховки, спецификации/позиции и реквизиты.
  [Сопоставление с roadmap и проверяемые критерии](docs/kompas-comparison.md).
  Закрытые шрифты, код и UI-ресурсы КОМПАС не переносились.
- Удалены четыре SVG марки ArtCraft. Четыре PNG ещё остаются:
  read/grep не возвращают snapshot tag для binary, обязательный для edit REM;
  дефект инструментов зарегистрирован. Удаление всего бренда не отмечено готовым.

## Связанные native-модели — продолжение 21.11.2026

- Desktop/CLI теперь подключают настоящие load/view/preview/status hooks.
  Данные источника и frozen native curves сохраняются в DXF; mesh для просмотра
  остаётся временным и не используется для измерений.
- Реальный CLI импортировал `.sc`, STEP-цилиндр и IGES-параллелепипед; объёмы
  соответственно `1570.7963267948958`, `1570.7963267948971`, `6000` мм³.
  Изометрические виды содержат 11, 11 и 12 редактируемых native curves.
  Reopen native DXF сохранил fingerprint, view handle, basis и quality.
- Настоящий GUI импортировал native цилиндр, показал upstream display mesh
  (544 triangles, 346 vertices); drag действительно изменил orbit.
  Во время import/view control inspect отвечал примерно за 5–7 мс.
- Default Front первоначально отказал: upstream сообщил два uncertain pieces,
  оба covered уже видимыми линиями. Исправление исключает только такие duplicates
  по явному coverage contract; raw `analytic_complete:false`, оба uncertainty
  и provenance остаются сохранены. Регрессия covered/uncovered пройдена.
  Настоящая кнопка создала Front без `accept_incomplete`, Undo/Redo дали 0/1 вид.
- Визуальная проверка выявила пересечение default GUI точки `[50,50]` с основной
  надписью. GUI приведён к существующей engine точке `[50,100]`; это не автоматическая
  компоновка произвольных видов или подбор масштаба, они ещё требуют реализации.
- Изменение высоты native цилиндра 20 → 30 мм при том же размере файла 787 bytes
  обнаружено по SHA. Новая проекция отказала с `stale_source`; refresh дал ревизию 2
  и оставил прежний view `18D` frozen/stale. Undo вернул всю прежнюю model record,
  Redo повторил refresh. Этот smoke использовал отдельный mutable fixture.
- Реальные кнопки менеджера также импортировали STEP и IGES; обе 3D-поверхности
  просмотрены, DXF с двумя model records сохранён (68361 bytes).
  Staged debug CLI нашёл sibling worker при удалённой `CADCRAFTGOST_MODEL_WORKER`,
  создал 8 native entities Front и сохранил DXF 79842 bytes.
  [Протокол и границы](tests/benchmarks/native-model-host-20261121.json).
- Пакеты Linux включают настоящий sibling worker и полные third-party notices.
  Staged release worker выполнил native load/preview; финальные host binaries
  после подключения менеджера ещё нуждаются в повторной release-упаковке.
- Проверка `cargo xtask i18n` запустила workspace ignored-test inventory и
  достигла timeout 120 s; этот запуск не считается успешным gate.
  Новый полный root gate и clipboard/native-reference проверки ещё не приняты.
- Standalone worker прошёл **19 тестов** (6 suites) и all-targets Clippy.
  Проверены реальные upstream named edges, exact arc-length fractions,
  свежая worker-сессия после изменения extrusion distance с rotated/repeated
  placements и явная потеря unverified/neutral references.
  Настоящий stdio `v1.references`/`v1.resolve_point` для native цилиндра вернул
  verified edge и exact point; worker завершился с exit 0 и пустым stderr.
  Compile timeout 900 s и первоначальный неверный import исправлены/учтены;
  успешный запуск после этого выполнен с deadline 1800 s.
- Протокол `cadcraft-model-worker/1` и `cadcraft-references/1` принят в границах
  опубликованной схемы/лимитов. Это не приёмка полной ассоциативности host-размеров:
  `.sc` не содержит UUID, проверенная межревизионная связь ограничена тем же
  canonical path и документом с разрешённым literal size/distance edit.
  Другие перестроения не подбираются по похожей геометрии.
- Новый root gate включает standalone worker девятым шагом (`cargo xtask models`).
  Десять clipboard consumer regressions прошли; Clippy затем выявил wildcard import
  и clone-on-Copy. После сужения imports потребовались явные imports в тестах;
  полный gate повторяется только после исправления этих compile failures.
- Remote `839343c7` Packaging lint: Linux lint прошёл, Windows MSI fixture step
  отказал. Публичная аннотация содержит только exit 1, logs API вернул 403.
  В fixture script добавлен diagnostic trap с экранированием `%`/CR/LF.
  Настоящий PowerShell 7.7.0 разобрал script и выполнил его trap на ошибке:
  exit 1 и корректная GitHub error annotation. Это не MSI runtime-проверка
  и не устранение пока неизвестной причины Windows отказа.
- Полный root запуск после imports прошёл native workspace-тесты и doctests:
  **4029 passed, 0 failed, 26 ignored, 145 result-suites**; i18n inventory выполнен.
  На WASM выявлены два невыводимых типа `channel()` в native-only ветках.
  Каналы теперь создаются только в native cfg-блоках, без ненужной аллокации в web.
  WASM и полный девятишаговый gate проверяются после исправления.
- Дополнительный настоящий `.sc` — полый ступенчатый вал: Ø20 × 100 мм,
  центральная ступень Ø30 × 40 мм на z=30…70 и сквозное Ø6.
  Exact boolean evaluation дал один body, объём `14100π` мм³ и площадь `3432π` мм²
  с невязками менее `1e-6`, центр масс z=50 мм и bbox ±15 × 0…100 мм.
  Изометрия полностью analytic; top имеет только covered uncertainty.
  **Front имеет 4 uncovered uncertain pieces**: обычный host CLI правильно
  отказал с exit 1, не выдал вид за точный. Этот эталон ещё не принят;
  причина HLR и полная инженерная приёмка остаются открытыми.
- После WASM fix отдельный WASM check прошёл (4 прежних upstream warnings).
  Следующий полный native gate выявил недетерминированный timing-test:
  16× groups дали 81,2× wall time (`18,75 ms → 1,522 s`) в параллельном IO suite.
  Повторного запуска «для подтверждения» не было. Reader уже использует hash index
  для имён и memo для dictionary headers; производственный parser не подменён.
  Вместо отношения wall time проверяется реальная работа reader на тех же
  2500/40000 groups: thread-local test-only счётчик посещений header/dictionary tags,
  линейный бюджет и сохранность всех anonymous groups. В production счётчика нет.
  Существующий 20k mixed-case/member roundtrip test сохранён.
- Исходник вала сохранён в `tests/fixtures/models/stepped-shaft.sc`, математический
  эталон/допуски и конкретные unresolved intervals —
  [воспроизводимый benchmark](tests/benchmarks/stepped-shaft-20261121.json).
  Эталон не помечен принятым и не заменён простым цилиндром ради успешного gate.
- Финальный `cargo xtask ci` прошёл **все 9 gate**: native workspace/doctests
  **4029 passed**, feature-off **2 passed**, standalone worker **19 passed**;
  0 failed, 26 прежних ignored. I18n inventory прошёл отдельно; RU всё ещё
  содержит 346 English fallbacks. Сохранились 4 прежних WASM warnings.
  Deterministic parser-work regression также прошёл отдельным запуском.
- Release desktop/CLI и man/completions собраны. STEP, IGES в границах ядра,
  native load, полные сохранённые metadata/assembly placements, SHA change detection
  и read-only 3D preview приняты по проверенным сценариям; этап 4 завершён.
  Это не закрывает точный HLR для всех моделей, ассоциативное перестроение,
  все новые ГОСТ-профили или кроссплатформенную установку.
- Все четыре Linux-пакета и zsync пересобраны с финальными host/worker binaries.
  Packaged CLI без worker override импортировал native `.sc`, создал Front из
  8 native entities и повторно открыл DXF с прежним fingerprint/raw quality.
  Неверный smoke argv `run --open` исправлен на документированный positional FILE;
  успешный import/view не повторялся для подтверждения.
- Настоящая кнопка packaged GUI создала второй Front с default `[50,100]`,
  `accept_incomplete:false`, без пересечения основной надписи в проверенном fixture.
  Сохранённый DXF — 92737 bytes; GUI завершился с exit 0.
  [SHA-256 артефактов и финальный runtime proof](tests/benchmarks/native-model-release-20261121.json).
  Это не системная установка/удаление RPM/DEB и не публикация Alpha.
- `96b356a5` отправлен в `main` по запросу владельца. Новый Packaging lint
  [44932592674](https://github.com/mikiway902/russian-cadcraft/actions/runs/44932592674)
  прошёл Linux lint, но diagnostic trap показал: MSI build завершился,
  отказал отдельный `check-install-policy.ps1` для x64. Его собственная причина
  не наследуется через exit code нового PowerShell process; добавлена такая же
  escaped error annotation в дочерний checker. Его AST и настоящий trap
  проверены PowerShell 7.7.0; Windows COM/MSI здесь не запускался.
- `3c6b7f0b` также отправлен в `main`. [Windows policy run 44933550712](https://github.com/mikiway902/russian-cadcraft/actions/runs/44933550712)
  теперь показал точную причину: сравнение всего `Directory.DefaultDir` с именем каталога.
  Согласно [формату MSI](https://learn.microsoft.com/en-us/windows/win32/msi/directory-table),
  это `short|long:source`, а не только destination name. Checker выделяет target long name;
  запрет установки в оригинальный каталог не ослаблен. PowerShell 7.7.0 выполнил
  настоящий AST проверки: 10 positive/negative target/source/alias cases прошли.
  Повторная проверка Windows COM/MSI выполняется только соответствующим Actions job.
- После push `597d4ccd` [Packaging lint 44934551690](https://github.com/mikiway902/russian-cadcraft/actions/runs/44934551690)
  прошёл, включая настоящий Windows MSI policy job. В [CI 44934551666](https://github.com/mikiway902/russian-cadcraft/actions/runs/44934551666)
  Windows и Ubuntu gate прошли; macOS gate отказал, точная причина пока недоступна:
  публичный Jobs API показывает лишь exit 1, logs требуют token. В command wrapper
  добавлен escaped bounded failure context в публичные annotations, без подавления
  exit status. Настоящий workflow shell block проверен с exit 0/37 и `%`/CR/LF.

## Точные привязки и разрезы — продолжение 22.11.2026

- Принят отдельный пункт «Связать чертёж с исходной 3D-моделью»: сохранены
  canonical source path, SHA/bytes, локальная ревизия, metadata, view parameters
  и provenance. Реальные native/STEP/IGES сценарии сохранения/открытия уже проверены;
  теперь native dimension также прошёл GUI → DXF → CLI reopen с точным audit.
  Это **57/172** принятых пунктов, **115** остаются открытыми.
- Реальная GUI-кнопка загрузила upstream edge tokens и создала native DIMENSION:
  source length **20 мм**, paper length **10 мм**, масштаб **1:2**.
  MOVE вида перепроецировал размер; Undo восстановил геометрию.
  Refresh пометил frozen размер `?` и убрал актуальное measurement;
  Undo refresh восстановил resolved. DXF 85934 bytes повторно открылся CLI,
  сохранив точные anchors и audit. `gost.check` не сообщил ложное
  `dimension.preset.linear_factor`.
  [Доказательства и пределы проверки](tests/benchmarks/native-model-dimensions-20261122.json).
- До начала exact-HATCH cutover прошли 25 model engine regressions,
  8 host validation tests, all-target Clippy engine/IO/UI и сборка desktop/CLI.
  Новые controls русифицированы; остаются 346 прежних English fallbacks.
  Автоматическая B-Rep-перестройка и radial/angular native anchors ещё не приняты.
- Worker `v1.section` использует настоящий half-space boolean и возвращает exact
  cap loops/holes с result-local topology и явной картой source/result bodies.
  Первый запуск обнаружил потерю крышки при слиянии с исходной копланарной гранью.
  Исправление вычитает исходные копланарные области публичным exact planar boolean;
  не удаляет весь merged face по одному history tag. После исправления прошли
  **8 section regressions**, **3 supervised transport tests**, all-target Clippy.
  Ordinary worker geometry/native tests до этого прошли без изменения поведения.
- Host HATCH cutover прошёл отдельную проверку: **23 IO HATCH**, **18 render HATCH**,
  **64 engine HATCH**, **25 model**, **37 spline** cases — всего 167 узких проверок.
  All-target workspace Clippy прошёл; после последних изменений повторно проверены
  затронутые doc/IO/engine targets. Feature-off: 2 regressions и CLI/desktop/web
  checks; WASM прошёл с четырьмя прежними dead-code warnings. Desktop/CLI собраны.
- HATCH хранит native line/arc/ellipse/rational SPLINE edge paths, а не sampled
  vertices. Checked clockwise ellipse angles, NURBS weights/knots, holes,
  nonuniform transforms и legacy bulge → ellipse. Старый тест подмены degree 12
  на degree 10 заменён проверкой сохранения исходной кривой и отказа при missing knots.
  Recreate boundary сохраняет native SPLINE/LINE; Undo и отказ на потере elevated
  spline plane проверены без оставшейся частичной геометрии.
- Настоящий CLI прочитал синтетический ellipse/bore/rational-island HATCH,
  измерил **147.65485463748368 мм²** (эталон `47*pi`, spline area numerical),
  сохранил DXF **18723 bytes**. GUI повторно открыл его: три edge loops `[1,1,4]`,
  без polygon vertices; отверстие осталось незаполненным, островок — видимым.
  [Runtime proof и пределы AREA](tests/benchmarks/native-hatch-boundaries-20261122.json).
- Приёмка one-plane section host/UI описана ниже. Stepped-shaft Front uncertainty,
  полная ассоциативная перестройка и полный ГОСТ не объявлены подтверждёнными
  этими частными сценариями.
- CI `44948553629` на `0a36ff2a93` дал конкретную причину macOS failure:
  `usage::non_unicode_output_names` требовал успеха для raw non-UTF-8 имени,
  хотя файловая система вернула `Illegal byte sequence (os error 92)`.
  Контракт теперь проверяет способность самой файловой системы: сохранение raw
  имени при поддержке либо exit 1 при отказе, без создания lossy Unicode-замены.
  Linux: existing regression прошёл для DXF/SVG/DWG; fmt, CLI all-target Clippy
  и JSON schema parse прошли. Новый macOS run ещё не подтверждён.
- Полный локальный `cargo xtask ci` завершён: **все 9 gates прошли**
  (fmt, workspace Clippy, tests, i18n, assets, layers, WASM, extensions, models).
  В models gate прошли все **27 worker cases**, включая 8 exact-section и
  3 supervised-transport cases. Изменённый CLI filename contract отдельно прошёл
  узкую проверку после правки; успешный локальный gate не заменяет macOS runtime.
- Проверен отдельный upstream HLR candidate `2efc54c2c01b515bb30d7266518e9239bcd093c9`
  (переход HLR на SolidCore). Dependency review: 128 name/version/license pairs
  совпали с принятой closure; изменились 18 git sources. Изолированная locked
  debug-сборка worker прошла. На неизменном stepped-shaft SHA `94344e28…7039`
  реальный stdio load/Front дал прежние **20 uncertain / 4 uncovered**:
  таблица кривых и exact source measures совпали с production. Кандидат не принят;
  production pin и строгий отказ default Front не изменены.

## Точный простой разрез — приёмка host/UI 22.11.2026

- `model_section` DTO и bounded decoder сохраняют directed native cap loops,
  исходный/result-local body mapping, отверстия и boolean recognition provenance.
  ModelHooks.section использует прежний supervised session pool и SHA guards;
  ordinary view/reference поведение не подменяется.
- Engine создаёт HATCH overlay с бумажным шагом 3 мм и сохраняет plane/side/audit.
  Empty и no-intersection возвращаются явно без мутации. Paper-style marker
  запрещает generic и nested scale/rotate/mirror; MOVE/COPY/Undo остаются доступны.
  Exact source dimensions отказывают на удалённых телах и точках вне kept side.
- Узкие проверки: **9 IO section**, **3 IO projection**, **36 engine model**
  cases прошли. Исправлены test import `PolyVertex` и Clippy `is_multiple_of`;
  doc/IO/engine/UI all-target Clippy, fmt, i18n и debug CLI/desktop build прошли.
  Русских fallback strings осталось 346; новые section controls переведены.
- Настоящий GUI: импорт неизменного stepped-shaft, ввод Z=50 и масштаба 1:2,
  нажатие **Создать точный разрез**. Worker вернул настоящую кольцевую крышку
  R15/R3 с двумя exact arc loops, без polygon vertices. Raw quality остаётся
  `analytic_complete:false`: 2 uncertain pieces покрыты по известному upstream
  contract; `accept_incomplete` оставался false, uncovered pieces не скрывались.
- На листе видна штриховка и незаполненное отверстие. MOVE +15 мм и Undo прошли;
  SCALE отказал с полной отменой. EXPLODE дал редактируемые native curves/HATCH;
  AREA на настоящей бумажной крышке: **169.64600329384896 мм²**, эталон `54*pi`.
  Pattern delta 6 мм × scale 0,5 = **3 мм** бумаги.
- DXF **84622 bytes** повторно открыт GUI и CLI; полный section provenance
  совпал с исходным. Реальные Z=101 и касание Z=0 дали `empty` и `no_intersection`
  без новых entities/dirty state; SHA исходника не изменился. GUI завершился 0.
  [Runtime evidence и пределы](tests/benchmarks/native-model-sections-20261122.json).
- Это frozen one-plane geometry, не ступенчатые/ломаные/местные разрезы,
  не uncut assembly exceptions и не ассоциативная перестройка B-Rep.
  Cutting-plane arrows/captions остаются отдельными drafting controls.
- Remote CI `44954122076` на `da4eb88271`: Ubuntu и Windows gates прошли;
  macOS дошёл до другого старого теста `read_file_unicode` и упал при создании
  raw non-UTF-8 fixture (`os error 92`), до вызова проверяемой функции.
  Удалено ненужное создание файла: `read_file` обязан отказать ещё до filesystem IO.
  Сценарий отказа и успешный Unicode-файл сохранены; узкий Linux test и fmt прошли.
  Новый macOS run ещё не подтверждён.
- Полный локальный `cargo xtask ci` на section arc: **9/9 gates прошли**,
  включая extensions feature-off и WASM. Последняя правка filename fixture
  отдельно проверена существующим regression после правки.
- Дополнительная runtime-приёмка: в одном сохранённом DXF **125028 bytes**
  созданы три native вида при масштабах **1:2, 1:1, 1:4**; реальные INSERT scales
  **0,5 / 1 / 0,25**. GUI открыл тот же лист, screenshot трёх размеров прочитан.
  На настоящем EXPLODE-generated HATCH выполнен HATCHEDIT 45° → 30°:
  обе exact loops сохранены; Undo восстановил полный объект. GUI завершился 0.
- Приняты отдельные пункты «Поддержать разные масштабы видов» и
  «Сохранить редактируемость сформированной 2D-геометрии»: **59/172** приняты,
  **113** остаются открытыми. Это не закрывает неопределённости ordinary HLR,
  полную ассоциативность или остальные виды разрезов.

Ни один из этих результатов не закрывает полный новый roadmap. Точные B-Rep-проекции, сборки, нормативные профили, полная русификация, кроссплатформенные установки и выпуск остаются в активном плане.
