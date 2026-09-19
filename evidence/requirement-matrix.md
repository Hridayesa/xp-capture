# Requirement → evidence

## `establish-opencv-tauri-runtime`

| Requirement | Positive evidence | Negative/boundary evidence |
| --- | --- | --- |
| Воспроизводимая подготовка native dependencies | `./tools/bootstrap-opencv.ps1` подтвердил vcpkg `9e593bb18ea69cc5095e012465dcd675a822ed0d`, `x64-windows`, OpenCV `4.12.0#7`; версии агрегированы в `environment-summary.json`. | `verify-environment.ps1` fixture для отсутствующего Git; bootstrap revision mismatch branch; `test-run-with-opencv.ps1` подтверждает изоляцию от inherited `OPENCV_*`/`VCPKG_*`. |
| Закреплённый application toolchain | `bun install --frozen-lockfile` — no changes; `package.json`, `bun.lock`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`; `bun run test` прошёл. | Frozen install не допускает изменение lockfile; package scripts возвращают non-zero при failure. |
| Проверяемый runtime manifest | `runtime/manifest.json`: 15 exact x64 DLL, включая 4 app-local VC runtime; `bun run test:runtime-supply`; `runtime-imports.json`. | Schema fixtures: wildcard/hash/architecture/license; controlled source hash mismatch; unlisted staging cleanup; unresolved import и отсутствующий app-local `MSVCP140_2.dll`. |
| Общая самопроверка runtime | Rust unit tests; real OpenCV integration test; installed `headless_self_check` stage в `bundle-verification.json`. | Typed exit-code branches `10`–`14`, `20`; missing backend/JPEG/writer/count/module failures; CLI сохраняет report без GUI. |
| Diagnostic UI без камеры | Self-check regression scenarios входят в общий набор 40 Vitest tests (7 test files); `camera-independent-gui.json`; installed `gui_smoke` stage passed. | Safe native error/retry tests; пустой diagnostics permission list; config test запрещает расширение capability; production code не открывает camera device. |
| Самодостаточный Windows installer | `tauri-build-environment.json` фиксирует `STATIC_VCRUNTIME=true`, target `x86_64-pc-windows-msvc`, Tauri `2.11.5`/CLI `2.11.4`; один x64 NSIS artifact; current-user/offline config test. | Pre-bundle import validation fail-closed; неподдерживаемые `staticVCRuntime`/`bundleVCRuntime` поля отсутствуют. |
| Проверка установленного приложения | `bundle-verification.json`: `manifest`, `install`, `headless_self_check`, `module_provenance`, `gui_smoke`, `uninstall` — passed; 15 loaded module paths/hashes; installer/executable hashes. | Rust test отклоняет build-tree module; verifier сохранял partial failing evidence на provenance/uninstall mismatch; policy tests не позволяют объявить pass при failed `module_provenance`, `gui_smoke` или `uninstall`. |
| Evidence и совместимость | `environment-summary.json`, `runtime-imports.json`, `bundle-verification.json`, этот matrix; README содержит точные команды, storage, troubleshooting и cleanup. | Evidence schemas не содержат full environment dumps/secrets; installers, AVI, staged DLL и detailed artifacts игнорируются; unsupported host завершается на prerequisite gate. |

Итоговая воспроизводимая последовательность 2026-09-19: `./tools/bootstrap-opencv.ps1` → `bun install --frozen-lockfile` → `bun run test` → `bun run app:build` → `bun run verify:bundle`. Все команды завершились успешно после разрешения сетевого скачивания CMake 4.4.0, требуемого pinned vcpkg tool; native OpenCV остался `4.12.0#7`.

## `opencv-camera-session`

| Requirement | Positive evidence | Negative / boundary evidence |
| --- | --- | --- |
| Валидируемый ограниченный scan | Rust policy/transport tests: defaults, ordered backends, accepted generation; `camera-session-smoke.json` фиксирует фактическую policy `0..=5`, MSMF/DSHOW и deadlines. | Reversed/oversized range, empty/duplicate/unknown backend, zero/oversized/inconsistent duration и unknown schema/field; invalid request не увеличивает generation. |
| Последовательное обнаружение endpoints | Scripted adapter tests подтверждают `available`, `open_failed`, `first_frame_timeout`, `read_failed`, ordered outcomes, checked release и максимум один active session. | Empty-frame timeout продолжает следующий probe; no-camera даёт `completed`/empty, release failure не выдаёт ложный cleanup. |
| Эфемерная identity | Generation и process-local `operationId`/`DeviceEndpointKey` tests; одинаковый index MSMF/DSHOW создаёт независимые keys. | Новый service начинает с generation 1 и не переиспользует operation identity; accepted rescan очищает UI selection. |
| Единственная operation и state machine | Service tests: normal completion, `BUSY`, partial/no-camera results, operation timeout, `Faulted -> Idle`, idempotent stop. | Concurrent start не создаёт второй worker; generation overflow типизирован; snapshot access не удерживает lock во время native work. |
| Cancellation, deadlines и честный `Stuck` | Cancellation между probes и terminal-idempotency; fake-time watchdog test; app `ExitRequested` вызывает тот же `stop` path. | Blocked read становится sticky `Stuck`; новый scan запрещён, late return не восстанавливает `Idle`, join ownership остаётся у service. |
| Versioned Tauri transport и безопасные ошибки | Serde round-trip и command/service contract tests; TS strict runtime decoders и single-in-flight polling tests. | Unknown fields/schema/error code управляемы; native marker/source/path отсутствуют в public JSON; raw frame не входит в DTO. |
| Camera UI без автоматического доступа | Component tests: idle, scanning/progress, completed, no-camera, cancel, independent MSMF/DSHOW selection, rescan invalidation, `Stuck`, live status. | Mount, self-check и self-check retry не вызывают camera commands; concurrent start/retry в `Stuck` disabled; raw error marker не отображается. |
| Совместимость и security boundary | Existing headless/self-check/UI tests; `security_config` подтверждает один managed service и четыре commands. | Capability permissions остаются пустыми, CSP не содержит external origins, persistence/миграции не добавлены. |
| Hardware/no-camera evidence | `evidence/camera-session-smoke.json`: camera-present Windows host, два completed run MSMF+DSHOW `0..=5`, 12/12 outcomes в каждом, generation `1 -> 2`; `DSHOW / index 0` получил первый кадр и остался `available` после release/reopen. | `MSMF / index 0` в обоих run дал `first_frame_timeout`, остальные tuples — `open_failed`; управляемые partial/no-camera paths дополнительно подтверждены scripted tests без global failure. |

Decision gate `Stuck`: deterministic fake-time test подтверждает sticky state и отсутствие unsafe recovery. Реальный camera-present smoke не обнаружил невосстанавливаемый stall и подтвердил open/read/release/reopen через DSHOW; оснований вводить helper process/native capture в рамках этого change нет.

## `add-opencv-mode-profiling`

| Requirement / scenario | Deterministic evidence | Hardware / boundary evidence |
| --- | --- | --- |
| Candidate policy, normalization и hash | Rust tests: default schema, все list/numeric/duration/threshold/deadline boundaries, duplicates/non-finite values, checked maximum 256; invalid config до adapter call; golden SHA-256 и array-order sensitivity. | `camera-mode-profile-smoke.json` содержит применённую policy и hash `d0547bff91b14df3aec3e390964e0f46d63ca82d8634f14d57eb60bfb0a0d53c`. |
| Полный ordered tuple plan | Rust exact cartesian-product tests; result array включает rejected candidates, без binary search/synthesis. | DSHOW evidence содержит ровно 30 outcomes в config order: 2 FourCC × 3 resolutions × 5 FPS. |
| Requested / set / reported / actual | Port/adapter tests: explicit CAP mapping/order, FourCC round-trip, synthetic `Mat` dimensions, `set=false` не является gate. | Evidence сохраняет четыре отдельные группы; reported FourCC остаётся OpenCV diagnostic, а не native subtype. |
| Warm-up, capture-only metrics и gates | Synthetic monotonic tests: reset, delayed first measured frame, `<2` frames, even median, nearest-rank p95/p99, 100000 cap, simultaneous failure reasons. | Реальный DSHOW run измерил все 30 tuples: 640×480 ≈24 FPS, 1280×720 ≈10 FPS; все rejected `capture_under_target`, maxima пусты. |
| ProfileReport, ties и verified registry | Unit tests: verified-only aggregation, all ties, stale profile/generation/config/stop invalidation и `MODE_COERCED`/`UNDER_TARGET_FPS` revalidation. | Реальный report не выдаёт ни одного `verifiedModeId`, потому что ни один tuple не прошёл configured `0.95` FPS gate. |
| Один owner и phase machine | Scripted service tests: отдельная session на tuple, ordered phases, continuous reads, max one active handle, scan/profile `BUSY`, checked release. | DSHOW terminal cleanup разрешил повторный scan/open того же endpoint; `release_reopen.endpoint_reopened=true`. |
| Retry, cancellation, deadlines и `Stuck` | Open retry ровно один раз, cancel ждёт release; scan watchdog regression сохраняет sticky `Stuck` и join ownership. | DSHOW profile не обнаружил native stall. MSMF/index 0 отдельной попыткой не прошёл bounded endpoint scan и завершился non-zero до profile. |
| Versioned Rust/Tauri/TypeScript transport | Rust strict serde/mapping tests, safe stale/not-ready codes, compact report без paths/source/raw frames; TypeScript strict decoders покрывают все state/phase/status enums и single-in-flight polling. | Capabilities остаются пустыми, CSP не изменён, четыре profile commands используют тот же managed `CameraService`. |
| Explicit profiling UI | Vue tests: mount/selection/reset не запускают profile, empty/stale selection и concurrent scan блокируют start, progress/cancel/`PROFILE_NOT_READY`/`Stuck`, ordered result/maxima labels и ties. | Installed GUI profile interaction требует отдельного post-build smoke; startup/self-check camera-independent regression остаётся в deterministic suite. |
| CLI harness и документация | `camera-mode-profile-smoke` parse/error tests; обычный `bun run test` не обращается к camera; README фиксирует defaults, limits, semantics, commands и troubleshooting. | DSHOW command exit 0 и записал compact JSON; MSMF command exit 1 как честный acceptance failure. |

Decision gates на 2026-09-19:

- **Native stall:** pass — DSHOW 30-candidate run и reopen завершились, `Stuck` не наблюдался.
- **Exact native subtype / friendly stable identity:** not proven — OpenCV reported FourCC и numeric index не заменяют native enumeration; если это станет требованием, нужен Media Foundation/`MediaCapture` design.
- **Достаточность empirical mode list:** fail для текущей policy/hardware — ни один из 30 tuples не достиг `minimum_fps_ratio=0.95`; preview change начинать нельзя, пока policy/hardware/architecture decision не будет пересмотрен отдельным change.
