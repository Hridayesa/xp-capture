# Design

## Context

Мотивация зафиксирована в [proposal.md](proposal.md). Текущая реализация уже
имеет один process-local `CameraService`, managed Tauri state, отдельные
domain/application/adapter/transport modules, один scan worker и внешний
watchdog. `CaptureSession` умеет только `read/release`, а `FrameRead` не несёт
размер frame; `ServiceInner` и `OperationRecord` специализированы под scan.
Scan DTO schema version 1 строго декодируются в `src/api/camera.ts`, а Vue
хранит только draft scan form, selection и polling state.

Предыдущий change подтвердил на реальной Windows camera, что DSHOW endpoint
можно открыть, прочитать, освободить и открыть повторно; невосстанавливаемый
stall не наблюдался. При этом sticky `Stuck` и сохранение `JoinHandle` уже
проверены scripted test и остаются обязательной границей.

Фактический stack закреплён manifests/lockfiles: Rust 1.98.1 edition 2024,
`opencv` 0.100.1, `serde`/`serde_json`, `sha2`, `thiserror`, Tauri 2.11.5 и Vue
3.5.43. Новые production dependencies для profiling не нужны. Применяются
требования delta specs `opencv-mode-profiling` и `opencv-camera-session`.

## Goals / Non-Goals

**Goals:**

- расширить существующий service без второго владельца камеры и без передачи
  OpenCV/Tauri types в domain core;
- сделать candidate planning, metrics, gates и aggregation чистыми и полностью
  проверяемыми fake clock/backend;
- сохранить scan transport v1 и существующий camera-independent startup;
- оставить следующему preview/recording change типизированный
  `VerifiedModeDescriptor` и одну точку stale/revalidation проверки;
- обеспечить bounded memory, bounded duration и compact report/evidence.

**Non-Goals:**

- удерживать `VideoCapture` открытым после profile либо кэшировать его между
  candidates;
- реализовывать preview, writer, frame bytes ownership или full-pipeline gates;
- предоставлять runtime file picker/config persistence; default config
  компилируется в приложение, а фактическая policy передаётся в command;
- обещать стабильность numeric index, native subtype или полноту mode list;
- вводить универсальную generic job framework вместо узкого расширения camera
  service.

## Decisions

### 1. Domain остаётся независимым от OpenCV, Tauri и Vue

В `camera` появится отдельная profiling область с domain types
`ModeCandidatePolicy`, `ModeTuple`, `CaptureProperties`, `FrameMetadata`,
`CaptureMetrics`, `CandidateResult`, `ProfileReport`,
`VerifiedModeDescriptor` и typed statuses/reasons. Pure modules отвечают за:

- validation и нормализацию policy;
- ordered cartesian product;
- расчёт metrics из monotonic samples;
- применение gates;
- aggregation `maxVerifiedByResolution`;
- формирование/проверку opaque IDs через service-owned registry.

Application layer (`CameraService` и profile worker) оркестрирует ownership,
cancellation, deadlines и progress. OpenCV adapter преобразует CAP_PROP и
`Mat` в port types. Tauri transport только валидирует versioned DTO и маппит
ошибки. TypeScript client декодирует wire format; Vue component не знает Rust
errors и не вычисляет verified results.

Направление зависимостей:

```text
Vue component -> TypeScript profile client -> Tauri commands
                                      |
                                      v
OpenCV adapter -> camera ports -> profile application -> profile domain
                                      ^
                                      |
                         fake adapter / fake clock tests
```

Альтернатива — считать metrics во frontend или прямо в OpenCV adapter —
отклонена: первый вариант делает UI источником истины, второй связывает policy
с native library и усложняет deterministic tests.

### 2. Один config source, строгая нормализация и hash применённой policy

`config/mode-candidates.json` содержит schema version 1 и PoC defaults из
исходного плана. Vite импортирует тот же JSON для draft/default UI, а Rust
компилирует его через `include_str!` для validation tests и hardware harness.
На `start_profile` frontend передаёт полный `candidate_config`; Rust не доверяет
frontend и повторно строит validated `ModeCandidatePolicy`.

Canonical wire/file fields используют `snake_case`, как существующие camera
DTO. Для hash validated struct сериализуется в отдельную canonical hash DTO с
фиксированным порядком полей и сохранённым порядком массивов; SHA-256 считается
по compact UTF-8 JSON. Исходные whitespace и порядок object keys на hash не
влияют. `config_hash` передаётся как lowercase hex.

Hard limits фиксируются рядом с domain policy и публикуются в README:

- не более 8 FourCC, 16 resolutions, 16 FPS и 256 итоговых candidates;
- width/height `1..=8192`, FPS `0 < value <= 1000`;
- durations `1..=600_000 ms`;
- `minimum_fps_ratio` в `(0, 1]`, failure/long-gap ratios в `[0, 1]`,
  `maximum_gap_periods >= 1`;
- duplicate FourCC/resolution/FPS запрещены после нормализации;
- `candidate_deadline_ms >= first_frame_deadline_ms + warmup_ms +
  capture_only_ms`, `operation_deadline_ms >= candidate_deadline_ms`,
  `shutdown_deadline_ms <= candidate_deadline_ms`.

Operation deadline не обязан покрывать worst-case всех candidates: он остаётся
явным global budget и может штатно завершить длинную matrix как failed partial
report. UI показывает candidate count и budgets до старта.

Альтернативы: runtime-чтение внешнего JSON потребовало бы deployment path и
policy доступа к filesystem; hard-coded Rust/TypeScript defaults создали бы два
источника истины. Оба варианта отклонены.

### 3. Capture port расширяется metadata и mode diagnostics, но handle остаётся один

Существующий `CaptureSession` расширяется transport-neutral методами:

```text
apply_mode(ModeTuple) -> PropertySetDiagnostics
reported_properties() -> ReportedCaptureProperties
read() -> FrameRead::Empty | Frame(FrameMetadata)
release()
```

`PropertySetDiagnostics` хранит отдельные bool для FourCC, width, height и FPS.
`ReportedCaptureProperties` хранит nullable values: невозможность прочитать
diagnostic property не должна подменять actual frame gate. `FrameMetadata`
содержит только width/height; bytes и `Mat` не выходят из adapter в этом
change. Scan использует тот же `FrameRead::Frame(_)`, игнорируя metadata.

OpenCV adapter на каждом candidate создаёт новый `VideoCapture` с выбранным
MSMF/DSHOW, кодирует requested FourCC в CAP_PROP value, вызывает `set` в порядке
FourCC → width → height → FPS, затем отдельно читает CAP_PROP values. Actual
size берётся из каждого непустого `Mat.cols()/rows()`. Scoped session guard и
явный checked `release()` сохраняются.

Ошибки set/get/read/release получают typed adapter variants с source chain.
Необязательная reported property может стать `unavailable` diagnostic; потеря
session/read или release остаётся candidate/service failure согласно фазе.

Альтернатива — отдельный второй `ProfilingCaptureSession` — дала бы две
абстракции одного native handle и дублировала release semantics. Передача
`Mat` в domain отклонена из-за ownership/Send/Sync и ненужных bytes.

### 4. Service хранит scan context и profile context раздельно

Текущий один `OperationRecord` заменяется узкой структурой:

```text
ServiceInner
  state: Idle | Scanning | Profiling | ProfileReady | Faulted | Stuck
  active_operation: None | Scan | Profile
  last_scan: Option<ScanRecord>
  last_profile: Option<ProfileRecord>
  verified_modes: map<VerifiedModeId, VerifiedModeDescriptor>
  control: Option<OperationControl>
  supervisor: Option<JoinHandle<()>>
```

Хранится только последняя operation каждого kind, не unbounded history.
`last_scan` нужен для проверки endpoint после завершения scan; profile не
перезаписывает этот context. Один `active_operation`, один `control` и один
`supervisor` обеспечивают mutual exclusion. Новый accepted scan/profile, а
также `stop_camera` из `ProfileReady`, очищают registry verified modes до
запуска/ответа. Historical terminal report может оставаться доступным через
его `profileId`, но IDs из него становятся stale.

Scan snapshot после завершения хранит собственное terminal service state и не
проецирует последующее `Profiling/ProfileReady`. Благодаря этому существующие
`DeviceScanSnapshotV1` и его enum values не меняются. Profile имеет отдельные
snapshot/report types со своими state/status enums.

Альтернатива — общий публичный tagged union для scan/profile — потребовала бы
ломающего изменения уже выпущенного strict DTO v1. Два независимых service
instances отклонены, потому что не обеспечивают одного владельца камеры.

### 5. Profile worker — последовательная phase machine с двумя уровнями deadline

Для каждого tuple worker выполняет фазы:

```text
opening -> applying_properties -> reading_reported_properties
  -> first_frame -> warmup -> measuring -> release -> reopen_delay
```

Candidate epoch и timestamps публикуются в `ProfileRecord`. Worker отмечает
progress после каждого вернувшегося native call/frame; lock не удерживается во
время OpenCV работы. Warm-up дренирует frames до monotonic boundary, затем
создаётся новый accumulator и фиксируется measurement start. Последний read,
вернувшийся после nominal window boundary, учитывается вместе с фактическим
elapsed; это не позволяет скрыть блокировку искусственным обрезанием времени.

Внешний supervisor обслуживает:

- `candidate_deadline_ms`: выставляет epoch-specific timeout; после возврата
  native call candidate получает failure reason и worker может release/continue;
- `operation_deadline_ms`: запрашивает terminal cancellation всей matrix;
- user cancellation;
- `shutdown_deadline_ms`: переводит service в sticky `Stuck`, если после любого
  timeout/cancel native call не вернулся.

Один retry разрешён только до начала warm-up для transient open либо
first-read-start failure. Он использует тот же candidate deadline; result
сохраняет `attempt_count = 2` и retry reason. Measurement failure не retry-ится,
чтобы плохой run не маскировался удачным повтором. После каждого подтверждённого
release выполняется interruptible `reopen_delay`.

Альтернатива — один thread/capture на candidate или параллельная matrix —
отклонена из-за конфликтов driver, недостоверных FPS и нарушения ownership.
Небезопасное завершение thread и detach по-прежнему запрещены.

### 6. Metrics и gates вычисляются pure accumulator

Accumulator принимает monotonic `Duration`, вид read и actual dimensions. Он
хранит bounded vector timestamps максимум на число frames, физически возможное
в bounded measurement; дополнительный hard cap 100 000 samples fail-closed
защищает от ошибочного fake/driver loop. Frame bytes не сохраняются.

Формулы:

- `elapsed` — от начала measurement до terminal boundary/последнего
  вернувшегося read;
- `measured_fps = captured_frames / elapsed_seconds`;
- intervals — разность соседних successful frame timestamps;
- median — среднее двух центральных integer nanosecond values для чётного N;
- p95/p99 — nearest-rank над отсортированными integer intervals;
- nominal period — `1 / requested_fps`;
- long gap — interval строго больше двух nominal periods;
- read failure ratio — `read_failures / read_attempts`, при нуле attempts tuple
  не может быть verified.

Gate evaluator собирает все failure reasons. Приоритет display status фиксирован
от более ранней/сильной причины к поздней: open/first-frame/stall/cancel,
coerced resolution, under-target FPS, затем instability. Verified status
выбирается только после всех gates и зависит от reported FourCC match. Результат
`set()` не входит в gate.

Альтернатива — рассчитывать FPS по первому/последнему frame или CAP_PROP_FPS —
отклонена, потому что исключила бы задержку первого frame и повторила бы
непроверенное driver metadata.

### 7. verifiedModeId — registry capability, а не сериализованный tuple

После terminal gate service создаёт `VerifiedModeDescriptor` и process-local
opaque ID из service instance sequence, profile sequence, candidate ordinal и
config hash. ID не является security token и не содержит доверяемых клиентом
полей: lookup всегда идёт через registry, затем проверяются active profile,
endpoint generation, tuple и config hash.

В этом change реализуются:

- выдача и registry lookup;
- invalidation на accepted scan/profile/stop;
- pure `RevalidationGate` для exact resolution и short live FPS samples;
- application seam, через который следующий preview/recording worker обязан
  выполнить native reopen/warm-up/revalidation перед созданием downstream
  pipeline.

Публичная `revalidate_verified_mode` command сейчас не добавляется: без preview
или recording у пользователя нет операции, которую она могла бы безопасно
разрешить. Fake application tests всё равно проверяют stale lookup, config hash,
coercion и under-target revalidation. Следующий change подключит seam к своему
worker, не меняя identity contract.

Альтернатива — передать tuple во frontend и принять его обратно — отклонена:
клиент мог бы собрать не профилированную комбинацию или смешать generations.
Удержание capture handle после profile отклонено как нарушение reopen gate.

### 8. Profile transport добавляется рядом со scan v1 без изменения scan shape

Новые Rust DTO имеют `#[serde(deny_unknown_fields)]` и schema version 1:

- `StartProfileRequestV1 { endpoint_key, candidate_config }`;
- `ProfileStartedV1 { profile_id, endpoint, policy, config_hash,
  total_candidates }`;
- `ProfileOperationRequestV1 { profile_id }`;
- `ProfileProgressV1` с current tuple/phase и counters;
- `ProfileReportV1` с ordered results и aggregation.

`start_profile` и `get_profile_status` выполняют validation/state/snapshot access
быстро. Worker запускается отдельно. `cancel_profile` выполняется через
`spawn_blocking`, как scan cancellation. `get_profile_result` не блокирует и до
terminal state возвращает `PROFILE_NOT_READY`. Polling остаётся single-in-flight
и прекращается для `completed`, `cancelled`, `failed`, `stuck`.

Existing scan commands/DTO остаются byte-for-byte совместимыми. `stop_camera`
после profile возвращает существующий generic v1 shape с terminal
`service_state` и `operation = null`; profile detail читается profile commands.
Window shutdown использует внутренний stop path и не сериализует snapshot.

Ошибки разделены по границам:

- `ProfileConfigError` — shape, limits и inconsistent policy;
- расширенный `CaptureAdapterError` — OpenCV source errors;
- `CameraServiceError` — busy/stale/not-ready/ownership/orchestration;
- `CameraPublicErrorV1` — только stable code и безопасное русское сообщение.

Ожидаемые candidate failures входят в `CandidateResult`, а не превращаются в
command error. Неожиданная внутренняя ошибка логируется один раз на Tauri/CLI
boundary с `profileId`, candidate ordinal и safe phase; debug/source/path не
попадает в DTO. Новая logging dependency не вводится: используется текущий
stderr boundary и machine-readable report/evidence.

### 9. ProfileReport остаётся in-memory; запись evidence делает opt-in harness

GUI получает report через IPC, но application service не пишет его в arbitrary
path. Это сохраняет отсутствие Tauri filesystem permissions и persistence.
`ProfileReportV1` содержит bounded arrays, Unix epoch milliseconds начала,
environment version references, policy/hash, results и maxima. Поля
`runtime_validation_status`/`external_validation_status` допускаются только как
`not_run`; file artifact paths отсутствуют до recording change.

Отдельный `camera-mode-profile-smoke` binary:

1. принимает explicit bounded scan scope, endpoint backend/index, config и
   evidence path;
2. запускает реальный scan и выбирает совпавший endpoint из выданных keys;
3. выполняет profile через тот же service/application path;
4. атомарно пишет compact JSON и проверяет terminal cleanup/reopen;
5. возвращает non-zero при отсутствии требуемого hardware evidence.

Он запускается только через repository-local OpenCV wrapper и не входит в
обычный deterministic `bun run test`. Evidence ссылается на environment summary
и не содержит frame bytes/full environment. Многомегабайтных artifacts на этом
этапе нет.

Альтернатива — позволить GUI сохранять report — расширила бы security boundary
раньше появления пользовательского file workflow. Автоматический hardware run
в unit suite отклонён как нестабильный.

### 10. UI получает отдельный profile client и компонент

Чтобы не раздувать текущие `src/api/camera.ts` и `App.vue`, новые strict DTO,
decoders и `ProfilePollingController` размещаются в `src/api/profile.ts`, а
profile section — в небольшом Vue component. Parent передаёт выбранный endpoint
и событие accepted rescan; component владеет только draft config, poll loop,
presentation selection и safe error. Authoritative progress/report остаются в
Rust snapshots.

Accepted scan немедленно очищает profile component и verified selection.
Accepted profile очищает предыдущие maxima. Таблицы используют исходный order;
для равных maxima показываются все ties. `aria-live`, disabled semantics и
keyboard-accessible controls продолжают текущий plain Vue UI style; shadcn-vue
не добавляется, потому что проект его не использует.

Альтернатива — единый generic polling abstraction — отложена: scan и profile
имеют разные DTO, а преждевременная абстракция расширила бы change. Общая
маленькая helper function допустима только после появления фактического
дублирования.

### 11. Проверки строятся по requirement-to-evidence цепочке

Hardware-free Rust tests используют scripted session с отдельными scripts для
set/get/read/release и fake monotonic clock. Они покрывают validation limits,
order, retry, warm-up reset, metrics formulas, все gates, ties, registry,
service transitions, cancellation/deadlines/Stuck и public serialization.
Adapter-level tests проверяют explicit CAP_PROP mapping без real camera;
opt-in integration — фактический device.

TypeScript tests покрывают strict decoders, unknown schema/code,
single-in-flight polling и terminal stop. Vue tests покрывают explicit start,
selected generation, invalidation, progress, cancel, empty/partial results,
requested/reported/measured labels, ties, accessibility и отсутствие camera
calls при mount/self-check/scan-only flow. Existing scan/self-check/security,
runtime supply и bundle tests остаются regression gate.

## Risks / Trade-offs

- **[Native `read()` зависает во время долгого profile]** → внешний watchdog,
  epoch timeout, sticky `Stuck`, сохранённый join ownership и обязательный
  decision gate; unsafe termination не применяется.
- **[Matrix занимает слишком долго]** → hard candidate limit, global operation
  budget, показ estimated scope и cancellation; partial report не выдаётся за
  полный успешный profile.
- **[Low light/driver jitter делает результат нестабильным]** → raw monotonic
  metrics и все rejection reasons сохраняются; результат относится только к
  конкретным endpoint/config/run и не объявляется hardware capability вообще.
- **[OpenCV reported FourCC вводит в заблуждение]** → отдельные requested/set/
  reported/actual поля и status `verified_fourcc_unconfirmed`; native subtype
  не утверждается.
- **[Refactor service ломает готовый scan]** → scan record/DTO сохраняются,
  existing tests выполняются без изменения ожидаемых wire shapes, новые tests
  проверяют scan/profile mutual exclusion.
- **[Большой report перегружает IPC/UI]** → максимум 256 compact results, без
  frames/log dumps; status polling не передаёт весь report до terminal result.
- **[Canonical hash меняется из-за serializer refactor]** → отдельная versioned
  hash DTO и golden test; schema change требует нового hash schema/profile.
- **[Profile проходит, но будущий pipeline не выдерживает режим]** → result
  называется capture-verified, validation statuses остаются `not_run`, а
  downstream всегда выполняет reopen/revalidation и позднее full-pipeline gate.

## Migration Plan

1. Ввести profiling domain types, config fixture/schema, pure metrics/gates и
   fake-backed tests без изменения действующего scan transport.
2. Расширить capture port/adapter metadata и CAP_PROP diagnostics; адаптировать
   существующие scan fakes и доказать отсутствие scan regression.
3. Разделить `last_scan`/`last_profile`, добавить profile worker/watchdog и
   verified registry при сохранении одного supervisor/handle.
4. Подключить profile DTO/commands и TypeScript client, затем Vue component.
5. Выполнить deterministic quality gate, opt-in camera profile smoke и
   installed GUI check без auto camera access; обновить requirement matrix и
   оценить stall/native-subtype decision gates.

Data migration отсутствует. Rollback удаляет profile modules/commands/config/UI,
возвращает прежний scan-specific service layout и не меняет runtime manifest,
capabilities, CSP или пользовательские данные. Если hardware run обнаруживает
невосстанавливаемый stall, change не продолжается к preview: отдельно выбирается
helper process либо native Windows capture.
