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

## Проекционные связи — приёмка 22.11.2026

- Добавлена `gost.model.projected_view`: first-angle top/bottom/left/right,
  наследование basis/масштаба/листа, бумажный промежуток и явный placement audit.
  Native curves не пересчитываются по display mesh. Ссылки образуют bounded DAG;
  исходные section views не принимаются за неразрезанную модель.
- MOVE родителя переносит потомки; потомок движется только по свободной оси.
  Несогласованные transforms отклоняются атомарно. Удаление родителя сохраняет
  геометрию и явный missing_parent; Undo возвращает активные связи.
  Clipboard переносит только реально скопированные связи без placeholder rebinding.
- Узкие engine/model проверки: **59/59 прошли**, all-target engine/UI Clippy,
  fmt, i18n и desktop/CLI debug build прошли до runtime-приёмки.
- Настоящий GUI импортировал неизменный native cylinder R5/H20 и создал Top
  реальным нажатием кнопки. Front [80,160], Top [80,135], Right [50,160]:
  вид сверху расположен снизу, справа — слева; промежуток 20 мм.
  Screenshot листа прочитан. Raw uncertain/covered quality не переписывается.
- На настоящем Drawing: MOVE родителя +[10,15] переместил все три INSERT;
  Undo восстановил полную геометрию. Top MOVE [0,-5] разрешён, [3,0] и SCALE 2
  отклонены с неизменными entities/undo/dirty. ERASE дал двум детям missing_parent
  без потери геометрии; Undo восстановил родителей.
- COPYCLIP/PASTECLIP одного ребёнка создал отдельную frozen копию: parent:null,
  original_parent:184, status:missing_parent, хотя исходный родитель остался
  в том же документе. DXF **116028 bytes** повторно открыт GUI и реальным CLI:
  полный model list, provenance, revision и orphan audit совпали. SHA исходника
  не изменился; GUI завершился 0.
  [Сценарий и пределы](tests/benchmarks/native-model-projections-20261122.json).
- Runtime обнаружил английский completion status при русском UI: model tick
  выполнялся до locale scope. Completion перенесён в scope фактически показанного
  языка; CJK сохраняет существующий fallback при отсутствии шрифта.
  Исправленный desktop повторно открыл DXF; реальное создание ребёнка показало
  **«Проекционно-связанный вид создан.»** (screenshot прочитан).
  Настоящие Undo/Redo создания восстановили полный model list; GUI завершился 0.
- Remote CI `44958624765` на `8f865d4de8`: **macOS и Ubuntu прошли**.
  Windows выявил bitwise сравнение hatch delta 6.000000000000001 с 6.0
  после поворота/обратного поворота DXF. Проверка теперь допускает <1e-12 мм
  только для transformed delta; прочие поля и точные границы сравниваются целиком.
  Это исправление тестового численного контракта, не изменение геометрии.
- Полный локальный `cargo xtask ci`: **9/9 gates прошли**, включая новый
  completion locale regression, feature-off и WASM. Обновлённый hatch roundtrip
  отдельно прошёл узкую проверку; debug desktop после locale fix собран и запущен.
- Принят пункт «Поддержать проекционные связи между видами»: **60/172** приняты,
  **112** остаются открытыми. Это не означает автоматическую перестройку B-Rep
  после изменения источника или полное соответствие всем требованиям ГОСТ.
- Полная source-change перестройка и stepped-shaft HLR остаются открытыми;
  cylinder placement smoke не подменяет канонический инженерный benchmark.

## Геометрические сечения — продолжение 22.11.2026

- `gost.model.section` получил `presentation:cutaway|detached|imposed`.
  Cutaway сохраняет прежнее поведение. Два новых режима создают только точные
  cap boundaries и HATCH, не показывая геометрию за плоскостью. Полный исходный
  projection payload всё равно валидируется; raw quality/provenance сохраняются,
  переключатель не обходит отказ при неполной геометрии.
- По ГОСТ 2.305-2008 §7.2: вынесенный контур — `GOST-OUTLINE`, наложенный —
  `GOST-THIN`, без обрезки или фоновой заливки исходного вида. Несколько cap-областей
  одного тела консервативно отклоняются для проверки §§7.9–7.10; это не
  автоматическая классификация всех multi-cap случаев как несвязных.
  Привязки размеров дополнительно ограничены плоскостью и рассечёнными телами.
- Engine model subset: **61/61 tests прошли**, включая оба новых consumer
  regressions — native curve types/holes/layers, отсутствие retained curves,
  исходный вид без изменений, Undo/Redo, полный DXF audit и atomic refusals
  для malformed provenance, raw unsupported quality и disconnected regions.
  Engine/UI all-target Clippy, fmt, i18n и desktop/CLI debug build прошли.
- Реальный GUI на неизменном canonical stepped-shaft SHA `94344e28…7039`
  создал оба режима реальными кнопками при Z=50 и масштабе 1:2.
  Наложенное сечение совмещено с настоящим cutaway; все прежние native entities
  остались неизменными. Оба русских completion status и лист прочитаны.
- EXPLODE каждого режима дал ровно **CIRCLE + CIRCLE + HATCH**:
  основной/тонкий контур, радиусы **7.500000000000003 / 1.4999999999999996 мм**,
  две exact arc loops без polygon vertices, незаполненное отверстие,
  pattern delta 6 мм × scale 0,5 = **3 мм** бумаги. Undo восстановил весь Store.
  DXF **107525 bytes** открыт GUI и CLI; полные model lists/provenance совпали,
  SHA исходника не изменился, GUI завершился 0.
  [Доказательства](tests/benchmarks/native-model-sections-20261122.json).
- Финальный полный `cargo xtask ci`: **9/9 gates прошли**, desktop/CLI собраны.
  После уточнения multi-cap диагностики тест использует действительно разнесённую
  вторую область, а не дубликат; узкий regression и fmt прошли.
  Пересобранный GUI показал кнопку **«Создать точное сечение»**; реальная новая
  геометрия содержит предупреждение §§7.3–7.9. DXF **117425 bytes** повторно
  открыт GUI и CLI с полным равенством данных и предупреждения; GUI завершился 0.
- Это пока не закрывает пункт вынесенных/наложенных сечений: автоматические
  секущие обозначения/оси, условность осевых отверстий §7.9 и source rebuild
  §7.11 ещё не приняты. Предупреждение сохраняется в новых видах.
  Приёмка roadmap остаётся **60/172**, открыто **112**.

## Автоматические центровые оси сечений — 22.11.2026

- Круговые внешние cap boundaries теперь получают центровые оси по умолчанию,
  без дополнительной команды или флага. Тот же native-stroke builder обслуживает
  обычную `gost.center`; подсчёт числа элементов не создаёт временную геометрию.
  Совпадающие точные центры объединяются с выбором большего радиуса; аппроксимации
  окружности из SPLINE/эллипса нет. Оси cap остаются frozen вместе с видом.
- Проверены текст и изображения таблицы 1 ГОСТ 2.303-68, §§7–11:
  длинный штрих **5–30 мм**, полный промежуток **с точкой 3–5 мм**, а не такой
  промежуток с каждой стороны точки. Исправлен прежний алгоритм, который для
  окружности D20 мог превращать центровые линии в сплошные.
  Рисунок симметричен; пересечение и окончания приходятся на штрихи.
- Для размеров <12 мм — сплошная тонкая. По умолчанию выход 3 мм, около порога
  D12–15 — до 4,5 мм для размещения полного рисунка. Явно заданный невозможный
  выход отклоняется атомарно; automatic policy переживает DXF и смену радиуса.
- Узкие проверки: **71 GOST / 62 model cases прошли**, engine all-target
  Clippy и debug desktop/CLI build прошли. Новые consumer regressions проверяют
  реальные dash/gap размеры, переход через 12 мм после DXF reopen, исходные exact
  cap loops без изменений и модельные оси разных бумажных масштабов.
- Настоящий GUI создал две секции canonical shaft без axis options:
  D15 мм на бумаге — **12 native GOST-CENTER lines**,
  D7,5 — **4 native GOST-THIN lines**. У большого вида положительная полуось:
  [0;2,5], [3,75;4,25], [5,5;10,5] мм; центральный/конечный штрих **5 мм**,
  полный dot interval **3 мм**, выход **3 мм**. Общий и увеличенный screenshots
  прочитаны; короткие dot strokes видны, данные проверены через настоящий EXPLODE.
- Реальная `gost.center` на D12 дала выход **4,5 мм**; explicit 3 мм отказал
  с неизменными entities/undo/dirty. DXF **94332 bytes** повторно открыт GUI/CLI:
  весь model state совпал; изменение D12→D11 после reopen дало сплошные тонкие
  оси с выходом 3 мм. Undo прошёл, GUI завершился 0.
- Полный локальный `cargo xtask ci` на автоматических осях: **9/9 gates прошли**,
  включая WASM и отключённое расширение. Новых runtime-install зависимостей нет.
- Remote GitHub Actions для pushed `2e9b6e1025`: Linux и macOS завершились
  успешно ([run 45024874350](https://github.com/mikiway902/russian-cadcraft/actions/runs/45024874350));
  Windows на последнем просмотре ещё выполнялся. Для предыдущего `b8d7ac25ec`
  Windows и macOS наблюдались `success` (run 45019728611); это не приёмка установщиков.
- Общая нотация/симметрия некруговых сечений, §7.9 и source rebuild ещё открыты.
  Приёмка roadmap остаётся **60/172**, открыто **112**.

## Автоматические обозначения видов и сечений — 22.11.2026

- Проверены текст и изображение §5.9 ГОСТ Р 2.316-2023, издание февраля 2025
  с поправкой ИУС 9-2024: общий алфавит независимо от количества листов,
  исключённые буквы и переход к цифровым нижним индексам.
  Отдельно изучены публичные справки АСКОН v24 об автосортировке и её настройках.
- `gost.section`, `gost.view` и создание `gost.section.view` без `label`
  выбирают первое свободное обозначение по native TEXT/MTEXT модели, всех листов
  и определений блоков. После А…Я / A…Z с нормативными исключениями —
  А₁, Б₁, … / A₁, B₁, …, затем индекс 2 и далее.
  Индексированные надписи — обычный редактируемый MTEXT, без взрыва в линии.
- Сканирование ограничено 100000 сущностями и 8 МиБ текста; отказ не создаёт
  частичных штрихов и не заменяет результат повторной «А». Для обычных букв —
  битовая маска без выделения памяти на подпись; для встреченных индексов —
  отдельные битовые маски. На каждом UI frame документ не сканируется.
- Узкие проверки: **74 GOST / 11 legacy drawing / 11 GOST UI cases прошли**;
  font regression проверяет реальную геометрию нижнего индекса и plain text.
  Новый consumer regression покрывает оба алфавита до индекса 2, DXF, Undo,
  форматированный MTEXT, другой лист и атомарный отказ по лимиту текста.
- Настоящий GUI 18067 создал А через меню секущей плоскости и Enter, затем Б
  через меню обозначения вида и Enter. Форматированный «В — В (1:2)» и Г
  зарезервировали буквы: следующая надпись — Д; Undo снова освободил Д.
  После 24 букв автоматически получено «А₁—А₁».
- Пустое поле в настоящем GUI редактора многогранного разреза дало «Б₁—Б₁»;
  native source box **40×20×30 мм**, сечение Z15, бумажный масштаб 1:2.
  Prompt/editor/created screenshots прочитаны: русский текст, уменьшенные
  нижние индексы и реальные штрихованные контуры видны.
- На rebuilt GUI 18068 новый разрез получил В₁; вычисленное обозначение
  вернулось в поле редактора. Следующее нажатие «Применить вид» со сменой
  Z15→Z20 сохранило В₁. Явное удаление обозначения пережило последующую смену
  плоскости; два изолированных Undo восстановили весь canonical view state.
- Итоговый DXF **80729 bytes** открыт настоящими GUI и CLI: полный набор native
  paper entities и metadata обоих многогранных видов совпал. Следующее свободное
  обозначение в обоих процессах — **Г₁**; GUI Undo восстановил исходные entities.
  Первый DXF roundtrip нормализовал `TEXT.insert` для aligned TEXT, не меняя
  `alignPt` и остальных полей; MTEXT сохранился полностью. После нормализации
  повторный roundtrip дал полное равенство. Оба GUI завершились 0.
- Полный `cargo xtask ci` на этой реализации: **все 9 gates прошли** за 488,61 с,
  включая workspace tests/Clippy, i18n, WASM, disabled-extension и реальный worker.
  Существующие четыре WASM dead-code warnings не скрывались.
- Общая группа связанных обозначений, автоматическая перенумерация и приоритет
  типов КОМПАС ещё не реализованы; самостоятельные пользовательские надписи
  не переписываются. Это не закрывает всю нотацию точных B-Rep-сечений,
  §7.9/§7.11 или задачу вынесенных/наложенных сечений.
  Приёмка roadmap остаётся **60/172**, открыто **112**.

## Фактический масштаб в обозначениях — 22.11.2026

- При создании/перестроении обозначенного многогранного вида/разреза после
  обозначения автоматически печатается фактическое отношение: «А—А (1:2)»,
  «Б (2,5:1)». Основание — §5.11 ГОСТ Р 2.316-2023; прочитан текст издания
  февраля 2025 с поправкой ИУС 9-2024. Величина берётся из геометрического
  `View.scale`, не из model DIMSCALE и не из текстового title-block SCALE.
  Явно печатается и 1:1: изменение реквизита не скрывает отличный масштаб.
- Узкие **11 legacy drawing / 74 GOST cases**, engine Clippy и debug build прошли.
  Контрактный тест проверяет 4:1, 2,5:1, 1:1, 1:2, 1:10, размерные измерения
  и бумажную высоту; fixture использует основной масштаб 1:5 и DIMSCALE 8.
- Настоящий GUI 18069, native box 40×20×30 мм: через редактор создан разрез
  «А₁₂—А₁₂ (1:2)», затем тем же редактором `scale` изменён 2→0,4:
  надпись стала «А₁₂—А₁₂ (2,5:1)», native paper rectangle — 100×50 мм.
  Оба screenshots прочитаны; индекс уменьшен и расположен ниже, масштаб
  печатается обычной строкой с десятичной запятой.
- Смена title-block SCALE на 1:5 не изменила фактический масштаб вида/надпись.
  Изменение исходника до 48×20×30 мм перестроило контур до **120×50 мм бумаги**,
  сохранив 2,5:1. EXPLODE показал native MTEXT высотой **7 мм**;
  Undo восстановил все paper entities. Финальный screenshot прочитан.
- DXF **70140 bytes** открыт GUI и настоящим CLI: reopened paper entity lists
  и canonical metadata вида совпали; native lowered MTEXT caption сохранился.
  Regenerated title attributes при первом DXF read нормализовали native `props`
  и baseline `insert` при неизменных `alignPt`; полное исходное entity equality
  до этой нормализации не заявляется. GUI завершился 0.
- Полный локальный `cargo xtask ci`: **9/9 gates прошли** за 426,40 с,
  включая workspace tests/Clippy, i18n, WASM, disabled extension и model worker.
- Это пока caption многогранного механизма, не общие подписи B-Rep и не
  автоматическая связь секущей плоскости с исходным видом. Задача полных
  вынесенных/наложенных сечений остаётся открытой; приёмка **60/172**.

## Стрелки сечений без букв — 22.11.2026

- Для несимметричных наложенных сечений и сечений в разрыве (§7.5
  ГОСТ 2.305-2008) `gost.section` поддерживает `label:null`.
  Интерактивный путь — `NoLetters` вместо обозначения; Enter сохраняет
  прежний автоматический выбор свободной буквы.
- Разомкнутые концы и стрелки вынесены в один paper-space builder с optional
  lettering и прямым callback emitter. Обычные `gost.section`/`gost.view` используют
  его стрелки без промежуточных arrow vectors и временных handle strings.
  Без букв не сканируется алфавит и не читается высота текста;
  guard неиспользуемого слоя `GOST-TEXT` не блокирует построение.
- Узкие **75 GOST / 11 legacy drawing cases**, engine/UI Clippy, i18n и debug build
  прошли. Новый consumer case проверяет реальные штрихи/стрелки, заблокированный
  текстовый слой, свободную А после построения и атомарный отказ на `caption`.
- Настоящий GUI 18070: меню секущей плоскости → точки (40;180)/(140;180) →
  `nOlEtTeRs`. Получены native **4 LINE + 2 SOLID**, без TEXT/MTEXT.
  Концевые штрихи **10 мм**, точки стрелок (42,5;180)/(137,5;180),
  головки **5 мм**, включённый угол **15°**. Prompt, лист и увеличенные
  стрелки прочитаны на screenshots; русский prompt объясняет `NoLetters`.
- Ошибочный `label:null + caption` сохранил entities/undo/dirty.
  Следующая обычная надпись получила А, Undo вернул исходные entities.
  JSON `reverse:true` создал обратные стрелки при заблокированном `GOST-TEXT`.
- DXF **66216 bytes** открыт GUI и настоящим CLI: полные native paper entity lists
  совпали без нормализующих исключений. GUI завершился 0.
- Полный `cargo xtask ci`: **9/9 gates прошли** за 412,90 с, включая
  workspace tests/Clippy, i18n, WASM, disabled extension и реальный model worker.
- Это ручная нотация, не генератор сечения или автоматическая классификация
  симметрии; для симметричного сечения §7.3 не требует таких стрелок.
  Общая parent-view связь, §7.9 и source rebuild остаются открытыми;
  весь пункт вынесенных/наложенных сечений не принят: **60/172**, открыто **112**.

## Source-bound обозначения точных сечений — 22.11.2026

- `gost.model.section` принимает `source_view` и optional `label`. Исходный
  ordinary view должен соответствовать модели/revision/fingerprint, находиться
  на том же mm-листе и показывать плоскость с ребра; INSERT положительный,
  равномерный, без поворота/зеркала/обёртки. Без `source_view` прежний
  geometry-only контракт сохранён; `label` без исходного вида отклоняется.
- Общий `CuttingPlane` строит штрихи/стрелки в paper mm с переводом в native
  source block с учётом base/масштаба. Буквы — свободные либо явно заданные,
  включая нижние индексы; target caption использует общий actual-ratio formatter.
  Для несимметричного наложенного режима — стрелки без букв/caption.
- `section.notation` v1 хранит активный source, original audit и исходный масштаб.
  MOVE источника переносит его marks, MOVE результата — его caption, без
  дополнительного наследования перемещения между ними. Scale/rotate/mirror,
  смена листа, дублирующий INSERT и вложенная обёртка активного источника
  отклоняются атомарно. Event callbacks не вызывают worker/source I/O.
- Удаление результата очищает только его marks. Delete/EXPLODE источника даёт
  `missing_source`; сечение/caption сохраняются. Exploded native primitives
  теряют association tags, не видимую геометрию. Без восстановления по совпавшему
  handle, ближайшему виду или `original_source_view`.
- Clipboard remap использует только реально переданные native views:
  whole pair → copied source, section-only → missing, source-only → удаление
  только скопированных orphan marks. Snapshot placeholders не активируют связь.
  Восемь consumer regressions охватывают COPY, clipboard/system DXF,
  cross-document collisions, повторное копирование, Undo/Redo.
- Дополнительные девять consumer cases проверяют бумажные размеры при source
  scales (model mm/paper mm) **0,25/2/10** и ненулевом base, indexed caption, MOVE, delete/EXPLODE,
  locked unused text layer, DXF, face-on refusal, alias/wrapper rollback.
  Native model group — **79 cases**; engine/UI Clippy и i18n прошли.
  Финальный `cargo xtask ci` после уточнения UI: **9/9 gates**, **308,84 с**;
  workspace tests/Clippy, i18n, WASM, disabled extension и real worker прошли.
- Реальный GUI 18071: IGES box **10×20×30 мм**, Front **2:1**,
  Z=15/+Z, detached **1:2**, source `184`, section `189`, label А12:
  настоящие source selector, поля и кнопка создания. Получены native
  **4 LINE + 2 SOLID + 2 MTEXT** marks; штрихи 10 мм, наконечники 5 мм/15°,
  tips (47,5;160)/(92,5;160), высота букв/caption 7 мм. Caption
  **А₁₂—А₁₂ (1:2)**, target rectangle **5×10 мм**, HATCH 3 paper mm.
- GUI imposed branch сохранил `label:null` даже после А12 в скрытом поле:
  source-scaled Z=20 section — **4 GOST-THIN LINE + HATCH**, без текста.
  Это проверка механизма несимметричного режима, не доказательство симметрии
  данного прямоугольника или полной нормативной корректности наложения.
- Реальные MOVE, scale refusal, source/target EXPLODE, pair COPY и Undo
  сохранили ожидаемые native entities/metadata; ошибочный scale не изменил
  entities/undo/dirty. Проекционный top-потомок создан при уже существующих
  marks; MOVE семьи перенёс его, но не отдельное сечение.
- DXF **108818 bytes**: GUI reopen сохранил полные paper entity lists и metadata
  четырёх views, обе source associations и MOVE/Undo. Final GUI 18072 подтвердил
  повторное открытие и уточнённый русский текст «несимметричные».
  Обе GUI-сессии завершились **0**. Screenshots формы, листа, MOVE,
  наложения, final guidance и reopened sheet прочитаны.
  Standalone CLI завершился 0: полные reopened paper entities и metadata
  совпали с GUI; source EXPLODE дал **12 LINE + 4 SOLID + 2 MTEXT** для
  двух групп, обе связи стали missing; Undo восстановил entities/metadata.
  Фактический HATCH: native delta 6 × scale 0,5 = **3 мм**.
- Никакой автоматической классификации симметрии, §7.9 или B-Rep rebuild
  по §7.11 этим не принято. Canonical shaft Front остаётся **20 uncertain /
  4 uncovered**; его качество не подавлялось. Весь пункт вынесенных/наложенных
  сечений остаётся открытым: **60/172**, открыто **112**.

Ни один из этих результатов не закрывает полный новый roadmap. Точные B-Rep-проекции, сборки, нормативные профили, полная русификация, кроссплатформенные установки и выпуск остаются в активном плане.
