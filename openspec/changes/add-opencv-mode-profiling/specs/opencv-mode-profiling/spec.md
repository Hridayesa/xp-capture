# Spec Delta

## Purpose

Обеспечить воспроизводимое capture-only профилирование конечного набора OpenCV
camera modes, измеряемые критерии пригодности и безопасную identity проверенных
tuples для последующих preview и recording capabilities.

## ADDED Requirements

### Requirement: Валидируемая конечная candidate policy

Система MUST иметь schema-versioned default `config/mode-candidates.json` и
MUST валидировать фактически переданную в `start_profile` candidate policy до
открытия camera device. Policy MUST содержать непустые конечные списки FourCC,
resolutions и FPS, а также положительные `warmupMs`, `captureOnlyMs`,
`firstFrameDeadlineMs`, `candidateDeadlineMs`, `operationDeadlineMs`,
`shutdownDeadlineMs`, `reopenDelayMs` и thresholds
`minimumFpsRatio`, `maximumReadFailureRatio`, `maximumGapPeriods`,
`maximumLongGapRatio`.

FourCC MUST состоять ровно из четырёх ASCII characters; width, height и FPS
MUST быть положительными; floating-point values MUST быть конечными; ratios
MUST находиться в документированных диапазонах; durations и общее число tuples
MUST быть ограничены hard limits. Deadlines MUST быть согласованы так, чтобы
`candidateDeadlineMs` покрывал first-frame, warm-up и measurement phases, а
`operationDeadlineMs` не был короче одного candidate и
`shutdownDeadlineMs` не превышал operation deadline. Итоговые snapshot и
report MUST содержать нормализованную применённую policy, schema version и её
stable SHA-256 hash.

#### Scenario: Валидная default policy принята

- **WHEN** пользователь запускает profiling с version 1 policy, содержащей
  допустимые уникальные FourCC, resolutions, FPS, durations и thresholds
- **THEN** система возвращает `profileId`, количество конечных tuples,
  нормализованную policy и её hash до завершения первого candidate

#### Scenario: Некорректная policy не открывает камеру

- **WHEN** policy имеет неизвестную schema, пустой список, duplicate candidate
  value, FourCC иной длины, неположительный или non-finite размер/FPS,
  threshold вне диапазона, несогласованный deadline либо превышает hard limit
- **THEN** `start_profile` возвращает `INVALID_CONFIG`, не создаёт profile
  operation и не вызывает camera adapter

#### Scenario: Переполнение cartesian product отклоняется

- **WHEN** произведение количества FourCC, resolutions и FPS превышает
  установленный maximum candidate count либо не может быть вычислено безопасно
- **THEN** запрос отклоняется с `INVALID_CONFIG` до выделения result array и до
  camera access

### Requirement: Актуальный endpoint и детерминированный план tuples

Profiling MUST принимать только opaque `DeviceEndpointKey` из последнего
успешного scan текущего process и MUST проверять его связь с текущим
`scanGeneration`, backend и numeric index. Система MUST строить полный
cartesian product `(FourCC, resolution, FPS)` в документированном порядке
массивов policy и MUST проверять каждый tuple ровно один раз, кроме одного явно
учтённого retry для transient open/first-read-start failure. Система MUST NOT
использовать binary search и MUST NOT предполагать, что промежуточные значения
FPS поддерживаются непрерывно.

#### Scenario: Полный план сохраняет порядок policy

- **WHEN** policy содержит несколько FourCC, resolutions и FPS
- **THEN** progress и итоговый report перечисляют полный cartesian product в
  стабильном порядке policy, включая rejected tuples, без синтеза
  отсутствующих FPS

#### Scenario: Endpoint предыдущего scan отклоняется

- **WHEN** `start_profile` получает endpoint из предыдущего `scanGeneration`,
  другого process либо не из последнего успешного scan
- **THEN** система возвращает `STALE_DEVICE_ENDPOINT`, не создаёт worker и не
  открывает numeric index из stale key

#### Scenario: Tuples разных endpoints не смешиваются

- **WHEN** одинаковый numeric index доступен через MSMF и DSHOW
- **THEN** profile относится ровно к выбранному `DeviceEndpointKey`, сохраняет
  его backend и не объединяет результаты с другим endpoint как с одной камерой

### Requirement: Независимое открытие и наблюдаемые свойства каждого tuple

Для каждого candidate система MUST создать новую capture session с явным
backend выбранного endpoint, затем запросить FourCC, width, height и FPS в
детерминированном порядке. Система MUST сохранить boolean result каждого
`set()`, отдельно прочитать reported FourCC/width/height/FPS и MUST полностью
освободить session до `reopenDelayMs` и открытия следующего candidate.

Boolean result `set()` MUST быть только diagnostic: `false` MUST NOT сам по
себе проваливать фактически работающий tuple, а `true` MUST NOT подтверждать
режим при несовпадающем actual frame size или недостаточном measured FPS.
Ошибки open или first-read-start отдельного candidate MUST становиться его
machine-readable outcome и не останавливать остальные candidates, если
cleanup подтверждён и service-level watchdog не сработал.

#### Scenario: Set false, но фактический режим проходит

- **WHEN** один или несколько `set()` возвращают `false`, но reported и actual
  данные позволяют завершить measurement и все capture gates проходят
- **THEN** tuple может получить verified capture status, а report сохраняет
  каждый `set=false` без преобразования его в автоматический failure

#### Scenario: Set true, но resolution coerced

- **WHEN** все `set()` возвращают `true`, но хотя бы один измеряемый frame имеет
  width или height, отличный от requested resolution
- **THEN** tuple получает `coerced_resolution`, не получает `verifiedModeId` и
  не участвует в `maxVerifiedByResolution`

#### Scenario: Cleanup кандидата не подтверждён

- **WHEN** release session завершается ошибкой либо native call не возвращается
  к `shutdownDeadlineMs`
- **THEN** система не открывает следующий candidate, operation завершается
  failed или `Stuck` согласно подтверждённому состоянию ownership и не сообщает
  успешный cleanup

### Requirement: Непрерывный warm-up и capture-only measurement

После открытия candidate первый непустой frame MUST поступить до
`firstFrameDeadlineMs`. Затем система MUST непрерывно читать и отбрасывать
frames в течение `warmupMs`; замена drain циклом `sleep` запрещена. После
последнего warm-up frame система MUST сбросить counters и timestamps и выполнить
отдельное capture-only окно продолжительностью `captureOnlyMs`. Ожидание первого
измеряемого frame MUST входить в elapsed time окна.

Для каждой read attempt система MUST использовать monotonic time, учитывать
успешный непустой frame, empty result и read failure и проверять actual
`width × height` каждого принятого frame. Requested, boolean `set`, reported и
actual values MUST оставаться раздельными в progress/result DTO.

#### Scenario: Накопленный warm-up burst не завышает FPS

- **WHEN** fake backend возвращает накопленный burst frames во время warm-up, а
  после сброса counters выдаёт frames медленнее requested FPS
- **THEN** warm-up frames не входят в capture-only counters и tuple оценивается
  только по timestamps measurement window

#### Scenario: Первый измеряемый frame задержан

- **WHEN** после сброса counters первый frame приходит с задержкой, но до
  candidate deadline
- **THEN** задержка входит в `elapsedMs` и уменьшает `measuredFps`, а начало
  окна не переносится на timestamp первого frame

#### Scenario: Cancel во время warm-up или measurement

- **WHEN** пользователь вызывает `cancel_profile` во время warm-up либо
  capture-only measurement и текущий native call возвращается
- **THEN** новые reads/candidates не начинаются, session освобождается, текущий
  tuple получает `cancelled`, а terminal report сохраняет уже завершённые
  результаты без выдачи partial tuple как verified

### Requirement: Monotonic metrics и capture gates

Для каждого измеренного tuple система MUST публиковать как минимум
`elapsedMs`, `readAttempts`, `capturedFrames`, `emptyFrames`, `readFailures`,
`measuredFps = capturedFrames / elapsedSeconds`, median, p95 и p99 inter-frame
interval, maximum inter-frame gap, count/ratio gaps больше двух nominal frame
periods и reported properties. Interval metrics MUST строиться только по
соседним успешным frames после warm-up и MUST быть явно unavailable при числе
frames меньше двух.

Tuple MUST получить verified capture status только если каждый принятый frame
имеет exact requested resolution, получено не менее двух frames,
`measuredFps >= requestedFps × minimumFpsRatio`,
`readFailures / readAttempts <= maximumReadFailureRatio`, maximum gap не больше
`maximumGapPeriods × nominalPeriod` и long-gap ratio не больше
`maximumLongGapRatio`. Фактически применённые thresholds MUST сохраняться в
report и MUST NOT описываться как production SLA.

Machine-readable `captureModeStatus` MUST различать как минимум
`opening_failed`, `first_frame_timeout`, `read_stalled`,
`coerced_resolution`, `capture_under_target`, `capture_unstable`,
`verified_fourcc_reported_match`, `verified_fourcc_unconfirmed` и `cancelled`;
`failureReasons` MUST сохранять все сработавшие gates, а не только первый.

#### Scenario: Delivered FPS ниже threshold

- **WHEN** reported FPS равен requested FPS, но monotonic measurement даёт
  `measuredFps` ниже configured ratio
- **THEN** tuple получает `capture_under_target`, reported FPS остаётся только
  diagnostic и tuple не считается verified

#### Scenario: Read failures или gaps делают поток нестабильным

- **WHEN** средний FPS проходит ratio, но read-failure ratio, maximum gap либо
  long-gap ratio превышает policy
- **THEN** tuple получает `capture_unstable`, report перечисляет каждый
  нарушенный gate и tuple не участвует в выборе максимума

#### Scenario: FourCC reported match не выдаётся за native subtype

- **WHEN** tuple проходит все capture gates и reported FourCC совпадает с
  requested FourCC
- **THEN** статус равен `verified_fourcc_reported_match`, но report и UI явно
  указывают, что native driver media subtype не был перечислен или доказан

#### Scenario: FourCC не подтверждён, но capture gates проходят

- **WHEN** tuple проходит resolution/FPS/stability gates, а reported FourCC
  отсутствует или не совпадает с requested
- **THEN** статус равен `verified_fourcc_unconfirmed`, requested и reported
  значения показаны раздельно и результат не переименовывается в подтверждённый
  native subtype

### Requirement: ProfileReport, максимум по resolution и verifiedModeId

Terminal `ProfileReport` schema version 1 MUST содержать `profileId`, время
начала, environment/version references, выбранный endpoint со
`scanGeneration`, применённую policy и config hash, ordered result каждого
tuple, terminal status/failure, а также `maxVerifiedByResolution`. Поля
full-pipeline и external validation, если зарезервированы schema, MUST иметь
только значение `not_run` в этом change и MUST NOT создавать утверждение
`provisional_verified` или `externally_validated`.

Aggregation MUST группировать только по
`(DeviceEndpointKey, requestedWidth, requestedHeight)` и MUST выбирать
наибольший measured FPS только среди capture-verified tuples. При равном
requested или measured FPS с разными requested FourCC report MUST сохранить
все равные candidates либо применить явно указанную в policy tie-break policy;
скрытое предпочтение FourCC запрещено.

Каждый capture-verified tuple MUST получить opaque backend-issued
`verifiedModeId`, связанный с `profileId`, `DeviceEndpointKey`,
`scanGeneration`, tuple и config hash. Новый принятый scan или profile MUST
сделать ранее выданный id stale. Любой будущий consumer `verifiedModeId` MUST
до preview/recording заново открыть tuple, непрерывно прогреть его и подтвердить
exact resolution и короткое live FPS окно; stale id или coercion MUST быть
отклонены и MUST NOT разрешать downstream operation.

#### Scenario: Максимум выбирается только из verified tuples

- **WHEN** для одного endpoint/resolution проверены несколько FPS и часть
  tuples rejected, а часть capture-verified
- **THEN** `maxVerifiedByResolution` ссылается на verified tuple с максимальным
  измеренным FPS и никогда не выбирает более высокий rejected FPS

#### Scenario: Равные FourCC не получают скрытого приоритета

- **WHEN** два FourCC для одного endpoint/resolution дают одинаковый лучший
  результат и tie-break policy не настроена
- **THEN** report и UI показывают оба результата как равные, не выбирая MJPG,
  YUY2 или иной FourCC неявно

#### Scenario: Stale verified mode отклоняется

- **WHEN** consumer предъявляет `verifiedModeId` после нового scan/profile,
  изменения config hash или с endpoint другого generation
- **THEN** camera service возвращает `STALE_VERIFIED_MODE` до открытия камеры

#### Scenario: Reopen coercion отзывает разрешение downstream operation

- **WHEN** актуальный `verifiedModeId` проходит identity validation, но при
  обязательном reopen/revalidation actual resolution или короткий FPS gate не
  подтверждается
- **THEN** revalidation возвращает `MODE_COERCED` или `UNDER_TARGET_FPS`, а
  preview/recording не запускается

### Requirement: Versioned profile transport и polling

Система MUST предоставлять versioned commands `start_profile`,
`get_profile_status`, `get_profile_result` и `cancel_profile`. Requests,
successful responses, progress/result DTO и public errors MUST иметь
`schema_version = 1`; frontend MUST выполнять strict runtime decoding и
управляемо отклонять unknown fields, enum values или schema. Long-running
profiling и ожидание cleanup MUST выполняться вне UI thread.

`get_profile_status` MUST быстро возвращать authoritative service/operation
state, `completedCandidates`, `totalCandidates`, текущий tuple и phase как
минимум `opening`, `first_frame`, `warmup`, `measuring` или `reopen_delay`.
`get_profile_result` MUST возвращать report только для terminal текущей/последней
profile operation. Public error codes MUST включать как минимум
`INVALID_CONFIG`, `BUSY`, `STALE_DEVICE_ENDPOINT`,
`STALE_PROFILE_OPERATION`, `PROFILE_NOT_READY`, `READ_TIMEOUT`,
`READ_STALLED`, `CANCELLED`, `STALE_VERIFIED_MODE`, `MODE_COERCED`,
`UNDER_TARGET_FPS` и `INTERNAL`, не раскрывая OpenCV debug output, paths,
environment values или source chain.

#### Scenario: Polling показывает progress без блокировки UI

- **WHEN** profiling выполняется и frontend опрашивает текущий `profileId`
- **THEN** каждый ответ согласованно показывает current candidate/phase и
  progress, одновременно остаются отзывчивыми self-check и остальной UI

#### Scenario: Result запрошен до завершения

- **WHEN** `get_profile_result` вызывается для активной operation
- **THEN** система возвращает safe `PROFILE_NOT_READY` и не выдаёт partial
  report как terminal result

#### Scenario: Неизвестный profileId отклоняется безопасно

- **WHEN** status/result/cancel запрошен для profile operation, которая не
  является текущей или последней известной operation process
- **THEN** система возвращает `STALE_PROFILE_OPERATION` без раскрытия
  внутренних identifiers или native diagnostics

### Requirement: Profiling UI, evidence и совместимость

Diagnostic UI MUST разрешать profiling только после выбора endpoint текущего
scan, показывать применяемые candidates/thresholds, progress
`candidate X of N`, текущий tuple/phase, safe cancellation, ordered таблицу всех
results и сгруппированный `resolution → maximum measured FPS → FourCC/backend`.
Requested, reported и measured values MUST иметь разные labels; rejected tuple
MUST NOT быть selectable как verified; UI MUST показывать предупреждение, что
OpenCV список эмпирический и может быть неполным.

Profiling MUST начинаться только по явному действию пользователя. Startup,
runtime self-check, device scan и выбор endpoint MUST NOT запускать profile.
Change MUST сохранять существующие runtime/installer contracts, не добавлять
filesystem/network Tauri permissions, не ослаблять CSP и не вводить persistence
или migration пользовательских данных. Opt-in Windows hardware smoke MUST
сохранять compact schema-versioned JSON с policy/hash, ordered outcomes,
metrics и environment version references без frame bytes, secrets или full
environment dump.

#### Scenario: Явный profile из выбранного endpoint

- **WHEN** scan завершён, пользователь выбрал endpoint текущего generation и
  нажал profile
- **THEN** UI очищает прежние verified selections, запускает ровно одну profile
  operation и отображает authoritative progress до terminal state

#### Scenario: Startup и scan остаются без profiling

- **WHEN** приложение запускается, выполняется self-check или device scan без
  нажатия profile
- **THEN** ни один mode tuple не настраивается и profiling worker не создаётся

#### Scenario: Нет verified modes

- **WHEN** все candidates rejected или profiling отменён до первого verified
  result
- **THEN** UI показывает управляемое empty/partial состояние,
  `maxVerifiedByResolution` пуст и никакое downstream действие с mode не
  разрешено

#### Scenario: Rollback не требует migration

- **WHEN** profile commands, UI и mode config удаляются при rollback change
- **THEN** существующие self-check и camera scan продолжают работать без
  преобразования пользовательских данных, capabilities или CSP

