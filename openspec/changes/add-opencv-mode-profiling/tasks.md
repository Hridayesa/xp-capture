# Tasks

## 1. Candidate policy и чистая domain model

- [x] 1.1 Добавить schema-versioned `config/mode-candidates.json`, Rust wire/domain types и validation hard limits для FourCC, resolutions, FPS, durations, thresholds и checked candidate count; проверить unit-тестами valid defaults, каждую boundary/duplicate/non-finite/overflow ошибку и отсутствие camera adapter calls при `INVALID_CONFIG`.
- [x] 1.2 Реализовать canonical normalization и SHA-256 hash применённой policy с отдельной versioned hash DTO; проверить golden-тестом независимость от JSON whitespace/object-key order и чувствительность к порядку candidate arrays.
- [x] 1.3 Реализовать детерминированный ordered cartesian product `FourCC × resolution × FPS` без synthesis/binary search; проверить exact order, полноту, checked multiplication и отсутствие пропуска rejected candidates.
- [x] 1.4 Реализовать transport-neutral domain types requested/set/reported/actual properties, candidate phases/statuses и все machine-readable failure reasons без OpenCV/Tauri imports; проверить compile/unit-тестами невозможность выдать verified result без terminal gate.
- [x] 1.5 Реализовать pure capture metrics accumulator для attempts/frames/empty/failures, elapsed/measured FPS, median/p95/p99/max gap и long-gap ratio; проверить synthetic monotonic timestamps, delayed first measured frame, `<2` frames, even median, nearest-rank percentiles, zero attempts и 100000-sample bound.
- [x] 1.6 Реализовать gate evaluator, который собирает все причины exact-resolution/FPS/read-failure/max-gap/long-gap rejection и не использует `set()` как pass/fail; проверить `set=false` с успешным actual mode, `set=true` с coercion, under-target и несколько одновременно нарушенных stability gates.
- [x] 1.7 Реализовать `ProfileReport`, aggregation `maxVerifiedByResolution`, сохранение всех FourCC ties, opaque `verifiedModeId` registry и pure revalidation gate; проверить grouping только внутри endpoint+resolution, исключение rejected tuples, invalidation по scan/profile/stop/config hash и результаты `MODE_COERCED`/`UNDER_TARGET_FPS`.

## 2. Capture ports и OpenCV adapter

- [x] 2.1 Расширить `CaptureSession` методами apply/read reported mode и `FrameRead::Frame(FrameMetadata)`, обновить scripted fake adapter для set/get/read/release/blocked calls и адаптировать scan path; проверить, что существующие scan unit tests и один-owner assertions проходят без изменения wire result.
- [x] 2.2 Реализовать OpenCV 0.100.1 mapping явных `CAP_MSMF`/`CAP_DSHOW`, `CAP_PROP_FOURCC`, width, height и FPS, сохраняя bool каждого `set()` и nullable reported values; проверить adapter tests для порядка properties, FourCC encode/decode, actual `Mat` dimensions и отсутствия `CAP_ANY`/raw frame за port boundary.
- [x] 2.3 Расширить typed `CaptureAdapterError` для apply/get/read/release, сохранив source chain только внутри Rust и checked scoped release; проверить negative tests с internal marker, который не попадает в candidate DTO/public error JSON, и release failure, который запрещает следующий open.

## 3. Profile worker, service state и watchdog

- [x] 3.1 Разделить `ServiceInner` на `last_scan`, `last_profile` и один `active_operation`, добавить `Profiling`/`ProfileReady` и verified registry при одном control/supervisor; проверить transitions `Idle → Scanning → Idle`, `Idle → Profiling → ProfileReady`, scan/profile `BUSY`, profile только для актуального endpoint и stale invalidation.
- [x] 3.2 Реализовать последовательную profile phase machine `opening → applying → reported → first_frame → warmup → measuring → release → reopen_delay`; проверить fake-тестами новый session на каждый tuple, continuous warm-up drain, сброс counters, delayed first measured frame, ordered progress и отсутствие одновременно открытых sessions.
- [x] 3.3 Реализовать единственный retry только для transient open/first-read-start до warm-up, включая `attempt_count`/retry reason и общий candidate deadline; проверить first-fail-second-pass, отсутствие retry measurement failure и запрет выхода retry за deadline.
- [x] 3.4 Обобщить `OperationControl`/supervisor на candidate epoch, global operation timeout, user cancellation и shutdown deadline; проверить cancel в warm-up/measurement, partial terminal report без verified current tuple, candidate timeout с продолжением после cleanup, sticky `Stuck`, late return без восстановления и сохранённый `JoinHandle`.
- [x] 3.5 Реализовать terminal report finalization, environment version references, `ProfileReady` cleanup и `stop_camera` для active/profile-ready states; проверить, что handle освобождён до terminal success, stop идемпотентен, verified IDs инвалидируются, а historical report остаётся доступен только по последнему `profileId`.
- [x] 3.6 Сохранить scan snapshot/DTO semantics при последующем profile и новый scan из `ProfileReady`; проверить regression tests, что `DeviceScanSnapshotV1` byte shape/enums не меняются, старый endpoint не переиспользуется, а self-check остаётся отзывчивым во время profile.

## 4. Versioned Tauri и TypeScript transport

- [x] 4.1 Добавить Rust DTO/mappers schema v1 для `start_profile`, `get_profile_status`, `get_profile_result` и `cancel_profile`, включая strict unknown-field/schema rejection, current tuple/phase, ordered results и maxima; проверить serde/contract tests для positive, malformed, stale, `PROFILE_NOT_READY` и отсутствия paths/source/raw frames.
- [x] 4.2 Зарегистрировать четыре profile commands на существующем managed `CameraService`, вынести cancel/cleanup ожидание в `spawn_blocking` и сохранить общий window shutdown stop path; проверить command tests, `security_config` для одного service, пустых permissions/неизменного CSP и отсутствие camera access на GUI startup.
- [x] 4.3 Добавить `src/api/profile.ts` с точными TypeScript types, strict runtime decoders, safe error mapping и single-in-flight `ProfilePollingController`; проверить Vitest для всех status/phase/status-code enums, unknown/extended DTO, command argument shape, no-overlap polling и остановки на completed/cancelled/failed/stuck.

## 5. Vue profiling UI

- [x] 5.1 Добавить отдельный profile component, использующий общий `config/mode-candidates.json`, выбранный endpoint текущего generation и явную кнопку start; проверить component tests, что mount/self-check/scan/selection не запускают profile, stale/empty selection блокирует start, а accepted start очищает прежние results/verified selection.
- [x] 5.2 Реализовать authoritative progress/current tuple/phase, candidate budgets и safe cancel/restart-required states; проверить fake-timer tests для single poll loop, disabled concurrent scan/profile, cancellation после cleanup, `PROFILE_NOT_READY`, safe public error и sticky `Stuck`.
- [x] 5.3 Реализовать ordered result table и grouped maxima table с раздельными requested/set/reported/actual/measured labels, всеми failure reasons и всеми ties; проверить rejected/coerced tuple не selectable, empty/partial result управляем, backend/FourCC видимы и reported match не называется native subtype.
- [x] 5.4 Связать accepted rescan/profile/stop с invalidation UI state и сохранить accessibility/responsive behavior; проверить keyboard/fieldset/button semantics, `aria-live`, selection reset по generation и regression, что существующие camera scan/self-check component tests проходят.

## 6. Hardware harness, документация и evidence

- [x] 6.1 Добавить opt-in `camera-mode-profile-smoke` binary с explicit bounded scan scope, backend/index, config и evidence path, использующий тот же service path и атомарно пишущий compact JSON; проверить CLI parse/error tests, non-zero acceptance failures и что обычный `bun run test` не требует camera.
- [x] 6.2 Дополнить README defaults/hard limits, команды profile smoke, requested/reported/measured semantics, numeric identity/FourCC warnings, cancellation/`Stuck` troubleshooting и capture-only versus full-pipeline границу; проверить все документированные команды и paths в новом shell через repository-local OpenCV wrapper.
- [x] 6.3 На доступной Windows camera выполнить bounded installed/profile smoke минимум для подтверждённого DSHOW endpoint и попытки MSMF, сохранить `evidence/camera-mode-profile-smoke.json` с config hash, ordered outcomes/metrics, release/reopen и environment references; не считать задачу выполненной без реального measurement либо честно зафиксированного decision-gate failure.
- [x] 6.4 Обновить `evidence/requirement-matrix.md`, сопоставив каждый profiling/state-machine scenario с unit/contract/UI/hardware evidence и явно оценив gates native stall, exact subtype/friendly identity и достаточность empirical mode list.

## 7. Итоговая проверка change

- [x] 7.1 Выполнить полный deterministic project gate `bun run test` и устранить все fmt/clippy/Rust/Vitest/typecheck/lint/build/runtime-supply failures; сохранить точные прошедшие команды в отчёте.
- [x] 7.2 Выполнить `bun run app:build` и `bun run verify:bundle`, проверить installed GUI startup/self-check без автоматического camera/profile access и отсутствие новых Tauri permissions/CSP origins; обновить bundle evidence только после полного pass.
- [x] 7.3 Выполнить `bun run tools/openspec.ts validate add-opencv-mode-profiling --strict` и project `$quality-gate`, проверить requirement → task → code → test/evidence traceability и не начинать preview change при непроверенном real-camera profiling или нерешённом `Stuck` decision gate.
