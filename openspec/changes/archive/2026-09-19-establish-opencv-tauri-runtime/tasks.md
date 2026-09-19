# Tasks

## 1. Application scaffold и контракты

- [x] 1.1 Создать в корне репозитория минимальный Tauri 2/Vue/TypeScript/Bun scaffold, выбрать совместимые версии без автоматического обновления на `latest`, зафиксировать их в `package.json`, `bun.lock`, `Cargo.toml`, `Cargo.lock` и `rust-toolchain.toml`; проверить `bun install --frozen-lockfile`, frontend build и `cargo metadata --locked`.
- [x] 1.2 Настроить package scripts `typecheck`, `lint`, `test:unit`, `build`, `test`, `app:dev`, `app:build`, `verify:bundle`; проверить, что отсутствующая вложенная команда и любой failure дают ненулевой exit code.
- [x] 1.3 Создать Rust границы `SelfCheckService`, OpenCV/module/filesystem adapters, versioned report/transport DTO и локальные typed error enums; проверить unit-тестами сериализацию schema version, безопасный Tauri error mapping и mapping exit codes `10`, `11`, `12`, `13`, `14`, `20`.
- [x] 1.4 Ограничить Tauri commands, capabilities, permissions и CSP минимальным diagnostic surface; проверить generated configuration и negative test, что непредусмотренная capability не разрешена.

## 2. Pinned native supply

- [x] 2.1 Добавить `vcpkg.json` и `vcpkg-configuration.json` с baseline `9e593bb18ea69cc5095e012465dcd675a822ed0d`, triplet `x64-windows`, `opencv4` 4.12.0#7, `default-features = false` и features `dshow`, `ffmpeg`, `jpeg`, `msmf`, `thread`; проверить manifest-mode resolution и фактическую port version.
- [x] 2.2 Реализовать `tools/verify-environment.ps1` для проверки Windows x64, PowerShell 7, Git, MSVC x64 toolset, Windows SDK, Rust/Bun versions и безопасного JSON evidence; проверить успешный report и управляемый failure как минимум для одного отсутствующего prerequisite через injectable test input.
- [x] 2.3 Реализовать идемпотентный `tools/bootstrap-opencv.ps1` с repository-local `.tools/vcpkg` и `.vcpkg_installed`, exact revision check до install и фактической проверкой port/triplet/features; проверить первый запуск, повторный запуск и отказ на подменённой revision.
- [x] 2.4 Реализовать `tools/run-with-opencv.ps1`, вычисляющий пути от repository root и передающий process-local native include/lib/bin environment только дочернему process; проверить запуск из другого current directory и отсутствие изменения parent/global environment.
- [x] 2.5 Добавить точечные `.gitignore` rules для repository-local toolchain, installed packages, staging, build и больших diagnostic artifacts; проверить `git status --short`, что manifests, schemas и компактное evidence отслеживаются, а generated binaries — нет.

## 3. Runtime manifest и staging

- [x] 3.1 Создать `tools/runtime-manifest.schema.json` и versioned Rust/PowerShell model с обязательными полями name, SHA-256, PE architecture, source, bundle destination, purpose и license/notice path; проверить schema validation на корректном manifest и negative fixtures с wildcard, неверным hash/architecture и отсутствующей license metadata.
- [x] 3.2 Реализовать генерацию `runtime/manifest.json` из конкретного release vcpkg install tree с рекурсивным non-system PE import closure, app-local VC runtime для dynamic OpenCV/FFmpeg DLL и исключением debug DLL; проверить, что каждый entry существует, имеет `x64` architecture, совпадающий SHA-256 и разрешимый license/notice.
- [x] 3.3 Реализовать deterministic release staging, которое очищает только свой известный output directory, повторно проверяет manifest и копирует только allowlisted files, включая app-local VC runtime; проверить failure при изменённой DLL и отсутствие unlisted файлов в staging.
- [x] 3.4 Добавить post-build import validation от release executable и staged DLL; проверить negative fixture с unresolved non-system import, обязательное app-local разрешение MSVC runtime imports и успешное разрешение прочих system imports только в canonical Windows system directories.

## 4. Runtime self-check

- [x] 4.1 Реализовать orchestration `SelfCheckService` с независимыми status для `opencv_load`, `image_codec`, `writer_open`, `writer_backend`, `writer_roundtrip` и skipped dependent checks; проверить unit-тестами все success/failure ветви через fake adapters без OpenCV runtime.
- [x] 4.2 Реализовать OpenCV adapter: version/build-information hash, наличие `MSMF`/`DSHOW`/`FFMPEG`, JPEG resize/encode/decode и 30-frame `CAP_FFMPEG`/MJPG/AVI write-finalize-reread; проверить integration test на подготовленном pinned OpenCV runtime, включая backend name, frame count и dimensions.
- [x] 4.3 Реализовать Windows process-module enumeration, canonical path и SHA-256 сопоставление с runtime manifest; проверить tests для разрешённого installed/system origin, hash mismatch и module из build-tree path.
- [x] 4.4 Реализовать parsing `--self-check --json <path>` до создания Tauri GUI, atomic report write, deterministic cleanup и exit-code mapping; проверить subprocess tests, что успешный запуск не создаёт окно и возвращает `0`, а representative failures возвращают согласованные codes и сохраняют report при возможности.
- [x] 4.5 Добавить Tauri command для того же `SelfCheckService`, выполняемый вне UI thread и возвращающий только versioned safe DTO; проверить contract tests на совпадение CLI/UI report semantics и отсутствие внутренних paths/source chain в публичной ошибке.

## 5. Diagnostic UI

- [x] 5.1 Реализовать TypeScript DTO и централизованный camera-independent API client для self-check command; проверить typecheck и unit tests для decoding поддерживаемой schema и управляемого отказа на неизвестной schema/error code.
- [x] 5.2 Реализовать минимальный Vue diagnostic экран со start/loading/result/error states и отдельным отображением каждого check; проверить component tests на отзывчивость, успешный report, native failure и повторный запуск.
- [x] 5.3 Проверить GUI startup и diagnostic self-check на машине без камеры либо с запрещённым camera access; подтвердить evidence, что первый change не открывает camera device и не требует camera permission.

## 6. NSIS bundle и installed verification

- [x] 6.1 Настроить `tauri.conf.json` для x64 NSIS, current-user install, offline WebView2 и manifest-driven staged DLL на стабильной связке `tauri` 2.11.5 / CLI 2.11.4; проверить resolved configuration, effective `STATIC_VCRUNTIME=true`, app-local VC runtime и содержимое release bundle.
- [x] 6.2 Подключить `bun run app:dev` и `bun run app:build` к `run-with-opencv.ps1`, manifest validation и staging; проверить dev startup и создание ровно одного однозначно выбираемого release NSIS artifact.
- [x] 6.3 Реализовать `tools/verify-bundle.ps1`: schema/hash/PE validation, installer hash/size, установка в отдельное test location, определение installed executable и headless self-check с удалёнными `VCPKG_*`/`OPENCV_*` и system-only `PATH`; проверить successful installed report без системного OpenCV.
- [x] 6.4 Дополнить verifier проверкой app executable hash, фактически загруженных module paths/hashes, GUI startup/controlled shutdown и uninstall result; проверить, что module из build tree, failed GUI smoke или failed uninstall дают общий failure при сохранении частичного evidence.
- [x] 6.5 Добавить versioned verification evidence model с отдельными этапами `manifest`, `install`, `headless_self_check`, `module_provenance`, `gui_smoke`, `uninstall`; проверить schema и отсутствие secrets/полных environment dumps в output.

## 7. Документация и сквозная приёмка

- [x] 7.1 Написать README с раздельными build/runtime prerequisites, exact clean-checkout commands, expected local storage, self-check exit codes, evidence paths, troubleshooting и cleanup; проверить команды README в новом shell без заранее заданных OpenCV/vcpkg variables.
- [x] 7.2 Зафиксировать environment evidence с фактическими версиями Windows, MSVC/SDK, Rust, Bun, Tauri, OpenCV, CMake/Ninja и vcpkg revision; проверить согласованность с manifests/lockfiles/runtime manifest автоматизированной проверкой.
- [x] 7.3 Выполнить `bun run test` и убедиться, что он включает frontend typecheck/lint/unit/build и Rust fmt/clippy/workspace tests; сохранить компактный command/result summary и устранить все обязательные failures.
- [x] 7.4 Из чистого checkout выполнить `./tools/bootstrap-opencv.ps1`, `bun install --frozen-lockfile`, `bun run test`, `bun run app:build`, `bun run verify:bundle`; проверить x64 NSIS installed-app self-check и GUI smoke без build-tree DLL и сохранить hashes/paths/results как итоговое evidence первого change.
- [x] 7.5 Сопоставить итоговые evidence со всеми сценариями `opencv-desktop-runtime`, выполнить strict OpenSpec validation и project quality gate; не начинать camera-pipeline change и не помечать этот change завершённым при непроверенном installed-app или module-provenance gate.
