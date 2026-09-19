# Design

## Context

Репозиторий пока содержит только OpenSpec/tooling и не имеет manifests, lockfiles или application code. Первый этап должен создать проверяемый runtime-фундамент из `proposal.md`, не вводя camera domain преждевременно. Нормативное поведение описано в `specs/opencv-desktop-runtime/spec.md`; исходное полное задание сохранено в корневом `opencv-poc-spec.md` как reference brief.

Ключевое ограничение — dynamic OpenCV из pinned vcpkg snapshot. Успех dev build недостаточен: границей готовности является installed NSIS application, работающий без build tree и developer environment. Build host обязан иметь Windows/MSVC prerequisites; runtime host не обязан иметь OpenCV, developer tools, network access или camera.

## Goals / Non-Goals

**Goals:**

- Разделить воспроизводимую подготовку native supply, runtime staging, application self-check и installed-bundle verification на явные проверяемые границы.
- Использовать один application-level self-check для CLI и Tauri command, сохраняя transport DTO отдельно от внутренних типов и ошибок.
- Получать проверяемое evidence о происхождении и целостности DLL как при staging, так и после установки.
- Оставить структуру приложения расширяемой следующим camera-pipeline change без фиктивных camera abstractions в первом этапе.

**Non-Goals:**

- Camera enumeration, `VideoCapture`, profiling, preview, recording реального потока и state machine камеры.
- `ffprobe`, аппаратные прогоны и выбор production codec/container.
- macOS/Linux packaging, code signing, auto-update и production installer hardening.
- shadcn-vue/design system: первый экран минимален и не требует новой UI dependency.

## Decisions

### 1. Границы модулей и направление зависимостей

Rust часть разделяется на независимые от Tauri types/use-case и platform adapters:

```text
CLI startup ---------+
                     v
               SelfCheckService ---> OpenCvAdapter
                     |                    |
Tauri command -------+                    +--> module inspection / filesystem
                     |
                     +--> SelfCheckReport + typed errors

Vue diagnostic UI --> versioned transport DTO only
```

`SelfCheckReport`, check statuses и exit-code mapping принадлежат application boundary. OpenCV calls, process-module enumeration, hashing и filesystem являются adapters. Tauri command валидирует request и единожды преобразует внутренние ошибки в безопасный versioned DTO; Vue не получает `opencv::Error`, Rust paths или `Debug` representation.

CLI arguments разбираются до создания Tauri window. При `--self-check --json <path>` вызывается тот же `SelfCheckService`, report записывается через temporary sibling file с последующим atomic replace/rename, после чего process завершается согласованным exit code. Обычный startup строит Tauri application и предоставляет минимальную command surface только для запуска self-check и чтения безопасного report.

Альтернатива — отдельные CLI и UI implementations — отклонена из-за риска расхождения checks. Выполнение проверки во frontend отклонено, потому что browser context не владеет native runtime.

### 2. Repository-local native supply

`vcpkg.json` фиксирует requested port/features, а `vcpkg-configuration.json` — immutable Git baseline. `.tools/vcpkg/` и `.vcpkg_installed/x64-windows/` являются локальными ignored roots. `bootstrap-opencv.ps1`:

1. Проверяет OS/architecture, PowerShell, Git, MSVC и Windows SDK.
2. Создаёт либо проверяет vcpkg checkout на exact peeled commit; существующее несовпадение считается ошибкой, а не поводом автоматически перезаписать пользовательский checkout.
3. Запускает manifest install с explicit triplet/roots.
4. Проверяет установленный port version/features и сохраняет environment evidence.
5. Строит runtime manifest и валидирует licenses.

`run-with-opencv.ps1` на каждом вызове вычисляет абсолютные пути от repository root и задаёт переменные только дочернему process. Глобальные environment и registry не меняются.

Альтернативы: системный OpenCV не даёт воспроизводимости; static triplet расходится с подтверждённым supply contract; автоматический fallback на другую revision скрывает изменение проверяемого runtime. Все три отклонены.

### 3. Lockfiles и версии application stack

Tauri 2, Vue, Bun и Rust выбираются в момент scaffold по совместимой поддерживаемой комбинации, после чего их точные значения становятся данными manifests/lockfiles и environment evidence. Для этого change зафиксированы Rust crate `tauri` 2.11.5 и соответствующий latest stable `@tauri-apps/cli` 2.11.4; Tauri 3 alpha не используется. Реализация не использует плавающие версии и не обновляет автоматически созданные lockfiles. `packageManager` фиксирует Bun; `rust-toolchain.toml` фиксирует toolchain и target; Cargo dependency resolution фиксирует `Cargo.lock`.

Это осознанно не копирует версии из исследовательского текста: до появления manifests они являются предположениями, а не фактом сборки.

### 4. Runtime manifest как allowlist

Manifest является schema-versioned allowlist, а не glob-рецептом. Генератор начинает с release OpenCV DLL, требуемых выбранными modules/features, рекурсивно разрешает их non-system PE imports внутри release `x64-windows` install tree и записывает точный dependency closure. Debug DLL исключаются. Для каждого файла сохраняются source, bundle-relative destination, SHA-256, PE machine, purpose и license/notice path.

Определение imports использует доступный MSVC PE inspection tool с зафиксированной версией либо repository-owned parser, если вывод tool нельзя обработать детерминированно. System DLL классифицируются отдельно и не копируются из Windows directories. После сборки closure проверяется повторно уже от release executable, чтобы обнаружить native dependency, не видимую на этапе bootstrap.

Staging очищает только свой заранее определённый output directory и копирует manifest-listed files после повторной проверки hash/architecture. Tauri bundle resources формируются из staging, а не напрямую из `.vcpkg_installed`.

Альтернатива `opencv_*.dll`/копирование всего `bin` отклонена: она не доказывает состав runtime, может захватить debug/лишние DLL и не обеспечивает license audit.

### 5. Self-check как последовательность независимых проверок

`SelfCheckService` выполняет checks последовательно и сохраняет отдельный status/error code каждого этапа:

1. Загрузка OpenCV, version и hash/summary `getBuildInformation()`.
2. Проверка registry backends `MSMF`, `DSHOW`, `FFMPEG`.
3. JPEG encode/decode round-trip.
4. Открытие `VideoWriter` с explicit `CAP_FFMPEG`, `MJPG`, AVI и проверка `getBackendName()`.
5. Запись/finalize 30 synthetic frames и повторное декодирование count/dimensions.
6. Перечисление загруженных process modules, normalised canonical paths и SHA-256.

Отказ раннего этапа не должен порождать заведомо небезопасные зависимые calls, но report по возможности сохраняет результаты уже выполненных и skipped checks. Temporary output получает уникальное имя и не считается evidence recording. Cleanup errors отражаются в report.

Ожидаемые ошибки моделируются локальными enums (`RuntimeSupplyError`, `SelfCheckError`, `ReportWriteError`) с preserved source chain внутри Rust. На CLI boundary они переходят в exit codes `10`, `11`, `12`, `13`, `14`, `20`; в Tauri boundary — в стабильный public code/message. Production paths не используют `panic`, `unwrap()` или `expect()` для I/O, OpenCV и входных данных.

### 6. Windows module provenance

Для фактически загруженных modules Rust adapter использует Windows process-module API и canonical file paths, после чего считает SHA-256 и сопоставляет module с runtime manifest. System modules разрешены только из canonical Windows system directories; manifest-controlled OpenCV/codec modules — только рядом с installed executable/в объявленном bundle location.

Проверка выполняется внутри installed process, поэтому она не полагается только на статический import analysis. `verify-bundle.ps1` валидирует self-check report повторно и проваливает run при origin/hash mismatch.

### 7. Bundle и installed verification

Tauri создаёт только x64 NSIS bundle с offline WebView2 и согласованной VC runtime policy. Stable config schema связки `tauri` 2.11.5 / CLI 2.11.4 ещё не содержит документированные в более новом reference поля `build.windows.staticVCRuntime` и `bundle.windows.bundleVCRuntime`, поэтому application build явно проверяет effective `STATIC_VCRUNTIME=true`, который выставляет Tauri CLI, а требуемые dynamic OpenCV/FFmpeg зависимости от MSVC runtime включаются app-local в runtime manifest/staging вместе с hash, architecture и license metadata. Это сохраняет наблюдаемое требование самодостаточного installer без перехода на Tauri 3 alpha или непубликуемый Git snapshot. `verify-bundle.ps1` принимает или однозначно обнаруживает один release installer, сверяет его hash/size, устанавливает в изолированное current-user test location (либо документированный clean VM), находит фактический executable и запускает его с очищенными `VCPKG_*`/`OPENCV_*` и системным-only `PATH`.

Verification разделяет этапы `manifest`, `install`, `headless_self_check`, `module_provenance`, `gui_smoke`, `uninstall`; каждый сохраняет status/exit code. Ошибка позднего этапа не стирает evidence ранних этапов. GUI smoke подтверждает startup/controlled shutdown, но не обращается к камере.

Rollback для change прост: удалить generated application files и repository-local ignored tool/build roots; пользовательские глобальные настройки не меняются. Test installation удаляется verifier, а при failed uninstall её path сохраняется для ручной очистки.

### 8. Tests и evidence

Rust unit/integration tests используют temporary directories и injectable adapters для failure cases: отсутствующий backend, codec/writer failure, count mismatch, manifest/hash/architecture/origin mismatch и report serialization. Отдельный integration test с реальным packaged OpenCV выполняет synthetic round-trip после bootstrap. Frontend tests проверяют versioned DTO mapping, loading/error state и отсутствие native error leakage.

`bun run test` оркестрирует frontend `typecheck`, `lint`, `test:unit`, `build` и Rust `fmt --check`, `clippy` с warnings-as-errors и workspace tests. Hardware и installer smoke не маскируются unit tests и запускаются отдельным `verify:bundle`.

Evidence использует schema-versioned JSON и небольшие logs. Installer и temporary AVI не коммитятся; рядом сохраняются canonical path, size и SHA-256. Secret-like environment values не сериализуются.

## Risks / Trade-offs

- **[vcpkg build очень долгий и объёмный]** → bootstrap идемпотентен, использует repository-local cache/root и не повторяет install при полном совпадении contract.
- **[Фактический dependency closure отличается между vcpkg revision и build configuration]** → manifest строится из конкретного install tree и повторно проверяется от release executable перед bundle.
- **[OpenCV backend присутствует в build information, но неработоспособен]** → первый change проверяет только наличие backend и synthetic writer; реальный capture остаётся обязательным отдельным change.
- **[Module enumeration или canonicalization даёт ложный результат]** → хранить raw и canonical path в internal evidence, тестировать path normalization и рассматривать любой неоднозначный non-system origin как failure.
- **[Offline WebView2 существенно увеличивает installer]** → размер фиксируется в evidence и принимается для PoC; оптимизация installer size не входит в этот change.
- **[GUI smoke сложно автоматизировать на headless host]** → verifier явно различает автоматический process startup check и ручной/VM evidence; отсутствие обязательного GUI smoke не превращается в pass.
- **[Current-user NSIS uninstall оставляет файлы]** → uninstall result и residual path фиксируются, verification завершается ошибкой, ручная очистка выполняется по сохранённому exact path.

## Migration Plan

1. Создать application scaffold и lockfiles, не меняя внешние пользовательские данные — миграции данных отсутствуют.
2. Подготовить pinned native environment и manifest.
3. Ввести общий self-check и automated tests.
4. Добавить staging, NSIS bundle и installed-app verifier.
5. Зафиксировать successful clean-checkout/installed evidence; только после этого начинать camera-pipeline change.

При неуспехе supply contract change не архивируется: manifests/scripts корректируются только через пересогласование спецификации. Rollback не требует migration — удаляются созданные application файлы и точечно указанные repository-local generated roots; глобальное состояние машины не затрагивается.
