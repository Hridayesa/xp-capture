# Дорожная карта OpenCV camera PoC

Дата фиксации: 19.09.2026.

Этот документ сохраняет согласованную декомпозицию оставшейся части
`opencv-poc-spec.md`. Он не заменяет living specs в `openspec/specs/`: каждый
шаг получает собственный OpenSpec change с проверяемыми требованиями до начала
реализации.

## Исходная точка

Change `establish-opencv-tauri-runtime` завершён и архивирован. Он подтвердил
воспроизводимый Windows/Tauri/OpenCV runtime, установленный NSIS bundle,
headless/UI self-check и происхождение native DLL. Реальная камера в текущем
приложении не открывается.

## Последовательность changes

Changes выполняются последовательно, потому что они расширяют общую camera
state machine и связанные Tauri transport-контракты.

### 1. `add-opencv-camera-session`

Статус: специфицирован в OpenSpec change
`openspec/changes/add-opencv-camera-session/`; реализация не начата.

Цель: добавить первый наблюдаемый camera slice — ограниченный scan реальных
устройств и безопасный lifecycle одной camera session.

Scope:

- domain/application границы `CameraService` и fakeable capture backend;
- единственный владелец `VideoCapture`;
- numeric endpoints по паре `backend + index` и `scanGeneration`;
- bounded sequential scan для MSMF/DSHOW с обязательным первым непустым кадром;
- cancellation, deadlines, cleanup и состояния `Idle`, `Scanning`, `Faulted`,
  `Stuck`;
- versioned Tauri commands/DTO и типизированные публичные ошибки;
- минимальный UI scan/selection и проверки без реальной камеры.

Не входит: mode profiling, `verifiedModeId`, preview, recording, writer и
hardware acceptance.

Decision gate: если native `read()` может зависнуть так, что worker нельзя
освободить в пределах shutdown deadline, до следующего шага решается вопрос о
helper process либо отказе от in-process OpenCV capture.

### 2. `add-opencv-mode-profiling`

Цель: эмпирически проверять полные camera tuples и выбирать максимальный
измеренный FPS для endpoint и resolution.

Scope:

- `mode-candidates.json` и строгая валидация;
- детерминированный порядок tuples без предположения о непрерывности FPS;
- reopen, continuous warm-up и capture-only measurement;
- monotonic FPS/interval/gap/read-failure metrics и gates;
- раздельные requested/reported/actual значения;
- `ProfileReport`, `maxVerifiedByResolution`, `verifiedModeId` и revalidation;
- profile/cancel/status/result commands, progress и таблицы результатов в UI.

Decision gate: полный driver-advertised mode list, точный native subtype,
friendly names или stable device IDs переводят дальнейшую работу на Windows
Media Foundation/`MediaCapture`.

### 3. `add-opencv-preview`

Цель: показать облегчённый live preview без передачи full-resolution/full-FPS
потока через Tauri IPC.

Scope:

- owned frame model и корректное копирование continuous/strided `Mat`;
- single-slot latest-wins input и encoded preview;
- sampling, resize, JPEG и monotonically increasing sequence;
- raw binary `[u64 sequence][JPEG]` pull transport без JSON/base64;
- frontend render acknowledgements, FPS/age/replacement metrics;
- Vue preview и гарантированное освобождение object URL/bitmap.

### 4. `add-opencv-recording-pipeline`

Цель: записывать выбранный verified mode одновременно с preview и подтверждать
полный runtime pipeline.

Scope:

- bounded record queue и отдельный владелец `VideoWriter`;
- baseline `CAP_FFMPEG` + MJPG/AVI с проверкой backend name;
- recording/finalization lifecycle и live counters;
- writer/disk/unplug/window-close failures и управляемый cleanup;
- full-pipeline measurement, record-drop gate и OpenCV reread;
- `provisional_verified`, recording commands и UI controls;
- 30-second recording acceptance.

Decision gate: если capture проходит требования, но writer не выдерживает режим
или не даёт нужный формат/timestamps, следующий технический путь — отдельный
FFmpeg/GStreamer/native writer, а не замена capture profiler.

### 5. `evaluate-opencv-camera-poc`

Цель: выполнить аппаратную приёмку установленного приложения и принять
stop/go решение.

Scope:

- `ffprobe` validation harness с явным executable path/version/hash;
- frame count, codec, PTS, duration и wall-clock drift gates;
- `externally_validated` для конкретного recording artifact;
- hardware matrix: встроенная и UVC 60+ камера, MSMF/DSHOW, bright/low light,
  USB hub при применимости;
- capture-only, full-pipeline, 30-second и 3-minute stress runs;
- компактное JSON/CSV evidence из установленного NSIS приложения;
- итоговый выбор: OpenCV, Media Foundation/`MediaCapture` либо другой writer.

## Зафиксированные defaults

- Для первого PoC допустимы numeric endpoints вида `Camera {index} / {backend}`.
- Numeric index не считается стабильной identity и не кэшируется между
  запусками.
- Основной backend — MSMF, сравнительный — DSHOW.
- Начальный `minimumFpsRatio` — `0.95`, но это конфигурируемая PoC policy, не
  production SLA.
- MJPG/AVI — диагностический writer baseline, не утверждённый production
  codec/container.
- Friendly names и Windows device-ID mapping не добавляются неявно; если они
  становятся обязательными, нужен отдельный согласованный change.

## Правило продолжения

Следующий change начинается только после реализации, quality gate и
рассмотрения decision gate предыдущего шага. Новые аппаратные факты сначала
обновляют planning artifacts через OpenSpec, а не молча расширяют реализацию.
