# Spec Delta

## Purpose

Обеспечить ограниченное и наблюдаемое обнаружение OpenCV camera endpoints на
Windows, безопасный lifecycle единственной camera session и versioned UI/API
контракт, на который смогут опираться последующие profiling, preview и
recording capabilities.

## ADDED Requirements

### Requirement: Валидируемый ограниченный scan

Система MUST принимать versioned запрос scan с конечным диапазоном numeric
indices, непустым упорядоченным набором поддерживаемых backends, положительными
`firstFrameDeadlineMs`, `operationDeadlineMs`, `shutdownDeadlineMs` и
`reopenDelayMs`. Система MUST отклонять диапазон, backend или deadlines вне
документированных безопасных границ с кодом `INVALID_CONFIG` до открытия
camera device.

Поддерживаемыми backends этого change являются `MSMF` и `DSHOW`; порядок из
валидного запроса MUST сохраняться. Defaults MAY выбирать `MSMF` первым и
`DSHOW` вторым, но MUST передаваться в итоговом snapshot, чтобы фактическая
политика scan была наблюдаемой.

#### Scenario: Валидный bounded scan запускается

- **WHEN** пользователь запускает scan с допустимым диапазоном indices,
  backends и deadlines
- **THEN** система создаёт ровно одну конечную scan operation, возвращает её
  `operationId` и полную применённую policy без ожидания завершения всех probes

#### Scenario: Некорректный запрос отклоняется до camera access

- **WHEN** `firstIndex > lastIndex`, диапазон превышает установленный предел,
  список backends пуст, backend неизвестен либо deadline неположителен или
  несогласован
- **THEN** система возвращает `INVALID_CONFIG`, не увеличивает
  `scanGeneration` и не пытается открыть ни один device index

### Requirement: Последовательное обнаружение camera endpoints

Система MUST проверять каждый tuple `(backend, numericIndex)` последовательно и
MUST освободить native handle предыдущего probe до открытия следующего.
Endpoint MUST считаться обнаруженным только если device успешно открыт и до
`firstFrameDeadlineMs` получен хотя бы один непустой кадр. Успешный `open()` без
кадра, ошибка чтения или истечение deadline MUST NOT добавлять endpoint в список
доступных устройств.

Результат MUST содержать progress и machine-readable outcome каждого
завершённого probe, включая как минимум `available`, `open_failed`,
`first_frame_timeout`, `read_failed` и `cancelled`. Ошибка отдельного probe,
включая занятую другим приложением камеру, MUST NOT останавливать оставшиеся
probes, если service-level watchdog не перевёл операцию в terminal failure.

#### Scenario: Endpoint подтверждён реальным кадром

- **WHEN** конкретный `(backend, numericIndex)` успешно открывается и возвращает
  непустой кадр до deadline
- **THEN** итоговый snapshot содержит ровно один доступный endpoint для этого
  tuple и показывает его backend, numeric index и успешный probe outcome

#### Scenario: Open без кадра не считается доступной камерой

- **WHEN** `open()` завершился успешно, но до deadline получены только пустые
  кадры либо чтение завершилось ошибкой
- **THEN** tuple получает соответствующий rejection outcome, отсутствует в
  списке доступных endpoints, native handle освобождается, а scan продолжает
  следующий tuple

#### Scenario: Одна физическая камера доступна через два backend

- **WHEN** одинаковый numeric index проходит probe через `MSMF` и `DSHOW`
- **THEN** система возвращает два независимых endpoints и не утверждает, что
  они представляют одну physical device

#### Scenario: Камеры отсутствуют

- **WHEN** ни один проверенный tuple не возвращает непустой кадр
- **THEN** scan завершается успешно с пустым списком endpoints и сохранёнными
  probe outcomes, а UI показывает управляемое состояние `no camera`

### Requirement: Эфемерная identity endpoint

Каждый принятый scan MUST получать monotonically increasing в пределах process
`scanGeneration`. Каждый доступный endpoint MUST иметь opaque
`DeviceEndpointKey`, связанный с `scanGeneration`, backend и numeric index, и
display name вида `Camera {index} / {backend}`. Numeric index или display name
MUST NOT объявляться стабильной identity и MUST NOT сохраняться для повторного
использования после нового scan или restart приложения.

#### Scenario: Новый scan инвалидирует предыдущий выбор

- **WHEN** пользователь запускает новый валидный scan после выбора endpoint из
  предыдущего поколения
- **THEN** предыдущий UI selection немедленно очищается, новый snapshot получает
  другое `scanGeneration`, а старый `DeviceEndpointKey` не предлагается как
  актуальный endpoint

#### Scenario: Restart не восстанавливает numeric endpoint

- **WHEN** приложение запускается заново после ранее успешного scan
- **THEN** camera selection отсутствует до нового scan и приложение не открывает
  сохранённый numeric index автоматически

### Requirement: Единственная camera operation и наблюдаемая state machine

Camera service MUST допускать не более одной активной scan operation и одного
camera worker. Состояние service MUST быть доступно как одно из `Idle`,
`Scanning`, `Faulted` или `Stuck`; snapshot активной операции MUST содержать
terminal или non-terminal status, количество завершённых и общее количество
probes и текущий tuple, когда он известен.

Новая операция MUST быть разрешена только из `Idle`. Обычные outcomes отдельных
probes MUST оставаться частью результата scan и MUST NOT сами по себе переводить
service в `Faulted`.

#### Scenario: Concurrent start отклоняется

- **WHEN** `start_device_scan` вызывается во время состояния `Scanning`
- **THEN** система возвращает `BUSY`, не создаёт второй worker и не открывает
  дополнительный camera handle

#### Scenario: Scan завершается штатно

- **WHEN** все probes завершились, каждый открытый handle освобождён и worker
  остановлен
- **THEN** operation получает terminal status `completed`, полный snapshot
  остаётся доступным для чтения, а service возвращается в `Idle`

#### Scenario: Service-level failure после cleanup

- **WHEN** непредвиденная ошибка прерывает всю operation, но worker завершён и
  все handles подтверждённо освобождены
- **THEN** operation получает terminal status `failed`, service переходит в
  `Faulted`, а `stop_camera` выполняет идемпотентный reset в `Idle`

### Requirement: Cancellation, deadlines и честный Stuck

`cancel_device_scan` и `stop_camera` MUST запрашивать cancellation и MUST NOT
сообщать успешный cleanup, пока worker фактически не завершён и native handle
не освобождён. Cancellation MUST проверяться между native operations; система
MUST NOT небезопасно завершать thread внутри OpenCV call.

Watchdog вне camera worker MUST отслеживать отсутствие progress и общий
operation deadline. Если native call не возвращается после cancellation до
`shutdownDeadlineMs`, service MUST перейти в sticky состояние `Stuck`, сохранить
terminal diagnostic snapshot с кодом `READ_STALLED` и запретить новые scans до
restart process. Система MUST NOT detach-ить worker и выдавать состояние
`Idle` как успешный cleanup.

#### Scenario: Cancellation между probes

- **WHEN** пользователь отменяет scan и текущая native operation возвращается в
  пределах shutdown deadline
- **THEN** новые probes не начинаются, текущий handle освобождается, operation
  получает terminal status `cancelled`, а service возвращается в `Idle`

#### Scenario: Повторная отмена terminal operation

- **WHEN** `cancel_device_scan` повторно вызывается для уже завершённой или
  отменённой последней operation
- **THEN** команда идемпотентно возвращает существующий terminal snapshot и не
  создаёт новую operation

#### Scenario: Native read остаётся заблокированным

- **WHEN** watchdog обнаружил просроченный progress, запросил cancellation, но
  native call не вернулся к `shutdownDeadlineMs`
- **THEN** service становится `Stuck`, UI остаётся отзывчивым и требует restart,
  `start_device_scan` отклоняется с `READ_STALLED`, а operation не представляется
  очищенной или успешно завершённой

#### Scenario: Stop в Idle

- **WHEN** `stop_camera` вызывается в `Idle` без активного worker
- **THEN** команда идемпотентно подтверждает `Idle` и не обращается к camera

### Requirement: Versioned Tauri transport и безопасные ошибки

Система MUST предоставлять versioned команды `start_device_scan`,
`get_device_scan`, `cancel_device_scan` и `stop_camera`. Requests, successful
responses и public errors MUST иметь поле `schema_version` со значением 1,
а frontend MUST отклонять неподдерживаемую schema управляемой transport
ошибкой. Длительные camera operations и ожидание cleanup MUST NOT блокировать
UI thread.

Public camera error MUST содержать стабильный code и безопасное сообщение.
Минимальный набор codes этого change: `INVALID_CONFIG`, `BUSY`,
`STALE_SCAN_OPERATION`, `OPEN_FAILED`, `READ_TIMEOUT`, `READ_STALLED`,
`CANCELLED` и `INTERNAL`. Public DTO и сообщения MUST NOT содержать Rust/OpenCV
debug output, локальные source paths, environment values или internal source
chain. Raw camera frames MUST NOT пересекать Tauri transport в этом change.

#### Scenario: UI опрашивает progress без блокировки

- **WHEN** scan выполняется и frontend вызывает `get_device_scan` для текущего
  `operationId`
- **THEN** команда быстро возвращает согласованный snapshot, а self-check и
  остальной UI остаются доступными

#### Scenario: Неизвестная operation отклоняется безопасно

- **WHEN** клиент запрашивает или отменяет operation, которая не является
  текущей либо последней известной operation process
- **THEN** система возвращает `STALE_SCAN_OPERATION` без раскрытия внутренних
  идентификаторов, paths или native errors

#### Scenario: Native ошибка преобразуется на transport boundary

- **WHEN** OpenCV adapter возвращает ошибку с внутренним diagnostic context
- **THEN** frontend получает только versioned public code/message, а raw context
  не сериализуется в response

### Requirement: Camera scan UI без автоматического доступа к устройству

Diagnostic UI MUST сохранять существующий camera-independent runtime self-check
и MUST добавлять отдельный scan flow с состояниями idle, scanning, completed,
cancelled, failed и stuck. Camera scan MUST начинаться только после явного
действия пользователя; startup приложения и запуск runtime self-check MUST NOT
открывать camera device.

Во время scan UI MUST отображать progress и текущий backend/index, разрешать
cancellation и запрещать повторный start. После завершения UI MUST показывать
доступные endpoints и probe rejections, разрешать выбрать только endpoint
текущего `scanGeneration` и показывать отдельные управляемые состояния для
пустого результата, safe error и `Stuck`/restart required.

#### Scenario: Startup остаётся camera-independent

- **WHEN** приложение запускается либо пользователь выполняет runtime
  self-check, не нажимая scan
- **THEN** camera device не открывается, self-check сохраняет прежнее поведение,
  а camera UI находится в idle state

#### Scenario: Пользователь выбирает найденный endpoint

- **WHEN** scan успешно вернул один или несколько endpoints текущего поколения
- **THEN** UI позволяет выбрать один из них, явно показывает backend и numeric
  index и не называет selection стабильным device ID

#### Scenario: UI получает Stuck

- **WHEN** camera snapshot сообщает состояние `Stuck`
- **THEN** UI прекращает polling, блокирует новые scans, показывает требование
  restart и не маскирует состояние обычной retry-кнопкой

### Requirement: Совместимость, отсутствие persistence и rollback

Change MUST сохранять существующие headless/UI runtime self-check, installer и
native supply contracts. Camera session MUST NOT требовать новых
filesystem/network permissions, ослабления CSP, persistence или миграции
пользовательских данных. Удаление camera commands/UI MUST возвращать приложение
к self-check-only поведению без data migration.

#### Scenario: Машина без камеры сохраняет runtime diagnostics

- **WHEN** приложение работает на машине без camera device
- **THEN** runtime self-check и установленный GUI startup продолжают работать,
  а явный scan завершается пустым результатом без изменения runtime supply

#### Scenario: Security boundary не расширена

- **WHEN** Tauri capability и CSP проверяются после change
- **THEN** они не содержат новых filesystem/network разрешений или внешних
  origins, необходимых только для camera scan
