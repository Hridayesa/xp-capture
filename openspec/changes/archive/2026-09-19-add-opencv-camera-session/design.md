# Design

## Context

См. `proposal.md` — Why. Текущий Rust crate содержит только camera-independent
`self_check`: Tauri создаёт service на каждый вызов команды, выполняет native
работу через `spawn_blocking` и возвращает versioned DTO. Постоянного
application state, background worker или camera module пока нет. Vue UI также
имеет только self-check client и component states.

OpenCV 4.12.0 и crate `opencv` 0.100.1 уже закреплены, а installer доказанно
поставляет MSMF/DSHOW runtime. Новый slice должен использовать этот supply без
обновления dependencies. Он не может полагаться на то, что blocking
`VideoCapture::open/read` поддерживает безопасное внешнее прерывание или
`CAP_PROP_READ_TIMEOUT_MSEC` для MSMF/DSHOW.

Поведение задаёт `specs/opencv-camera-session/spec.md`. Полная дорожная карта
сохранена в корневом `opencv-poc-roadmap.md`.

## Goals / Non-Goals

**Goals:**

- Ввести расширяемые domain/application границы camera session до появления
  profiling, preview и recording.
- Сохранить `VideoCapture` внутри одного dedicated worker и сделать ownership,
  cancellation, deadlines и cleanup наблюдаемыми.
- Обеспечить детерминированные hardware-free tests через fake adapter и один
  отдельный hardware smoke, подтверждающий реальный OpenCV open/read/release.
- Расширить diagnostic UI вертикальным scan slice, не меняя существующий
  self-check и installer runtime contract.

**Non-Goals:**

- Держать camera открытой после успешного probe.
- Применять resolution/FPS/FourCC, измерять throughput или выпускать
  `verifiedModeId`.
- Передавать preview frames, создавать `OwnedFrame` либо writer queue.
- Перечислять friendly names, Windows device IDs или driver-advertised modes.
- Автоматически восстанавливаться из `Stuck` либо изолировать capture в helper
  process; `Stuck` является decision gate для следующего change.
- Добавлять persistence, telemetry service или новую UI library.

## Decisions

### 1. Модульные границы и направление зависимостей

Новый Rust slice разделяется на четыре слоя:

```text
Vue camera UI --> TypeScript camera client --> Tauri camera commands
                                                |
                                                v
                                         CameraService
                                         /     |     \
                                  domain   worker   watchdog
                                                |
                                                v
                                      CaptureBackend port
                                         /           \
                                  OpenCV adapter    fake adapter
```

Предлагаемая структура:

```text
src-tauri/src/camera/
  mod.rs
  model.rs          domain types, states, policies and snapshots
  error.rs          config, adapter and application errors
  ports.rs          capture backend boundary and monotonic clock seam
  service.rs        lifecycle and use-cases
  worker.rs         sequential probe loop
  watchdog.rs       deadline/cancellation supervision
  transport.rs      versioned DTO and public error mapping
  adapters/
    mod.rs
    opencv.rs
src/api/camera.ts   invoke wrapper and strict DTO decoders
```

Domain/application code не импортирует Tauri, Vue или concrete OpenCV types.
`transport.rs` единожды преобразует внутренние результаты и errors в публичный
контракт. `opencv::videoio::VideoCapture` существует только внутри adapter,
который целиком создаётся и используется camera worker.

Альтернатива — добавить camera calls прямо в `lib.rs` по образцу простого
self-check — отклонена: persistent operation state, cancellation и будущие
profiling/preview transitions быстро смешали бы Tauri transport с native
ownership.

### 2. Один camera worker и отдельный watchdog

Каждая принятая scan operation создаёт:

- один camera worker, владеющий adapter и всеми `VideoCapture` instances;
- один watchdog, который не вызывает OpenCV, а наблюдает monotonic progress,
  operation deadline, cancellation timestamp и worker completion;
- shared operation record под короткоживущей синхронизацией;
- cancellation flag и notification primitive;
- сохранённые join handles, принадлежащие `CameraService`.

Service lock никогда не удерживается во время `open`, `read`, `release`, sleep
или join. Worker публикует immutable snapshot updates после значимых переходов:
начало probe, open result, первый frame, release и terminal result. Watchdog
использует condition-variable timeout либо эквивалентное ожидание, а не busy
loop.

Параллельные probes отклонены. Последовательность строится как ordered
cartesian product backends и inclusive index range. Worker проверяет
cancellation перед каждым probe, между возвращающимися native calls и перед
`reopenDelayMs`.

Если worker завершился, service join-ит его и только после этого публикует
успешный cleanup. Если watchdog достиг shutdown deadline при незавершённом
worker, он атомарно устанавливает sticky `Stuck`; join handle сохраняется и
новая operation запрещается. Поздний возврат worker может быть записан во
внутреннюю диагностику, но не возвращает process в `Idle`: после такого driver
stall дальнейшая in-process camera работа считается недостоверной.

Альтернатива — timeout внутри camera worker — не обнаруживает зависший native
call. Unsafe thread termination и `JoinHandle` drop с продолжением обычной
работы отвергнуты, потому что не подтверждают release device handle.

### 3. State и operation record

`CameraService` хранит только текущую/последнюю operation и process-local
generation counter; истории и persistence нет.

```text
Idle ----start----> Scanning ----complete----> Idle
 |                    |   |
 |                    |   +----service failure after cleanup----> Faulted
 |                    |
 |                    +----cancel + cleanup----> Idle
 |                    |
 |                    +----shutdown deadline----> Stuck
 |
 +<----stop/reset confirmed from Faulted---------+

Stuck ----> restart process only
```

Operation record содержит policy, `operationId`, `scanGeneration`, timestamps,
current tuple, completed/total counts, ordered probe outcomes, endpoints,
terminal status и безопасный failure code. Domain invariants проверяют, что:

- accepted start использует checked increment generation;
- completed count не превышает total;
- endpoint существует только для `available` probe того же generation;
- `Idle` не имеет живого worker;
- `Stuck` не может перейти обратно в рабочее состояние.

Обычные `open_failed`/`read_failed` являются probe outcomes, а не
`CameraServiceError`. `Faulted` зарезервирован для ошибки orchestration,
синхронизации или cleanup после того, как worker уже подтверждённо завершён.

### 4. Scan policy и defaults

Transport request преобразуется в валидированный `DeviceScanPolicy` до
изменения state. Начальные UI defaults:

- `firstIndex = 0`, `lastIndex = 5`;
- `backends = [MSMF, DSHOW]`;
- `firstFrameDeadlineMs = 5000` от начала probe до первого непустого frame;
- `operationDeadlineMs = 90000` для всей scan operation;
- `shutdownDeadlineMs = 3000` после cancellation/watchdog trigger;
- `reopenDelayMs = 500` после подтверждённого release.

Defaults являются PoC policy и возвращаются в snapshot. Domain validation
вводит конечные hard limits: не более 32 indices и 64 total probes, durations
не выше 10 минут, `operationDeadlineMs > firstFrameDeadlineMs`, а
`shutdownDeadlineMs` не превышает operation deadline. Конкретные constants
документируются рядом с request DTO и покрываются boundary tests; их изменение
после выпуска transport v1 требует обновления spec/change, если меняется
принимаемый контракт.

`firstFrameDeadlineMs` контролирует watchdog начиная с начала probe, поэтому
охватывает как затянувшийся open, так и ожидание первого frame. Он не обещает
принудительно прервать native call: после deadline запрашивается cancellation,
а затем применяется `shutdownDeadlineMs`/`Stuck` policy.

### 5. OpenCV adapter и release semantics

Adapter принимает typed `CaptureBackend::{Msmf,Dshow}` и numeric index, создаёт
новый `VideoCapture` с явным `apiPreference` и не использует `CAP_ANY`.
Успешный probe требует `is_opened()` и первого `read()` с непустым `Mat`.
Размер или pixel format в этом change не являются gate: frame используется
только как доказательство поступления данных и не покидает worker.

Каждый probe использует scoped session guard. Нормальный путь вызывает явный
`release()` и сохраняет его result; guard остаётся последней защитой при раннем
return. Endpoint публикуется только после кадра, но следующий probe начинается
только после успешного выхода из session scope. Ошибка release завершает всю
operation как service failure, потому что concurrent reuse устройства после
неподтверждённого cleanup небезопасен.

Альтернатива `CAP_ANY` отклонена: она скрыла бы фактический backend и сделала
сравнение MSMF/DSHOW недостоверным. Кэширование открытого handle после scan
отложено до change, где появится revalidation и preview lifecycle.

### 6. Versioned transport и polling

Tauri регистрирует четыре команды:

```text
start_device_scan(requestV1)       -> DeviceScanStartedV1
get_device_scan(operationId)       -> DeviceScanSnapshotV1
cancel_device_scan(operationId)    -> DeviceScanSnapshotV1
stop_camera()                      -> CameraServiceSnapshotV1
```

`CameraService` создаётся один раз при GUI startup и передаётся командам через
managed state. `start` и `get` выполняют только validation/state access и быстро
возвращаются. `cancel`/`stop`, если им нужно ожидать worker completion, делают
это вне UI thread через Tauri blocking runtime boundary.

Snapshot polling выбран вместо Tauri events на первом шаге: payload мал,
progress редок, polling проще корректно остановить при terminal state и легче
проверить contract tests. Frontend использует один poll loop, не запускает
следующий request до завершения предыдущего и прекращает polling при
`completed`, `cancelled`, `failed` или `stuck`.

Все DTO имеют schema version 1 и deny/validate unknown shape на соответствующей
границе. Rust использует snake_case JSON, TypeScript decoder централизованно
проверяет fields/enums и не доверяет `invoke<SomeType>` без runtime validation.
`operationId` и `DeviceEndpointKey` являются opaque strings для frontend.

### 7. Модель ошибок

Ошибки разделяются по границам:

- `CameraConfigError` — ожидаемые ошибки входной policy;
- `CaptureAdapterError` — OpenCV open/read/release с сохранённым source;
- `CameraServiceError` — busy, stale operation, orchestration и state errors;
- `CameraPublicErrorV1` — стабильный code и безопасное сообщение.

Один transport mapper сопоставляет внутренние variants с codes из spec.
Внутренний error логируется один раз на Tauri/application boundary вместе с
`operationId`, backend/index и безопасным phase marker. Debug representation,
paths, environment и source chain не попадают в DTO. Probe-level ожидаемые
failures сериализуются как outcomes без внутреннего текста.

Production camera paths не используют `panic`, `unwrap()` или `expect()` для
request data, locks, OpenCV и thread lifecycle. Poisoned synchronization или
worker panic преобразуются в `INTERNAL`; если владение native handle после
этого нельзя подтвердить, состояние становится `Stuck`, а не восстанавливается
молча.

### 8. UI state ownership

Authoritative service/operation state находится в Rust snapshot. TypeScript
client владеет только transport/polling, а Vue component — draft request,
выбранным endpoint текущего generation и отображением.

Новый accepted start немедленно очищает frontend selection. Terminal snapshot
заменяет список endpoints целиком. UI не объединяет одинаковые indices разных
backends и показывает предупреждение, что OpenCV scan эмпирический и numeric
identity нестабильна.

Self-check остаётся отдельным flow. Ни mount компонента, ни GUI startup, ни
`run_self_check` не вызывают camera commands. Существующий installed GUI smoke
поэтому остаётся camera-independent. Новые commands не требуют Tauri plugin
permissions; capability permissions остаются пустыми, CSP не расширяется.

### 9. Tests и evidence

Hardware-free Rust tests используют scripted fake adapter и fake monotonic
clock/notifications для validation boundaries, tuple order, empty frames,
open/read/release failures, cancellation, concurrent start, operation timeout,
sticky `Stuck`, late worker completion и absence of second worker. Transport
tests проверяют schema, stable codes и отсутствие internal marker в JSON.

Frontend tests используют mocked centralized client и fake timers для polling,
selection invalidation, empty result, cancel, safe error и stuck UI. Existing
self-check tests остаются обязательными и получают regression scenario, что
camera scan не запускается автоматически.

Отдельный opt-in Windows hardware smoke использует pinned OpenCV wrapper,
explicit backend/index range и не входит в обычный deterministic unit run. Для
завершения change требуется compact evidence как минимум одного из двух честных
результатов:

- найден хотя бы один endpoint, получен первый frame и последующий повторный
  scan доказывает release;
- на явно camera-less host scan штатно завершился пустым результатом.

Второй результат подтверждает no-camera path, но не доказывает hardware open;
если доступная камера существует, hardware smoke с найденным endpoint остаётся
обязательным. Evidence не сохраняет frames, полный environment или secrets.

## Risks / Trade-offs

- **[MSMF/DSHOW call блокируется внутри драйвера]** -> внешний watchdog,
  cancellation flag, sticky `Stuck` и запрет следующей operation; результат
  используется как decision gate для helper process/native capture.
- **[Один physical device появляется дважды]** -> endpoints намеренно не
  объединяются без Windows identity mapping; backend всегда видим пользователю.
- **[Polling создаёт лишние IPC calls]** -> только один pending request и
  умеренный interval; payload ограничен максимум 64 probe outcomes.
- **[Ошибки probe выглядят как global failure]** -> domain явно разделяет
  outcomes и service errors; UI показывает пустой/частичный scan отдельно от
  `Faulted`.
- **[Поздно вернувшийся worker после `Stuck`]** -> state остаётся sticky и
  запрещает reuse process, даже если handle позднее освободился.
- **[Hardware tests нестабильны в CI]** -> scripted tests остаются
  детерминированными, hardware smoke opt-in и сохраняет environment/policy в
  evidence; он не подменяется mock-тестом.

## Migration Plan

1. Добавить domain/application/adapter modules и полностью проверить их через
   fake backend до регистрации Tauri commands.
2. Подключить один managed `CameraService`, versioned transport и contract
   tests, сохранив существующий `run_self_check` без изменений semantics.
3. Добавить TypeScript client и camera section в текущий diagnostic UI; camera
   access остаётся только за явной кнопкой scan.
4. Выполнить deterministic project checks, затем opt-in real-camera smoke и
   установленный GUI startup без автоматического camera access.
5. Зафиксировать evidence и оценить `Stuck` decision gate до начала
   `add-opencv-mode-profiling`.

Persistence и data migration отсутствуют. Rollback удаляет camera module,
commands/client/UI section и возвращает self-check-only application; runtime
manifest, installer и пользовательские данные не изменяются.
