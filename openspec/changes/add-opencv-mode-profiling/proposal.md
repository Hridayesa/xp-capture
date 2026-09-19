# Proposal

## Why

Приложение уже умеет находить реальный OpenCV camera endpoint и безопасно
владеть одной camera session, но пока не может доказать, какие полные tuples
`backend × FourCC × resolution × FPS` действительно работают с требуемой
частотой. Следующий шаг нужен, чтобы получить воспроизводимый capture-only
`ProfileReport`, выбрать максимальный измеренный FPS для каждого разрешения и
выдать backend-owned `verifiedModeId`, не смешивая это доказательство с ещё не
реализованными preview и recording gates.

Измеримый результат: для выбранного endpoint текущего `scanGeneration`
установленное приложение последовательно проверяет конечный набор кандидатов,
показывает progress и requested/reported/measured значения, сохраняет все
machine-readable outcomes и однозначно вычисляет
`maxVerifiedByResolution` только из tuples, прошедших capture-only gates.

## What Changes

- Добавить строгую schema/version validation для repository-owned
  `config/mode-candidates.json`: конечные FourCC, resolutions и FPS, а также
  warm-up, measurement, reopen, first-frame, operation/shutdown deadlines и
  относительные thresholds. Некорректная конфигурация отклоняется до открытия
  camera device.
- Добавить последовательный profiler полного tuple
  `(DeviceEndpointKey, backend, FourCC, width, height, requested FPS)`: каждый
  кандидат открывается заново, свойства запрашиваются в детерминированном
  порядке, warm-up непрерывно дренирует frames, а capture-only окно измеряется
  monotonic clock после сброса counters.
- Разделить requested, результаты `set()`, reported и actual frame values;
  вычислять throughput FPS, interval percentiles, gaps и read-failure ratio.
  Boolean `set()` остаётся diagnostics и сам по себе не определяет результат.
- Добавить machine-readable статусы/rejection reasons, `ProfileReport`,
  группировку `maxVerifiedByResolution` и opaque `verifiedModeId`, связанный с
  `profileId`, endpoint, `scanGeneration` и hash фактически применённой
  candidate config.
- Расширить единую camera state machine состояниями `Profiling` и
  `ProfileReady`, сохранив одного владельца `VideoCapture`, взаимное исключение
  scan/profile, cancellation, watchdog и sticky `Stuck` semantics.
- Добавить versioned Tauri commands `start_profile`, `get_profile_status`,
  `get_profile_result` и `cancel_profile`; расширить стабильные public error
  codes для stale endpoint/profile и mode validation без раскрытия native
  diagnostics.
- Расширить Vue diagnostic screen явным запуском/отменой profiling, progress
  `candidate X of N`, таблицей всех результатов и отдельной таблицей максимумов
  по resolution. Startup, runtime self-check и обычный scan не запускают
  profiling автоматически.
- Добавить deterministic hardware-free tests и opt-in Windows hardware smoke с
  compact JSON evidence для capture-only profiling.

Обязательные ограничения: numeric endpoint остаётся эфемерным; tuples разных
backends не объединяются; одновременно открыт не более чем один capture handle;
raw frames не пересекают Tauri transport; cancellation не завершает native
thread небезопасно; rejected/coerced/under-target tuple не получает
`verifiedModeId`.

Предпочтения/defaults: начальный `minimumFpsRatio` равен `0.95`, основной
backend — MSMF, сравнительный — DSHOW, а candidate values берутся из
версионированного repository config. Это PoC policy, а не production SLA или
утверждение driver-advertised mode list.

Предположение: decision gate предыдущего change остаётся положительным —
real-camera smoke не выявил невосстанавливаемый native stall. Новый stall или
потребность в точном native subtype/friendly device identity останавливает
in-process OpenCV ветвь для отдельного архитектурного решения.

Не входят: preview transport/JPEG, `VideoWriter`, record queue, full-pipeline и
30-second measurements, OpenCV reread записанного файла, `ffprobe`,
`runtimeValidationStatus=provisional_verified`, external validation, friendly
names, stable Windows device IDs и driver-advertised enumeration.

## Capabilities

### New Capabilities

- `opencv-mode-profiling`: validation candidate policy, последовательное
  capture-only измерение modes, metrics/gates, profile transport/report,
  `verifiedModeId`, aggregation и profiling UI.

### Modified Capabilities

- `opencv-camera-session`: расширить общую camera state machine и exclusivity с
  scan-only на взаимно исключающие scan/profile operations, сохранив прежние
  cancellation, cleanup, `Stuck`, security и ephemeral endpoint guarantees.

## Impact

- Rust camera domain/application: новые policy, tuple, metrics, report и
  verified-mode types; существующие service/worker/ports расширяются без
  передачи OpenCV или Tauri types в domain core.
- OpenCV adapter: setting/reading capture properties и возврат frame metadata;
  один scoped `VideoCapture` по-прежнему принадлежит camera worker.
- Tauri/TypeScript: четыре новых versioned commands/DTO, строгие runtime
  decoders и polling без raw frame payload.
- Vue: profile controls/progress/result tables рядом с текущим scan flow, без
  shadcn-vue и без новых permissions/CSP origins.
- Files/evidence: новый `config/mode-candidates.json` и compact profile JSON;
  persistence и migration пользовательских данных отсутствуют.
- Dependencies: новые production dependencies и обновление lockfiles не
  предполагаются; фактические версии Rust 1.98.1, OpenCV crate 0.100.1, Tauri
  2.11.5, Vue 3.5.43 и текущий OpenCV runtime сохраняются.
- Дорогая альтернатива — сразу перейти к Media Foundation/`MediaCapture` для
  точного native mode enumeration. Она отложена, потому что текущая цель —
  проверить достаточность конечного эмпирического OpenCV списка; переход станет
  обязательным, если точный subtype, stable identity или полный mode list
  окажутся требованием либо profiler обнаружит неуправляемый stall.
