# Spec Delta

## MODIFIED Requirements

### Requirement: Единственная camera operation и наблюдаемая state machine

Camera service MUST допускать не более одной активной camera operation и одного
camera worker. Scan и profiling MUST быть взаимно исключающими. Состояние
service MUST быть доступно как одно из `Idle`, `Scanning`, `Profiling`,
`ProfileReady`, `Faulted` или `Stuck`; snapshot активной operation MUST
содержать её kind, terminal или non-terminal status, количество завершённых и
общее количество work items и текущий probe/candidate, когда он известен.

Scan MUST быть разрешён из `Idle` и `ProfileReady`; новый принятый scan из
`ProfileReady` MUST инвалидировать ранее выданные `verifiedModeId`. Profiling
MUST быть разрешён только для endpoint последнего успешного scan, когда нет
активной operation, и штатное завершение profiling MUST переводить service в
`ProfileReady`. Обычные outcomes отдельных probes/candidates MUST оставаться
частью соответствующего результата и MUST NOT сами по себе переводить service
в `Faulted`.

#### Scenario: Concurrent start отклоняется

- **WHEN** `start_device_scan` или `start_profile` вызывается во время состояния
  `Scanning` или `Profiling`
- **THEN** система возвращает `BUSY`, не создаёт второй worker и не открывает
  дополнительный camera handle

#### Scenario: Scan завершается штатно

- **WHEN** все probes завершились, каждый открытый handle освобождён и worker
  остановлен
- **THEN** scan operation получает terminal status `completed`, полный snapshot
  остаётся доступным для чтения, а service возвращается в `Idle`

#### Scenario: Profile завершается штатно

- **WHEN** все candidates получили outcome, последний handle освобождён и
  profile worker остановлен
- **THEN** profile operation получает terminal status `completed`, report
  остаётся доступным для чтения, а service переходит в `ProfileReady`

#### Scenario: Новый scan заменяет profile-ready контекст

- **WHEN** пользователь запускает новый валидный scan из `ProfileReady`
- **THEN** service переходит в `Scanning`, старые verified modes становятся
  stale, а новый scan не переиспользует прежний endpoint или capture handle

#### Scenario: Service-level failure после cleanup

- **WHEN** непредвиденная ошибка прерывает scan или profile operation, но worker
  завершён и все handles подтверждённо освобождены
- **THEN** operation получает terminal status `failed`, service переходит в
  `Faulted`, а `stop_camera` выполняет идемпотентный reset в `Idle`

### Requirement: Cancellation, deadlines и честный Stuck

`cancel_device_scan`, `cancel_profile` и `stop_camera` MUST запрашивать
cancellation соответствующей активной operation и MUST NOT сообщать успешный
cleanup, пока worker фактически не завершён и native handle не освобождён.
Cancellation MUST проверяться между native operations и profile phases;
система MUST NOT небезопасно завершать thread внутри OpenCV call.

Watchdog вне camera worker MUST отслеживать отсутствие progress и применимый
probe/candidate/operation deadline. Если native call не возвращается после
cancellation или phase timeout до `shutdownDeadlineMs`, service MUST перейти в
sticky состояние `Stuck`, сохранить terminal diagnostic snapshot с кодом
`READ_STALLED` и запретить новые scans/profiles до restart process. Система MUST
NOT detach-ить worker и выдавать состояние `Idle` или `ProfileReady` как
успешный cleanup.

#### Scenario: Cancellation между probes

- **WHEN** пользователь отменяет scan и текущая native operation возвращается в
  пределах shutdown deadline
- **THEN** новые probes не начинаются, текущий handle освобождается, operation
  получает terminal status `cancelled`, а service возвращается в `Idle`

#### Scenario: Cancellation во время profiling

- **WHEN** пользователь отменяет profiling и текущий native call возвращается в
  пределах shutdown deadline
- **THEN** новые reads/candidates не начинаются, handle освобождается, profile
  получает terminal status `cancelled`, partial result не выдаётся как verified
  terminal tuple, а service возвращается в `Idle`

#### Scenario: Повторная отмена terminal operation

- **WHEN** соответствующая cancel command повторно вызывается для уже
  завершённой или отменённой последней operation
- **THEN** команда идемпотентно возвращает существующий terminal snapshot и не
  создаёт новую operation

#### Scenario: Native read остаётся заблокированным

- **WHEN** watchdog scan или profiling обнаружил просроченный progress, запросил
  cancellation, но native call не вернулся к `shutdownDeadlineMs`
- **THEN** service становится `Stuck`, UI остаётся отзывчивым и требует restart,
  новые scan/profile отклоняются с `READ_STALLED`, а operation не представляется
  очищенной или успешно завершённой

#### Scenario: Stop в Idle

- **WHEN** `stop_camera` вызывается в `Idle` либо после безопасного reset из
  `ProfileReady` без активного worker
- **THEN** команда идемпотентно подтверждает отсутствие camera handle и не
  обращается к device
