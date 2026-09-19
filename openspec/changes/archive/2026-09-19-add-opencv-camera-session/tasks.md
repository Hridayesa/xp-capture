# Tasks

## 1. Domain model и scan policy

- [x] 1.1 Создать `camera` module с отдельными domain types для backend, validated `DeviceScanPolicy`, service/operation states, probe outcomes, endpoint identity и snapshots; проверить unit-тестами инварианты generation, progress counts, terminal states и запрет endpoint без успешного probe.
- [x] 1.2 Реализовать преобразование versioned request в policy с defaults `0..=5`, `[MSMF, DSHOW]`, deadlines/reopen delay и hard limits из design; проверить boundary-тестами валидные границы, reversed/oversized range, empty/unknown backend, zero/oversized/inconsistent durations и отсутствие generation increment при `INVALID_CONFIG`.
- [x] 1.3 Реализовать детерминированный ordered cartesian product `(backend, numericIndex)` и opaque process-local `operationId`/`DeviceEndpointKey`; проверить порядок, независимость одинакового index для MSMF/DSHOW, checked generation increment и отсутствие persistence/reuse после нового service instance.
- [x] 1.4 Добавить раздельные `CameraConfigError`, `CaptureAdapterError`, `CameraServiceError` и локальные `Result` aliases без `panic`/`unwrap`/`expect` в production paths; проверить сохранение internal source chain и ожидаемые variants unit-тестами.

## 2. Worker, lifecycle и watchdog

- [x] 2.1 Определить fakeable capture backend/session и monotonic time/notification seams так, чтобы concrete OpenCV types не попадали в domain/application modules; проверить scripted fake adapter на open, empty/read failure, first frame и explicit release outcomes.
- [x] 2.2 Реализовать один sequential camera worker, который проверяет cancellation между native calls, публикует progress, освобождает session до `reopenDelayMs` и продолжает после обычного probe failure; проверить fake-тестами найденный endpoint, empty frame timeout, busy/open/read failure, no-camera result, ordered outcomes и отсутствие второго одновременно открытого session.
- [x] 2.3 Реализовать `CameraService` с единственной current/last operation, состояниями `Idle`/`Scanning`/`Faulted`/`Stuck`, сохранёнными join handles и snapshot access без удержания lock во время native work; проверить normal completion, `BUSY`, partial failures, service-level failure after cleanup, `Faulted -> Idle` через `stop_camera` и idempotent stop в `Idle`.
- [x] 2.4 Реализовать cancellation и внешний watchdog для first-frame/operation/shutdown deadlines без unsafe thread termination; проверить deterministic fake-time tests для cancel между probes, повторной cancel terminal operation, operation timeout, sticky `Stuck`, запрета нового scan, позднего возврата worker без восстановления `Idle` и отсутствия ложного cleanup success.
- [x] 2.5 Обработать app/window shutdown через тот же service stop path и сохранить честный state при непрервавшемся native call; проверить lifecycle test, что завершившийся worker join-ится, а незавершившийся остаётся `Stuck` и не detach-ится для продолжения работы приложения.

## 3. OpenCV adapter

- [x] 3.1 Реализовать Windows OpenCV adapter с явными `CAP_MSMF`/`CAP_DSHOW`, новым `VideoCapture` на каждый probe, `is_opened`, чтением до первого непустого `Mat` и явным checked `release`; проверить adapter-level tests/compile checks и отсутствие `CAP_ANY`, кэширования handle или передачи `Mat` за пределы worker.
- [x] 3.2 Сопоставить ожидаемые OpenCV open/read outcomes с probe statuses, а release/internal failures — с service errors без раскрытия native diagnostic text; проверить negative tests с injectable adapter results и marker, который отсутствует в public snapshot/error JSON.
- [x] 3.3 Добавить opt-in Windows hardware integration harness поверх pinned `run-with-opencv.ps1`, принимающий явные backend/index range и evidence path; проверить, что обычный `bun run test` не требует camera, а opt-in run формирует compact schema-versioned JSON без frame bytes, secrets и полного environment dump.

## 4. Tauri transport и security boundary

- [x] 4.1 Добавить versioned DTO и единый mapper для `start_device_scan`, `get_device_scan`, `cancel_device_scan`, `stop_camera` и codes `INVALID_CONFIG`, `BUSY`, `STALE_SCAN_OPERATION`, `OPEN_FAILED`, `READ_TIMEOUT`, `READ_STALLED`, `CANCELLED`, `INTERNAL`; проверить serde round-trip, unknown-field/schema rejection и отсутствие paths/source chain/raw frames.
- [x] 4.2 Создать один managed `CameraService` при GUI startup, зарегистрировать четыре commands и вынести ожидание cancel/stop из UI thread; проверить command contract tests для start/progress/completion, concurrent start, stale operation, idempotent cancel/stop и responsive `run_self_check` во время scan.
- [x] 4.3 Обновить shutdown wiring и security tests без добавления filesystem/network permissions или ослабления CSP; проверить, что GUI startup и `run_self_check` не создают camera worker, capability permissions остаются минимальными, а existing headless self-check tests проходят без изменения exit codes.

## 5. TypeScript client и Vue UI

- [x] 5.1 Добавить централизованный `src/api/camera.ts` с runtime decoding transport v1, safe public error mapping и single-in-flight polling controller; проверить unit-тестами supported/unknown schema, malformed DTO, unknown error code, отсутствие overlapping polls и остановку polling для всех terminal states.
- [x] 5.2 Расширить diagnostic screen отдельным camera scan flow: editable bounded range/backends, start/cancel, progress/current tuple, empty state, ordered endpoints/rejections и selection текущего generation; проверить component tests для idle, scanning, completed, no-camera, partial failures, cancel и выбора независимых MSMF/DSHOW endpoints.
- [x] 5.3 Реализовать очистку selection при accepted rescan, блокировку concurrent start и sticky restart-required UI для `Stuck`; проверить fake-timer component tests, что stale endpoint не остаётся выбранным, polling прекращается, retry недоступен в `Stuck`, а safe error не показывает native marker.
- [x] 5.4 Сохранить self-check как независимый flow и обновить accessibility/responsive states scan UI; проверить keyboard/button disabled semantics, live status announcements и regression test, что mount, self-check и retry не вызывают ни одну camera command без явного scan.

## 6. Hardware evidence, документация и приёмка

- [x] 6.1 Дополнить README camera-session defaults, numeric identity warning, command/error semantics, opt-in hardware smoke, evidence path и `Stuck`/restart troubleshooting; проверить команды документации в новом shell через repository-local OpenCV wrapper.
- [x] 6.2 На доступной Windows camera выполнить opt-in bounded scan как минимум с попытками MSMF и DSHOW, получить хотя бы один endpoint с первым кадром и повторить scan после cleanup; сохранить compact `evidence/camera-session-smoke.json` с policy, outcomes, generation, release/reopen result и environment version references, не отмечая задачу выполненной при наличии камеры без положительного open/read/release evidence.
- [x] 6.3 Проверить no-camera/occupied-camera path отдельно либо scripted hardware fixture: scan завершается пустым/частичным результатом без global failure, self-check остаётся доступным, а повторный scan возможен после подтверждённого cleanup; сохранить краткий result в camera-session evidence.
- [x] 6.4 Выполнить `bun run test`, `bun run app:build` и `bun run verify:bundle`, проверить installed GUI startup без автоматического camera access и устранить все обязательные failures; сохранить обновлённую requirement-to-evidence matrix для `opencv-camera-session`.
- [x] 6.5 Выполнить `bun run tools/openspec.ts validate add-opencv-camera-session --strict` и project `$quality-gate`, сопоставить каждый scenario с test/evidence и явно оценить decision gate `Stuck`; не начинать `add-opencv-mode-profiling` и не объявлять change завершённым при непроверенном real-camera open/read/release либо обнаруженном невосстанавливаемом stall без отдельного архитектурного решения.
