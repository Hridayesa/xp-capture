# Proposal

## Why

Проверенный Tauri/OpenCV runtime пока не обращается к реальной камере, поэтому
он не подтверждает возможность безопасно найти устройство, получить первый
кадр и освободить native handle. Нужен первый ограниченный camera slice,
который установит lifecycle и transport-контракт до более дорогих profiling,
preview и recording изменений.

Измеримый результат change: установленное или dev-приложение выполняет
ограниченный последовательный scan MSMF/DSHOW endpoints, показывает только
endpoints с реально полученным кадром и после success, cancellation или ошибки
возвращает camera service в честное наблюдаемое состояние без конкурентного
владения `VideoCapture`.

## What Changes

- Добавляется application-level camera service с единственным активным
  владельцем `VideoCapture` и абстракцией capture backend для детерминированных
  tests без hardware.
- Добавляется конфигурируемый bounded scan numeric indices отдельно для MSMF и
  DSHOW. Endpoint считается найденным только после успешного open и первого
  непустого кадра до deadline.
- Добавляются `scanGeneration` и непрозрачный `DeviceEndpointKey`; numeric index
  не трактуется как стабильная identity и не кэшируется между запусками.
- Добавляется lifecycle scan-операции с cancellation, operation/shutdown
  deadlines и явными состояниями `Idle`, `Scanning`, `Faulted`, `Stuck`.
- Добавляются versioned Tauri commands/DTO для запуска и отмены scan, чтения
  progress/result и остановки camera service. Долгие native операции не
  выполняются на UI thread.
- Добавляется минимальный Vue UI для запуска scan, отображения progress/error и
  выбора найденного endpoint; существующий runtime self-check сохраняется.
- Добавляются unit, integration, transport и component tests для no-device,
  busy/open/read failures, cancellation, concurrent start, stalled read и
  cleanup.
- Change не реализует mode profiling, `verifiedModeId`, preview, recording,
  writer, полный hardware matrix или итоговый выбор OpenCV против Media
  Foundation.

### Обязательные ограничения

- Одновременно существует не более одного scan/camera worker, и только он
  открывает и закрывает `VideoCapture`.
- Очереди и диапазон scan ограничены; параллельное открытие нескольких camera
  indices запрещено.
- Worker не объявляется завершённым до фактического release native handle.
- Если native `read()` не возвращается к shutdown deadline, состояние становится
  `Stuck`; новый scan запрещён, а UI требует перезапуска вместо ложного `Idle`.
- Ожидаемые ошибки моделируются типами и преобразуются в безопасные стабильные
  Tauri error codes без native debug output, локальных путей и source chain.

### Предпочтения и предположения

- MSMF проверяется первым, DSHOW используется как сравнительный backend.
- Для этого PoC допустимы display names `Camera {index} / {backend}`.
- Первый range и deadlines берутся из валидируемой конфигурации и могут иметь
  defaults из `opencv-poc-spec.md`; точные значения не являются product SLA.

### Альтернативы

- Параллельный scan отклонён: он нарушает требование одного владельца камеры и
  повышает риск конфликтов драйвера без пользы для PoC.
- Friendly names/stable Windows device IDs отложены: их добавление потребует
  отдельного Media Foundation enumeration/mapping решения и может изменить
  выбранную архитектуру.
- Небезопасное завершение или detach зависшего native thread отклонены. Если
  in-process cleanup не подтверждается, следующий change должен рассмотреть
  helper process либо native Windows capture.

## Capabilities

### New Capabilities

- `opencv-camera-session`: ограниченный scan camera endpoints, единоличное
  владение `VideoCapture`, lifecycle/cancellation/deadline semantics и
  безопасный Tauri/UI контракт выбора endpoint.

### Modified Capabilities

Нет. `opencv-desktop-runtime` остаётся неизменным foundation и продолжает
обеспечивать self-check, packaging и native supply.

## Impact

- Rust: новые domain/application types, capture backend adapter, worker и
  camera transport DTO/commands рядом с существующим `self_check`, без передачи
  Tauri types в domain core.
- Vue/TypeScript: централизованный camera API client и минимальные scan,
  progress, error и endpoint-selection states на diagnostic screen.
- Tests: fake backend, state/lifecycle tests, OpenCV integration smoke с явно
  разрешённым hardware run и component/transport contract tests.
- Tauri: расширяется command surface, но не добавляются filesystem/network
  permissions и не ослабляется CSP.
- Dependencies и публичные данные: новые production dependencies не
  предполагаются; persistence и миграции отсутствуют. Rollback удаляет новый
  camera slice и возвращает self-check-only UI без изменения пользовательских
  данных.
- Связанный план следующих этапов сохранён в `opencv-poc-roadmap.md`.
