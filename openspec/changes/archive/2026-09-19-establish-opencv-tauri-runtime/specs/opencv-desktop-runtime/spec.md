# Spec Delta

## Purpose

Обеспечить воспроизводимый Windows runtime для Tauri/OpenCV и доказать до подключения реальной камеры, что установленное приложение использует только поставленные native зависимости и проходит детерминированную самопроверку codec/writer pipeline.

## ADDED Requirements

### Requirement: Воспроизводимая подготовка native dependencies
Система MUST предоставлять идемпотентную команду `./tools/bootstrap-opencv.ps1`, которая из чистого checkout подготавливает repository-local native environment со следующими обязательными параметрами: Rust target `x86_64-pc-windows-msvc`, vcpkg triplet `x64-windows`, vcpkg peeled commit `9e593bb18ea69cc5095e012465dcd675a822ed0d`, port `opencv4` 4.12.0#7, `default-features = false` и features `dshow`, `ffmpeg`, `jpeg`, `msmf`, `thread`. Команда MUST проверять prerequisites и фактические версии, не менять глобальные environment variables или `PATH` и сохранять machine-readable environment evidence без secrets.

#### Scenario: Bootstrap из чистого checkout
- **WHEN** разработчик запускает `./tools/bootstrap-opencv.ps1` на поддерживаемой Windows x64 build-машине с установленными prerequisites и без заранее заданных `VCPKG_*` или `OPENCV_*`
- **THEN** exact vcpkg revision и manifest dependencies устанавливаются в repository-local каталоги, а evidence содержит проверенные версии, пути и параметры supply contract

#### Scenario: Повторный bootstrap
- **WHEN** bootstrap повторно запускается поверх уже корректно подготовленного repository-local environment
- **THEN** команда подтверждает соответствие revision, triplet, port version и features без неявного обновления зависимостей

#### Scenario: Несовпадающая ревизия vcpkg
- **WHEN** repository-local vcpkg checkout существует, но `git rev-parse HEAD` не равен требуемому peeled commit
- **THEN** bootstrap завершается ошибкой до сборки или установки OpenCV и сообщает ожидаемую и фактическую revision

#### Scenario: Отсутствует build prerequisite
- **WHEN** отсутствует совместимый MSVC x64 toolset, Windows SDK, PowerShell 7, Git или иной обязательный prerequisite
- **THEN** bootstrap завершается с ненулевым exit code и называет отсутствующий prerequisite, не выдавая environment за подготовленный

### Requirement: Закреплённый application toolchain
Приложение MUST фиксировать фактически выбранные совместимые версии Rust, Bun, Tauri, Vue и JavaScript/Rust dependencies в manifests и lockfiles. Документированный clean-checkout flow MUST использовать frozen/locked dependency resolution и MUST состоять из реальных команд `./tools/bootstrap-opencv.ps1`, `bun install --frozen-lockfile`, `bun run test`, `bun run app:build` и `bun run verify:bundle`; `bun run app:dev` MUST запускать приложение через тот же process-local OpenCV environment wrapper.

#### Scenario: Воспроизводимый запуск команд
- **WHEN** команды выполняются в документированном порядке в новых shell processes
- **THEN** каждая команда самостоятельно вычисляет пути от repository root и не зависит от environment variables предыдущего shell

#### Scenario: Lockfile расходится с manifest
- **WHEN** frozen install обнаруживает, что lockfile отсутствует или не соответствует manifest
- **THEN** установка завершается ошибкой вместо неявного изменения версий

#### Scenario: Единая команда тестирования
- **WHEN** выполняется `bun run test`
- **THEN** запускаются frontend проверки и Rust fmt, clippy и tests, а failure любого обязательного этапа возвращает ненулевой exit code

### Requirement: Проверяемый runtime manifest
После подготовки native dependencies система MUST формировать schema-versioned `runtime/manifest.json`, перечисляющий точное имя, SHA-256, PE architecture, repository-local source, bundle destination, назначение и license/notice path каждой поставляемой OpenCV, FFmpeg и transitive DLL. Release staging MUST копировать только manifest-listed runtime files и MUST проверять их hash и `x64` architecture.

#### Scenario: Корректный runtime manifest
- **WHEN** bootstrap завершён для зафиксированного native supply contract
- **THEN** manifest содержит замкнутый набор требуемых non-system DLL без wildcard entries, и каждый entry разрешается в существующий файл с совпадающими SHA-256 и `x64` architecture

#### Scenario: DLL изменена после формирования manifest
- **WHEN** hash staging source не совпадает с manifest
- **THEN** staging или build завершается ошибкой и не создаёт bundle, представленный как проверенный

#### Scenario: Необъявленная native dependency
- **WHEN** import closure приложения или manifest-listed DLL требует non-system DLL, отсутствующую в manifest
- **THEN** bundle verification завершается ошибкой и указывает unresolved dependency

#### Scenario: Отсутствует license metadata
- **WHEN** manifest entry не содержит разрешимый license/notice path
- **THEN** runtime manifest validation завершается ошибкой

### Requirement: Общая самопроверка runtime
Приложение MUST иметь один Rust implementation самопроверки, используемый headless CLI и diagnostic UI. Самопроверка MUST возвращать schema-versioned report с отдельными результатами `opencv_load`, `image_codec`, `writer_open`, `writer_backend` и `writer_roundtrip`, версией и build-information summary/hash OpenCV, ожидаемыми backends `MSMF`, `DSHOW`, `FFMPEG`, а также путями и SHA-256 фактически загруженных OpenCV/codec modules.

Самопроверка MUST выполнить `Mat 640x480 -> resize 320x240 -> imencode(.jpg) -> imdecode` с проверкой размеров и записать 30 синтетических кадров 320x240 @ 30 FPS через `VideoWriter(CAP_FFMPEG, MJPG, AVI)`. Writer MUST сообщить backend name `FFMPEG`; после finalize файл MUST повторно открыться, декодировать ровно 30 кадров ожидаемого размера и быть корректно закрыт/удалён согласно diagnostic artifact policy.

#### Scenario: Успешная headless самопроверка
- **WHEN** установленное приложение запускается как `<app>.exe --self-check --json <path>` с корректным packaged runtime
- **THEN** GUI window не создаётся, report атомарно сохраняется по указанному пути, все обязательные checks имеют успешный результат, а process завершается с exit code `0`

#### Scenario: Отсутствует обязательный backend
- **WHEN** OpenCV загружается, но один из обязательных backends недоступен
- **THEN** соответствующий check содержит безопасное диагностическое описание, остальные доступные checks отражаются отдельно, а process завершается с exit code `11`

#### Scenario: Ошибка JPEG codec
- **WHEN** JPEG encode/decode round-trip не проходит
- **THEN** `image_codec` имеет status `failed`, report сохраняется при возможности, а process завершается с exit code `12`

#### Scenario: Ошибка writer или reread
- **WHEN** writer не открывается, выбран не backend `FFMPEG`, запись/finalize завершается ошибкой или reread возвращает неверное число/размер кадров
- **THEN** report различает место отказа и process возвращает соответственно exit code `13` для writer failure либо `14` для reread/count mismatch

#### Scenario: Внутренняя ошибка отчёта
- **WHEN** самопроверка не может безопасно завершить внутреннюю операцию или сериализовать/сохранить report
- **THEN** process завершается с exit code `20`, не использует panic как штатный путь и не выводит secrets или внутренний source chain в UI

### Requirement: Diagnostic UI без камеры
Установленное GUI-приложение MUST запускаться без доступной камеры и предоставлять минимальный diagnostic экран, который вызывает общий Rust self-check через versioned Tauri command, отображает отдельный результат каждого check и не дублирует self-check logic во frontend. Tauri capabilities, permissions и CSP MUST быть ограничены необходимым для этого экрана минимумом.

#### Scenario: Self-check из GUI
- **WHEN** пользователь запускает self-check на diagnostic экране
- **THEN** UI остаётся отзывчивым, получает versioned safe DTO от общего Rust implementation и показывает pass/fail для каждого обязательного check

#### Scenario: Native ошибка из GUI
- **WHEN** self-check завершается внутренней native ошибкой
- **THEN** UI получает стабильный публичный error code и безопасное сообщение без Rust debug output, локальных source paths или source chain

#### Scenario: Камера отсутствует
- **WHEN** на runtime-машине нет камеры или camera permission не предоставлен
- **THEN** приложение и diagnostic self-check этого change продолжают работать, поскольку не открывают camera device

### Requirement: Самодостаточный Windows installer
`bun run app:build` MUST создавать release executable и устанавливаемый x64 NSIS artifact на стабильной связке Rust crate `tauri` 2.11.5 и `@tauri-apps/cli` 2.11.4. Поскольку config schema этой связки ещё не предоставляет поля `build.windows.staticVCRuntime` и `bundle.windows.bundleVCRuntime`, build MUST подтверждать effective `STATIC_VCRUNTIME=true` для application binary, а VC runtime для dynamic OpenCV/FFmpeg DLL MUST поставляться app-local как часть manifest-listed native runtime с точными SHA-256, `x64` architecture, source и license metadata. Bundle configuration MUST включать `bundle.windows.webviewInstallMode.type = "offlineInstaller"` и current-user NSIS install mode, пока отдельное согласованное требование не задаст иное.

#### Scenario: Сборка release bundle
- **WHEN** build выполняется после успешных bootstrap, frozen install и tests
- **THEN** создаются release executable и x64 NSIS installer, содержащие только проверенный runtime manifest payload, app-local VC runtime для dynamic native DLL и необходимые runtime installers; evidence подтверждает effective `STATIC_VCRUNTIME=true`

#### Scenario: Runtime dependency берётся из build tree
- **WHEN** bundle может запуститься только за счёт DLL из `.vcpkg_installed`, `target`, developer `PATH` или другого build-tree location
- **THEN** bundle не считается прошедшим verification

### Requirement: Проверка установленного приложения
`bun run verify:bundle` MUST проверять конкретный NSIS artifact через установку в отдельное test location или чистую Windows VM, запускать installed executable в окружении без `VCPKG_*` и `OPENCV_*` с `PATH`, содержащим только системные каталоги, и сохранять machine-readable evidence. Verification MUST проверить installer SHA-256/size, manifest hashes/architecture, installed path, app executable hash, self-check report/exit code, фактически загруженные module paths/hashes, GUI startup и uninstall result.

#### Scenario: Installed application не зависит от developer environment
- **WHEN** NSIS artifact устанавливается на совместимую Windows x64 runtime-машину без OpenCV development/runtime и запускается в очищенном окружении
- **THEN** headless self-check успешен, GUI запускается из installed location, а все non-system OpenCV/codec modules загружены только из installed location

#### Scenario: Module загружен из недопустимого location
- **WHEN** фактически загруженный OpenCV/codec module находится вне installed location и системного Windows directory либо его hash не совпадает с manifest
- **THEN** verification завершается с ненулевым exit code и сохраняет offending module path и ожидаемый безопасный metadata, не объявляя bundle готовым

#### Scenario: Installer или uninstall завершился ошибкой
- **WHEN** установка, определение installed path, запуск installed executable или удаление test installation не завершается успешно
- **THEN** verification сохраняет достигнутые результаты и exit codes, завершается ошибкой и не подменяет отсутствующий этап успешным статусом

### Requirement: Evidence и совместимость
Система MUST сохранять компактные JSON/log summaries, версии инструментов и точные команды воспроизведения, достаточные для аудита результата. Многомегабайтные temporary AVI, installer copies и build outputs MUST оставаться локальными и ссылаться из evidence через path, size и SHA-256, а не добавляться в git. Первый change MUST поддерживать Windows 10/11 x64 build/runtime hosts; camera hardware и `ffprobe` не являются prerequisites этого change.

#### Scenario: Успешный evidence package
- **WHEN** clean-checkout build и installed-app verification завершены
- **THEN** evidence связывает commit, toolchain versions, native supply metadata, runtime manifest, installer, executable, self-check и loaded modules без включения secrets

#### Scenario: Большой artifact создаётся во время проверки
- **WHEN** self-check создаёт temporary AVI или build создаёт installer
- **THEN** git-tracked evidence содержит только metadata и hash, а сам большой artifact хранится вне отслеживаемого evidence набора

#### Scenario: Неподдерживаемая платформа
- **WHEN** bootstrap или bundle verification запускается не на Windows x64
- **THEN** команда завершается ранней управляемой ошибкой и не заявляет частичную проверку как успешную
