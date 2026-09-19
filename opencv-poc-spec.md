# Техническая спецификация PoC: OpenCV camera capture в Tauri 2

Дата: 19.09.2026 · Статус: готово к согласованию и реализации · Основание: [архитектурное исследование](report.md).

## 1. Цель

Создать Windows-first Tauri 2 приложение, которое на реальной камере проверяет, можно ли использовать OpenCV как владельца video-only pipeline для:

- выбора камеры;
- эмпирического поиска рабочих комбинаций `backend × FourCC × resolution × FPS`;
- выбора максимального **измеренного** FPS для каждого проверяемого разрешения;
- записи полного потока в файл;
- показа облегчённого preview в Vue без передачи полного видеопотока через Tauri IPC;
- сбора доказательств, достаточных для решения: продолжать с OpenCV либо перейти к Windows `MediaCapture`/Media Foundation.
- воспроизводимой разработки, сборки Tauri bundle, установки и запуска как обычного desktop-приложения.

PoC считается успешным не потому, что `VideoCapture::set(CAP_PROP_FPS, value)` вернул `true`, а только если запрошенный режим прошёл проверку фактического размера кадров, частоты их поступления и полного recording pipeline.

Итоговый deliverable — исходный код приложения, зафиксированные зависимости, автоматизированная подготовка OpenCV, команды test/dev/build, как минимум один Tauri installer artifact и evidence запуска установленного приложения на Windows без заранее установленного OpenCV runtime.

## 2. Решение, которое должен поддержать PoC

По результатам PoC необходимо выбрать одну из ветвей:

1. **Оставить OpenCV:** продукту достаточно конечного набора эмпирически проверенных presets и формулировки «проверено на этой машине».
2. **Перейти на Windows native capture:** продукту нужен полный driver-advertised список `resolution × FPS × subtype`, устойчивый device identity либо строгий выбор конкретного media type.
3. **Перейти на другой writer/pipeline:** OpenCV захватывает нужный FPS, но `VideoWriter` не обеспечивает требуемый codec/container, timestamps, длительность, производительность или восстановление файла.

## 3. Подтверждённые требования

- Основная платформа PoC: Windows.
- Desktop shell: Tauri 2.
- UI: Vue.
- Camera capture и обработка кадров: Rust + OpenCV.
- Записывается только видео; микрофон и звук не входят в scope.
- Пользователь выбирает камеру до профилирования или записи.
- Главный сценарий: для выбранного разрешения определить и применить максимальный устойчиво работающий FPS.
- Preview может иметь меньшие разрешение, FPS и качество, чем записываемый поток.
- Смена камеры или режима во время записи запрещена.
- Все очереди кадров ограничены; накопление кадров без верхней границы запрещено.
- Результат PoC должен собираться и запускаться по README из чистого checkout; исследовательский CLI или library без Tauri UI не считается выполнением.

## 4. Рабочие предположения

До уточнения целевого оборудования PoC использует обратимые defaults:

| Параметр | Default PoC | Статус |
|---|---:|---|
| OpenCV | 4.12.0 | Версия из pinned vcpkg snapshot `2026.07.29`; 4.14.0 актуальнее, но не должна называться фактически собранной без отдельного overlay port/build spike |
| Rust crate `opencv` | 0.100.1 | Поддерживает OpenCV 4.x/5.x; фиксируется lockfile |
| Основной backend | `CAP_MSMF` | Основная Windows-проверка |
| Сравнительный backend | `CAP_DSHOW` | Диагностический, не product default |
| Preview | 640×360, не более 15 FPS, JPEG quality 70 | Настраиваемый PoC default |
| Warm-up режима | 3 секунды | Настраиваемый PoC default |
| Capture-only measurement | 10 секунд | Настраиваемый PoC default |
| Full-pipeline measurement | 30 секунд | Настраиваемый PoC default |
| Stress recording | 3 минуты | Обязательная аппаратная проверка выбранного режима |
| Допуск среднего FPS | не ниже 95% запрошенного | Временный критерий PoC, не утверждённый SLA продукта |
| First-frame deadline | 5 секунд | Watchdog threshold; не обещает принудительно прервать native `read()` |
| Shutdown deadline | 3 секунды | После превышения состояние `Stuck`, не ложный `Idle` |
| Baseline writer | MJPG в AVI | Диагностический формат; не окончательный production format |

Версии Tauri, Vue, Bun и Rust должны быть зафиксированы фактическими manifests/lockfiles созданного PoC и записаны в evidence. Нельзя подставлять версии из исследовательского отчёта вместо реально собранной комбинации.

## 5. Вне объёма PoC

- запись или синхронизация звука;
- полный кроссплатформенный backend для macOS/Linux;
- программное перечисление полного списка аппаратных camera modes средствами OpenCV;
- production hardening, code signing и auto-update; при этом воспроизводимый Tauri installer со всеми runtime DLL обязателен;
- crash recovery незавершённого контейнера;
- гарантированная поддержка произвольной камеры;
- изменение камеры, разрешения, FPS или FourCC во время записи;
- полный набор exposure/focus/white-balance controls;
- передача raw/BGR полного потока в WebView;
- утверждение финального codec/container и требования компактности по результату одного режима.

## 6. Целевая структура

Реализацию разместить отдельно от существующего WebView PoC:

```text
research/2026-09-18-tauri-camera-capture/
  opencv-poc-spec.md
  poc-opencv/
    README.md
    package.json
    bun.lock
    rust-toolchain.toml
    vcpkg.json
    vcpkg-configuration.json
    tools/
      bootstrap-opencv.ps1
      run-with-opencv.ps1
      verify-environment.ps1
      verify-bundle.ps1
      validate-recording.ps1
      runtime-manifest.schema.json
    src/
      App.vue
      camera-api.ts
      preview-controller.ts
      types.ts
    src-tauri/
      Cargo.toml
      Cargo.lock
      tauri.conf.json
      capabilities/
        default.json
      src/
        lib.rs
        camera/
          mod.rs
          config.rs
          service.rs
          worker.rs
          profiler.rs
          metrics.rs
          preview.rs
          recorder.rs
          types.rs
      tests/
        profiler_policy.rs
        metrics.rs
        state_machine.rs
        preview_packet.rs
    config/
      mode-candidates.json
    third-party/
      README.md
      licenses/
    runtime/
      manifest.json
    evidence/
      .gitkeep
```

`evidence/` не должен содержать многомегабайтные записи в git. В репозитории сохраняются JSON/CSV/log summaries и команды воспроизведения; video artifacts остаются локальными с указанием пути, размера и hash.

Native supply contract для первой реализации фиксирован:

- Rust target: `x86_64-pc-windows-msvc`;
- vcpkg triplet: `x64-windows` — dynamic libraries, не `static` и не `static-md`;
- immutable vcpkg tag: `2026.07.29`, peeled commit `9e593bb18ea69cc5095e012465dcd675a822ed0d`;
- port из этого snapshot: `opencv4` 4.12.0, port-version 7;
- `default-features = false`, features: `dshow`, `ffmpeg`, `jpeg`, `msmf`, `thread`; лишние DNN/GUI/image formats не включаются;
- writer backend: OpenCV `CAP_FFMPEG`, writer FourCC `MJPG`, container AVI;
- repository-local roots: `.tools/vcpkg/` и `.vcpkg_installed/x64-windows/`; они не коммитятся.

`vcpkg-configuration.json` фиксирует тот же Git baseline, а `vcpkg.json` обязан содержать именно этот feature set. Если build spike докажет, что feature set недостаточен, изменение оформляется как новая проверенная ревизия спецификации, а не как незафиксированная установка дополнительных пакетов.

`bootstrap-opencv.ps1` проверяет `git rev-parse HEAD == 9e593bb18ea69cc5095e012465dcd675a822ed0d` до запуска bootstrap/install, затем читает port manifest и сохраняет `opencv4 4.12.0#7` в environment evidence. `run-with-opencv.ps1` на каждом вызове вычисляет пути от repository root и задаёт process-local `VCPKG_ROOT`, `VCPKG_INSTALLED_DIR`, `VCPKGRS_DYNAMIC`, include/lib/bin paths; новый shell не зависит от переменных предыдущего. Скрипты не меняют глобальный `PATH` и не используют глобальную OpenCV installation.

После install bootstrap строит `runtime/manifest.json`: точные имена и SHA-256 всех OpenCV/FFmpeg/transitive DLL, источник внутри `.vcpkg_installed`, PE architecture, license/notice path и назначение в bundle. Шаблоны вида `opencv_*.dll` не являются manifest. Build staging копирует только перечисленные файлы рядом с application executable; установленное приложение не зависит от build tree, `VCPKG_ROOT`, `OPENCV_*` или developer `PATH`.

## 7. Контракт сборки и запуска приложения

README обязан разделять prerequisites build-машины и runtime-машины.

### 7.1 Build prerequisites

- Windows 10/11 x64; точный edition/build каждого проверенного host записывается в evidence;
- MSVC C++ Build Tools x64 и Windows SDK; bootstrap проверяет наличие совместимого MSVC toolset, а первый successful build фиксирует полную версию compiler/SDK в `environment.json`;
- Rust toolchain из `rust-toolchain.toml` либо явно закреплённая версия;
- Bun версии из `packageManager`/lockfile policy;
- PowerShell 7, версия проверяется script;
- Git; CMake/Ninja берутся через pinned vcpkg tool acquisition и их фактические версии сохраняются в environment report.

### 7.2 Обязательные команды

После реализации следующие команды должны быть реальными package scripts/wrappers, а не псевдокодом:

```powershell
./tools/bootstrap-opencv.ps1
bun install --frozen-lockfile
bun run test
bun run app:dev
bun run app:build
bun run verify:bundle
```

- `bootstrap-opencv.ps1` идемпотентно подготавливает pinned native dependencies и печатает версии/пути без secrets.
- `bun run test` запускает Rust unit/integration tests и Vue tests либо вызывает единый repository check script, который делает это явно.
- `bun run app:dev` вызывает Tauri через `run-with-opencv.ps1` и запускает интерактивное приложение с камерой.
- `bun run app:build` через тот же wrapper создаёт release executable и NSIS installer.
- `bun run verify:bundle` вызывает `verify-bundle.ps1`: сверяет manifest, устанавливает NSIS artifact в отдельный test location/VM и запускает self-check именно из installed location с очищенными `VCPKG_*`, `OPENCV_*` и `PATH`, содержащим только системные каталоги.

### 7.3 Bundle/runtime policy

`tauri.conf.json` фиксирует:

- bundle target `nsis`, architecture x64;
- `build.windows.staticVCRuntime = true` для application binary;
- `bundle.windows.bundleVCRuntime = true`, потому что dynamic OpenCV/FFmpeg DLL могут требовать VC runtime;
- `bundle.windows.webviewInstallMode.type = "offlineInstaller"`, чтобы установка не зависела от сети или заранее установленного WebView2; ожидаемое увеличение installer примерно на 127 MB принимается для PoC;
- current-user install mode, если hardware lab не требует per-machine install.

Выбор основан на официальной конфигурации Tauri: `bundleVCRuntime` кладёт VC runtime рядом с приложением, `staticVCRuntime` управляет самим MSVC application binary, а `offlineInstaller` не требует интернет-соединения. [Tauri Windows configuration](https://v2.tauri.app/reference/config/#windowsconfig)

`verify-bundle.ps1` обязан:

1. Сверить runtime files с `runtime/manifest.json`, включая SHA-256 и PE `x64`.
2. Проверить отсутствие разрешения imports в DLL из build tree либо случайного `PATH`.
3. Установить созданный NSIS artifact, определить фактический installed path и запустить `<installed-app>.exe --self-check --json <evidence-path>`.
4. Сохранить installer hash/size, installed path, self-check JSON, список реально загруженных process modules с путями и hashes, exit code и uninstall result.
5. Провалиться, если OpenCV/codec DLL загрузились не из installed location или системного Windows directory.

### 7.4 Runtime smoke и self-check без камеры

Executable должен распознавать `--self-check --json <path>` до создания GUI window и завершаться детерминированным exit code. Self-check не требует камеры и возвращает schema-versioned JSON:

- OpenCV version и `getBuildInformation()` hash/summary;
- наличие ожидаемых capture backends `MSMF`, `DSHOW` и writer backend `FFMPEG`;
- фактические OpenCV/codec module paths и SHA-256;
- `Mat 640×480 → resize 320×240 → imencode(.jpg) → imdecode` с проверкой размеров;
- создание 30 синтетических кадров 320×240 @ 30 FPS, открытие `VideoWriter(CAP_FFMPEG, MJPG, AVI)`, `getBackendName() == "FFMPEG"`, finalize;
- повторное открытие полученного AVI, декодирование всех 30 кадров и проверка размера;
- отдельные результаты `opencv_load`, `image_codec`, `writer_open`, `writer_backend`, `writer_roundtrip`.

Exit code `0` разрешён только если все обязательные проверки прошли. `10` означает missing/wrong DLL, `11` — отсутствующий backend, `12` — image codec failure, `13` — writer failure, `14` — reread/count mismatch, `20` — internal/serialization error. В UI тот же self-check доступен на diagnostic screen через общий Rust implementation.

На Windows-машине или VM без установленного OpenCV development/runtime:

1. Установить созданный x64 NSIS artifact.
2. Запустить headless self-check из установленного location в очищенном окружении.
3. Запустить GUI из того же location.
4. Открыть diagnostic screen и повторить environment self-check.
5. Найти хотя бы одну камеру либо получить управляемое состояние `no camera`.
6. При наличии камеры выполнить один короткий profile/preview/record run.
7. Перезапустить приложение и убедиться, что оно не зависит от build-tree DLL или environment variables.

Успешный `cargo test` без installed-app smoke не подтверждает готовность deliverable.

## 8. Архитектура

```text
Vue UI
  │ control commands / small JSON status
  ▼
Tauri CameraService
  │
  ├── capture worker — единственный владелец VideoCapture
  │      ├── monotonic timestamps → Metrics
  │      ├── deep copy → bounded queue<OwnedFrame> → writer worker → VideoWriter → file
  │      └── sampled OwnedFrame → single latest slot → preview worker
  │                                                └── resize + JPEG
  │
  └── raw preview pull ← Vue (<img>/ImageBitmap)
```

### 8.1 Правила владения

- Только capture worker открывает и закрывает `VideoCapture`.
- Только writer worker владеет `VideoWriter`.
- `opencv::core::Mat` не является межпоточным контрактом PoC. Capture worker копирует данные в собственный `OwnedFrame { width, height, pixelFormat, stride, capturedAt, bytes }`; writer/preview workers создают локальное представление поверх принадлежащих им bytes. Нельзя полагаться на поверхностное reference-counted копирование `Mat` или предполагать `Send`/`Sync` без compile-time проверки конкретной версии crate.
- Одновременно существует не более одной активной камеры и одной записи.
- Profiling, preview и recording управляются одной state machine; параллельные `open/start/profile` отклоняются явной ошибкой.

### 8.2 Ограниченные очереди

- Record queue: фиксированная ёмкость, PoC default 4 full-resolution frames.
- Preview input: single-slot `latest wins`.
- Encoded preview: single-slot `latest wins`.
- При заполнении record queue кадр не блокирует бесконечно capture thread: увеличивается `record_dropped`; run помечается failed для режима записи.
- Пропуск preview-кадров ожидаем и не является потерей recording frames.
- Стоимость полного копирования `Mat → OwnedFrame` входит в full-pipeline measurement. Если именно копия не позволяет выдержать FPS, это измеренный недостаток выбранной архитектуры, а не основание заменить её zero-copy предположением без отдельного spike.

## 9. State machine

Допустимые состояния:

```text
Idle
  ├─> ScanningDevices ─> Idle
  ├─> Profiling ─> ProfileReady | Idle | Faulted
  └─> Opening ─> Previewing | Faulted

ProfileReady
  ├─> Opening ─> Revalidating ─> Previewing | Faulted | Stuck
  ├─> Profiling
  ├─> ScanningDevices
  └─> Idle

Previewing
  ├─> Recording ─> Finalizing ─> Previewing | Faulted
  └─> Stopping ─> Idle

Faulted
  └─> Stopping/cleanup ─> Idle

Stuck
  └─> RestartRequired
```

Требования:

- `cancel_profile` и `stop_preview` идемпотентны, но success возвращается только после фактического завершения worker и освобождения handles.
- Закрытие окна сначала отменяет profiling, прекращает capture, финализирует writer и только затем освобождает ресурсы.
- Ошибка writer не оставляет capture и preview в неопределённом состоянии: запись прекращается, файл закрывается/помечается failed, камера может продолжить preview только после явного результата cleanup.
- После unplug/read failure сервис переходит в `Faulted`, освобождает capture и позволяет новый scan/open только если worker действительно завершён.
- Конфигурация задаёт `firstFrameDeadlineMs`, per-operation deadline и `shutdownDeadlineMs`. Deadlines обслуживает watchdog вне capture worker.
- Cancellation проверяется между вызовами OpenCV. Для MSMF/DSHOW нельзя предполагать поддержку `CAP_PROP_READ_TIMEOUT_MSEC`: watchdog обнаруживает отсутствие progress и сообщает `READ_STALLED`, но Rust не пытается небезопасно завершить зависший native thread.
- Если `read()` не возвращается после unplug/driver fault к `shutdownDeadlineMs`, сервис переходит в `Stuck`, а не в `Idle`; новый scan/open запрещён, worker нельзя detach-ить и объявлять очищенным. UI остаётся отзывчивым и требует перезапуска приложения. Наблюдаемый stall является stop-критерием для in-process OpenCV architecture либо основанием вынести capture в отдельный helper process.

## 10. Конфигурация кандидатов

`config/mode-candidates.json` задаёт конечный, обозримый набор тестов:

```json
{
  "deviceScan": { "firstIndex": 0, "lastIndex": 5 },
  "backends": ["MSMF", "DSHOW"],
  "fourcc": ["MJPG", "YUY2"],
  "resolutions": [
    { "width": 640, "height": 480 },
    { "width": 1280, "height": 720 },
    { "width": 1920, "height": 1080 }
  ],
  "fps": [120.0, 60.0, 59.94, 30.0, 29.97],
  "warmupMs": 3000,
  "reopenDelayMs": 500,
  "firstFrameDeadlineMs": 5000,
  "candidateDeadlineMs": 60000,
  "shutdownDeadlineMs": 3000,
  "captureOnlyMs": 10000,
  "fullPipelineMs": 30000,
  "minimumFpsRatio": 0.95,
  "maximumReadFailureRatio": 0.01,
  "maximumGapPeriods": 5.0,
  "maximumLongGapRatio": 0.01,
  "recordQueueCapacity": 4,
  "preview": {
    "width": 640,
    "height": 360,
    "maxFps": 15,
    "jpegQuality": 70,
    "minimumRenderedFps": 8,
    "maximumFrameAgeMs": 500
  }
}
```

Значения являются PoC defaults, а не заявлением, что все камеры поддерживают эти режимы или что thresholds являются production SLA. Конфигурация валидируется до открытия устройства: положительные размеры/FPS, FourCC длиной четыре ASCII-символа, разумные durations/deadlines, bounded capacities и согласованные относительные thresholds.

## 11. Выбор камеры

OpenCV не предоставляет стабильный кроссплатформенный список устройств с friendly names. Поэтому первая версия PoC:

1. Пробует ограниченный диапазон numeric indices из конфигурации отдельно для каждого backend.
2. Считает индекс найденным только после успешного `open` и получения хотя бы одного непустого кадра в пределах timeout.
3. Показывает `Camera {index} / {backend}` и диагностический backend name.
4. Не кэширует numeric index между запусками как устойчивую identity.

Если отсутствие friendly names или стабильной identity мешает проверке на целевом оборудовании, это самостоятельный результат PoC в пользу native Windows device enumeration. Добавлять Media Foundation device IDs в первую реализацию молча нельзя: это изменит проверяемую архитектуру.

Внутренняя identity endpoint:

```text
DeviceEndpointKey = (scanGeneration, backend, numericIndex)
```

Результаты разных backend/index никогда автоматически не объединяются как одна физическая камера. Cross-backend экран показывает их рядом только как независимые endpoints.

## 12. Профилирование режимов

### 12.1 Единица проверки

Проверяется полный tuple:

```text
(device index, backend, FourCC, width, height, requested FPS)
```

Разрешение и FPS нельзя проверять независимо: максимальный FPS может отличаться для MJPG и YUY2 при одинаковых width/height.

FourCC при этом остаётся **запрошенным OpenCV свойством**, а не доказанным native subtype. Результат хранит отдельно:

```text
requestedCaptureFourcc
setCaptureFourccReturned
reportedCaptureFourcc
reportedCaptureFourccMatchesRequest
captureSubtypeConfidence = unconfirmed | reported_match | reported_mismatch
writerFourcc
writerContainer
ffprobeCodec
```

Даже `reported_match` не означает, что Media Foundation/driver гарантированно использует соответствующий native media type: OpenCV может возвращать преобразованный BGR `Mat`. Если точный subtype является обязательным продуктовым требованием, PoC останавливает OpenCV-ветвь в пользу native Media Foundation enumeration/capture.

### 12.2 Порядок

Для каждого tuple:

1. Создать новый `VideoCapture`; состояние предыдущего tuple не переиспользовать.
2. Явно выбрать backend.
3. Запросить capture FourCC, width, height и FPS; сохранить boolean-результаты каждого `set()` только как diagnostics.
4. Прочитать reported width/height/FPS/FourCC.
5. Начать непрерывный `read`; первый непустой кадр должен прийти до `firstFrameDeadlineMs`, иначе watchdog фиксирует timeout/stall policy.
6. Во время всего warm-up непрерывно читать и отбрасывать кадры; `sleep` без drain запрещён.
7. После последнего warm-up frame обнулить counters/timestamps и начать фиксированное capture-only окно. Ожидание первого измеряемого кадра входит в elapsed time окна.
8. Проверять `Mat.cols/rows` каждого измеряемого кадра; отклонить tuple как `coerced_resolution`, если фактический размер не совпал с запросом.
9. Рассчитать capture-only метрики и применить gate. Boolean-результат любого `set()` сам по себе не является terminal pass/fail: `set=false` может сопровождаться уже активным подходящим режимом, а `set=true` — coercion.
10. Если capture-only прошёл, полностью закрыть и заново открыть тот же tuple для full pipeline.
11. Повторить first-frame check и непрерывный warm-up, затем обнулить все full-pipeline counters.
12. Выполнить full-pipeline measurement с writer и preview.
13. Проанализировать файл и counters.
14. Присвоить итоговый статус и освободить все native handles до следующего tuple.

Профилирование выполняется последовательно: камера не открывается конкурентно для нескольких кандидатов. Между release и следующим open выдерживается `reopenDelayMs`; один явно учтённый retry допустим только для transient open/read-start failure и сохраняется в evidence, чтобы нестабильный режим не выглядел успешно открытым с первой попытки.

### 12.3 Метрики capture-only

На каждый успешный `read` фиксируются monotonic timestamp и фактический размер кадра. Рассчитываются:

- `elapsedMs`;
- `capturedFrames`;
- `readFailures`;
- `measuredFps = capturedFrames / elapsedSeconds`;
- median, p95 и p99 inter-frame interval;
- maximum inter-frame gap;
- число gaps больше `maximumGapPeriods × nominal frame period`;
- reported OpenCV properties.

Capture-only tuple проходит default gate, если одновременно:

- каждый принятый кадр имеет точный requested width/height;
- получено не менее двух измеряемых кадров, иначе interval metrics считаются недоступными и tuple проваливается;
- `measuredFps >= requestedFps × minimumFpsRatio`;
- `readFailures / attempts <= maximumReadFailureRatio`;
- maximum gap не превышает `maximumGapPeriods × nominal frame period`;
- доля gaps, превысивших две nominal frame periods, не больше `maximumLongGapRatio`.

Порог 95% конфигурируем и должен отображаться в evidence; он не объявляется production SLA.

### 12.4 Метрики full pipeline

Дополнительно фиксируются:

- `capturedFrames`;
- `recordEnqueuedFrames`;
- `writtenFrames`;
- `recordDroppedFrames`;
- `previewSampledFrames`;
- `previewEncodedFrames`;
- `previewReplacedFrames`;
- `frontendRenderedFrames`, `frontendLastRenderedSequence`, rendered FPS и maximum preview age по периодическим UI acknowledgements;
- peak record queue depth;
- wall-clock duration;
- writer open/finalize duration;
- output file size и SHA-256;
- CPU/RSS, если доступен надёжный measurement mechanism; иначе явно `unavailable`.

Full-pipeline tuple проходит gate, если:

- capture-only FPS gate продолжает выполняться;
- `recordDroppedFrames == 0`;
- `writtenFrames == recordEnqueuedFrames`;
- очередь ни разу не превысила configured capacity;
- writer успешно финализировал непустой файл;
- OpenCV повторно открывает файл, декодирует ожидаемое число кадров и видит ожидаемые width/height;
- preview продолжал обновляться, но его пропуски не влияли на record queue;
- frontend rendered FPS не ниже `preview.minimumRenderedFps`, а maximum age последнего rendered frame не превышает `preview.maximumFrameAgeMs` во время видимого окна; minimized/hidden режим измеряется отдельно и не смешивается с этим gate.

После этого runtime присваивает статус `provisional_verified`: capture и writer round-trip подтверждены самим приложением, но контейнерные timestamps/duration ещё не проверены независимым инструментом. Статус `externally_validated` появляется только в evidence после успешного `ffprobe` gate; приложение не обязано иметь `ffprobe` на машине пользователя.

### 12.5 Выбор максимума

Результаты группируются по `(DeviceEndpointKey, width, height)`. Максимальным считается tuple с наибольшим измеренным FPS, для которого одновременно `captureModeStatus=verified_*` и `runtimeValidationStatus=provisional_verified`; внешний `ffprobe` gate не нужен для интерактивного выбора, но обязателен для итогового hardware acceptance. Результаты MSMF и DSHOW показываются рядом, но не сливаются как одна физическая камера. При равном FPS предпочтение между requested FourCC не задаётся молча: UI показывает оба либо применяет явно сконфигурированную политику.

Каждый прошедший tuple получает непрозрачный `verifiedModeId`, связанный с `profileId`, `DeviceEndpointKey`, hash candidate config и scan generation. Frontend не создаёт `verifiedMode` самостоятельно. Перед preview/recording backend заново открывает tuple, непрерывно прогревает его и повторно подтверждает exact resolution и короткое live FPS окно; coercion или stale ID отменяет разрешение записи.

Результат не кодируется одним взаимоисключающим status. Поле `captureModeStatus` принимает:

- `not_tested`;
- `opening_failed`;
- `first_frame_timeout`;
- `read_stalled`;
- `coerced_resolution`;
- `capture_under_target`;
- `capture_unstable`;
- `verified_fourcc_reported_match`;
- `verified_fourcc_unconfirmed`;
- `cancelled`.

Отдельно используются:

- `runtimeValidationStatus = not_run | failed | provisional_verified`;
- `externalValidationStatus = not_run | failed | externally_validated`.

При writer open/write/finalize failure или record drop поле `captureModeStatus` остаётся `verified_*`, `runtimeValidationStatus` становится `failed`, а `failureReasons` получает соответственно `writer_failed`, `record_dropped_frames` или более точный code. `captureModeStatus=verified_*` не означает, что файл проверен. И наоборот, external validation относится к конкретному recording artifact и не превращает эмпирически найденный OpenCV subtype в нативно перечисленный media type.

## 13. Ограничение `VideoWriter` и проверка файла

Обычный `VideoWriter` получает nominal FPS при открытии и не обязан сохранять реальные arrival timestamps камеры. Поэтому значение `avg_frame_rate` или PTS файла само по себе не доказывает delivered FPS.

Baseline writer открывается с явным `apiPreference = CAP_FFMPEG`, `writerFourcc = MJPG`, `.avi`. Сразу после `open()` приложение записывает `VideoWriter::getBackendName()`; значение, отличное от `FFMPEG`, является `writer_backend_mismatch` и проваливает baseline, даже если файл создался. Это отделяет проверенный writer path от автоматического fallback OpenCV.

Для каждого recording run сопоставляются:

- wall-clock начало/конец;
- capture timestamps;
- `capturedFrames` и `writtenFrames`;
- frame count файла;
- container duration;
- nominal file FPS и PTS.

Writer policy первой версии:

```text
writerNominalFps = requestedFps
expectedFileDuration = writtenFrames / writerNominalFps
durationToleranceMs = max(100 ms, 2 × 1000 / writerNominalFps)
fileWallClockDriftRatio = abs(fileDuration - wallClockDuration) / wallClockDuration
```

Runtime gate приложения, не требующий внешних executable, проверяет:

- writer открылся на `FFMPEG` и корректно завершился;
- файл существует и имеет ненулевой размер;
- повторное чтение через OpenCV успешно;
- decoded frame count равен `writtenFrames`;
- каждый decoded frame имеет ожидаемые width/height.

Это даёт только статус `provisional_verified`. External file gate проверяет отдельно:

- decoded/read frame count файла равен `writtenFrames`;
- width/height и `ffprobeCodec` соответствуют writer configuration;
- PTS монотонны;
- `abs(fileDuration - expectedFileDuration) <= durationToleranceMs`;
- `fileWallClockDriftRatio <= 1 - minimumFpsRatio` с добавлением container rounding tolerance из формулы выше.

Команды анализа должны быть сохранены в README/evidence, минимум:

```powershell
ffprobe -v error -select_streams v:0 -count_frames -show_entries stream=width,height,codec_name,avg_frame_rate,nb_read_frames,duration -of json recording.avi
ffprobe -v error -select_streams v:0 -show_frames -show_entries frame=best_effort_timestamp_time -of csv=p=0 recording.avi
```

`ffprobe` — **не runtime dependency приложения и не входит в installer**. Это независимый инструмент validation harness. `tools/validate-recording.ps1` принимает явный `-FfprobePath`; скрипт сохраняет `ffprobe -version`, SHA-256 executable и полный command line рядом с JSON. Отсутствие `ffprobe` не мешает пользователю запустить приложение и записать файл, но hardware acceptance не может получить `externally_validated` без внешнего анализа. Нельзя подменять этот статус `provisional_verified`.

Если 10 секунд wall-clock дали 450 кадров при requested 60 FPS, режим не проходит, даже если файл объявляет 60 FPS. Если product требует сохранить реальные variable timestamps, это criterion для перехода с `VideoWriter` на FFmpeg/GStreamer/native writer.

Baseline — MJPG/AVI для отделения camera/IPC проверки от H.264 availability. Дополнительный MP4/H.264 writer допускается как отдельный candidate, но его отсутствие не проваливает camera-capture PoC. Компактность измеряется и документируется, но не считается подтверждённой baseline MJPG-записью.

Capture FourCC камеры и writer FourCC/codec являются разными настройками и логируются отдельно. Например, камера может отдать MJPG, OpenCV декодировать его в BGR `Mat`, а `VideoWriter` снова закодировать BGR как MJPG. Нельзя считать это passthrough или смешивать capture subtype с output codec в результатах.

## 14. Preview transport

### 14.1 Rust side

- Preview sampler выбирает не более `preview.maxFps` кадров в секунду.
- Выбранный кадр независимо копируется, уменьшается с сохранением aspect ratio и letterbox/crop policy, зафиксированной в config.
- Результат кодируется в JPEG с configured quality.
- Хранится только последний encoded frame и monotonically increasing `sequence`.

### 14.2 IPC contract

Frontend вызывает `pull_preview(afterSequence)` только после завершения предыдущего decode/render и не чаще configured preview FPS.

Ответ — raw bytes:

```text
[sequence: u64 little-endian][JPEG payload]
```

- Пустой body означает, что нового кадра нет.
- JSON/base64 для JPEG запрещён.
- Ошибки команды возвращаются штатным error response, а не JPEG body.

Frontend:

1. Сравнивает sequence.
2. Создаёт `Blob`/`ImageBitmap` из JPEG payload.
3. Рендерит кадр.
4. Освобождает предыдущий object URL/bitmap.
5. После render планирует следующий pull.

Preview lag не должен создавать очередь старых кадров; он приводит только к пропуску промежуточных sequence.

## 15. Recording lifecycle

1. Запись разрешена только для backend-issued `verifiedModeId` после успешного reopen/revalidation либо явного diagnostic override, который заметно маркируется в UI и evidence.
2. До старта Rust создаёт output directory в app data и уникальный filename с timestamp, backend, resolution и requested FPS.
3. `VideoWriter::open` должен успешно завершиться до перехода в `Recording`.
4. Capture и writer counters обнуляются атомарно на старте run.
5. `stop_recording` прекращает enqueue, дожидается bounded queue, вызывает `release`, проверяет файл и только затем сообщает success.
6. При writer error очередь закрывается, run помечается failed, частичный файл сохраняется для диагностики либо удаляется только по явной cleanup policy.
7. Повторный start и смена настроек во время `Recording` отклоняются.

## 16. Tauri commands и события

Минимальный контракт:

```text
scan_devices(config?) -> DeviceProbe[]
start_profile(device, candidateConfig) -> operationId
cancel_profile(operationId) -> Ack
get_profile_status(operationId) -> ProfileProgress
get_profile_result(operationId) -> ProfileReport
start_preview(verifiedModeId, previewConfig) -> RevalidatedAppliedMode
pull_preview(afterSequence) -> raw bytes
get_live_metrics() -> LiveMetrics
start_recording(writerConfig) -> RecordingInfo
stop_recording() -> RecordingResult
stop_camera() -> Ack
get_environment() -> EnvironmentReport
run_self_check() -> SelfCheckReport
```

Progress можно передавать Tauri event/channel как небольшие JSON-сообщения; image bytes через обычные events не передаются.

Команды проверяют state и возвращают типизированные error codes:

```text
BUSY
NO_DEVICE
STALE_DEVICE_ENDPOINT
STALE_VERIFIED_MODE
OPEN_FAILED
READ_TIMEOUT
READ_STALLED
MODE_COERCED
UNDER_TARGET_FPS
WRITER_OPEN_FAILED
WRITER_BACKEND_MISMATCH
WRITER_WRITE_FAILED
DEVICE_DISCONNECTED
CANCELLED
INVALID_CONFIG
INTERNAL
```

## 17. Формат результатов

`ProfileReport` должен содержать:

```json
{
  "schemaVersion": 1,
  "startedAt": "2026-09-19T12:00:00Z",
  "environment": {
    "os": "Windows ...",
    "appVersion": "...",
    "rustVersion": "...",
    "tauriVersion": "...",
    "opencvVersion": "4.12.0",
    "opencvCrateVersion": "0.100.1"
  },
  "device": {
    "scanGeneration": 1,
    "index": 0,
    "backend": "MSMF",
    "displayName": "Camera 0 / MSMF"
  },
  "policy": {
    "minimumFpsRatio": 0.95,
    "maximumReadFailureRatio": 0.01,
    "maximumGapPeriods": 5.0,
    "maximumLongGapRatio": 0.01
  },
  "results": [],
  "maxVerifiedByResolution": []
}
```

Для каждого tuple сохраняются requested, boolean results `set`, reported properties, фактический frame size, capture-only metrics, full-pipeline metrics, `writerBackendName`, artifact paths/hashes, `captureModeStatus`, `runtimeValidationStatus`, `externalValidationStatus`, nullable external validation details и machine-readable rejection reasons.

## 18. UI

Один диагностический экран должен содержать:

- выбор backend;
- scan диапазона device indices;
- выбор найденной камеры;
- редактор/просмотр candidate resolutions, FPS и FourCC;
- кнопки `Профилировать`, `Отменить`, `Открыть preview`, `Начать запись`, `Остановить`;
- progress `candidate X of N` и текущий tuple;
- таблицу всех результатов;
- сгруппированную таблицу `разрешение → максимальный verified FPS → FourCC/backend`;
- requested/reported/measured значения без смешивания терминов;
- live counters capture/writer/preview/queues;
- облегчённый preview;
- ссылки/пути к report JSON и записи;
- отдельные индикаторы `проверено приложением` и `внешне проверено ffprobe`, без выдачи первого за второе;
- явное предупреждение: список OpenCV является эмпирическим и может быть неполным.

UI не должен:

- называть `CAP_PROP_FPS` фактическим FPS;
- выдавать rejected/coerced tuple за поддерживаемый;
- позволять запись после изменения draft-настроек до повторного применения verified mode;
- скрывать backend/FourCC из diagnostic details;
- автоматически считать самый большой FPS лучшим пользовательским качеством.

## 19. Ошибочные и граничные сценарии

Обязательно проверить:

- ни одного доступного device index;
- камера занята другим приложением;
- `open()` успешен, но первый кадр не приходит;
- `set()` возвращает `false`;
- `set()` возвращает `true`, но resolution coerced;
- `set()` возвращает `false`, но уже активный фактический режим проходит resolution/FPS gates;
- reported FPS совпадает, measured FPS ниже порога;
- low light снижает delivered FPS;
- MJPG работает, YUY2 не выдерживает тот же FPS;
- `VideoWriter` не открывает выбранный FourCC/container;
- writer медленнее capture и record queue заполняется;
- WebView перестаёт запрашивать preview;
- cancel во время warm-up и measurement;
- backend не возвращается из `read()` после watchdog/deadline;
- unplug во время preview и recording;
- повторный start/double click;
- закрытие окна во время profiling, recording и finalization;
- недостаточно места на диске;
- выходной файл существует, но `ffprobe` не может его прочитать.

## 20. Автоматические проверки без камеры

Rust unit/integration tests должны покрыть:

1. Валидацию `mode-candidates.json`.
2. Детерминированное построение и порядок candidate tuples.
3. Отсутствие binary search и неверного предположения о непрерывности FPS.
4. Непрерывный drain warm-up и сброс counters; fake backend с накопленным burst не должен завышать измеренный FPS.
5. Расчёт throughput FPS и interval percentiles на синтетических timestamps, включая `<2` frames и consecutive failures.
6. Gates exact resolution, FPS ratio, read failures и относительных gap thresholds.
7. `set=false` при фактически прошедшем режиме не даёт автоматический fail; `set=true` с coercion проваливается.
8. Раздельные requested/reported capture FourCC и writer codec/container.
9. Агрегацию максимального verified tuple только внутри `DeviceEndpointKey + resolution`.
10. `verifiedModeId`: stale scan generation/config hash отклоняются; reopen coercion отзывает разрешение записи.
11. State machine, reprofile/rescan из `ProfileReady`, `Stuck` и отклонение конкурентных операций.
12. Bounded record queue и счётчик drops.
13. Single-slot preview replacement и frontend render acknowledgements.
14. Копирование continuous и strided `Mat`-layout в `OwnedFrame` без чтения padding как pixels.
15. Кодирование/декодирование raw preview packet `[sequence][JPEG]`.
16. Cleanup после writer/open/read errors и watchdog state при stalled read.
17. Формулы file duration/drift и сериализацию `ProfileReport` schema version 1.
18. `--self-check`: успешный image encode/decode и synthetic MJPG/AVI round-trip, а также отдельные exit codes для missing backend, writer failure и frame-count mismatch.
19. Writer baseline отклоняется, если `getBackendName()` не равен `FFMPEG`.
20. Runtime manifest schema, SHA-256/PE architecture checks и запрет DLL path вне installed/system directories.

Vue tests должны покрыть:

- смену draft camera/mode блокирует запись до нового успешного apply;
- rejected tuple нельзя выбрать как verified;
- cancel и ошибки возвращают UI в допустимое состояние;
- object URLs/bitmaps preview освобождаются;
- UI различает requested, reported и measured FPS.

Fake camera/backend в tests должен выдавать заранее заданные кадры и timestamps; unit tests не обращаются к реальному hardware.

## 21. Аппаратная матрица

Минимум:

- одна встроенная камера;
- одна UVC USB camera, заявляющая 60 FPS или выше;
- основной прогон `CAP_MSMF`;
- сравнительный прогон `CAP_DSHOW` хотя бы на USB camera;
- bright light и low light;
- камера напрямую и, если применимо, через shared USB hub;
- capture-only, full pipeline, 30-second recording и 3-minute stress для выбранного максимума;
- установленный NSIS artifact, собранный из проверяемого commit; dev server и запуск release exe из build tree не засчитываются.

Для каждого прогона сохранить:

- config;
- environment report;
- installer SHA-256/size, installed path и exact app executable hash;
- runtime manifest и список реально загруженных DLL с путями/hashes;
- self-check JSON/exit code из installed executable;
- profile JSON;
- metrics CSV;
- `ffprobe` JSON;
- `ffprobe -version`, executable SHA-256 и command line validation harness;
- SHA-256 и размер видео;
- краткое наблюдение о preview latency/quality;
- результат pass/fail и причины.

## 22. Acceptance criteria

PoC принимается, если одновременно выполнено следующее:

- [ ] Из чистого checkout документированные `bootstrap → install → test → app:build → verify:bundle` команды завершаются успешно в новом shell без заранее заданных OpenCV/vcpkg environment variables.
- [ ] Native stack совпадает с contract: target `x86_64-pc-windows-msvc`, triplet `x64-windows`, vcpkg peeled commit `9e593bb18ea69cc5095e012465dcd675a822ed0d`, `opencv4` 4.12.0#7 и заданный feature set.
- [ ] `app:build` создаёт release executable и устанавливаемый x64 NSIS artifact с offline WebView2 installer и VC runtime policy.
- [ ] Installer запускается на Windows runtime-машине без установленного OpenCV; приложение не зависит от build-tree DLL, `VCPKG_ROOT`, `OPENCV_*` или developer `PATH`.
- [ ] Installed executable проходит `--self-check` в очищенном окружении: OpenCV/version/backends, JPEG round-trip и synthetic `CAP_FFMPEG` MJPG/AVI write/read 30-frame round-trip.
- [ ] Runtime manifest содержит точные DLL names/hashes/PE architecture/licenses; фактически загруженные OpenCV/codec DLL находятся в installed location, а не в build tree или случайном `PATH`.
- [ ] Установленное приложение позволяет пройти пользовательский путь `scan → profile → выбрать максимум → preview → record → stop → открыть/проверить файл`.
- [ ] CameraService имеет одного владельца `VideoCapture` и не открывает одну камеру конкурентно.
- [ ] Device scan не выдаёт индекс без успешно прочитанного кадра.
- [ ] Profiler проверяет полный requested tuple с backend/FourCC, но UI/evidence не называют capture subtype нативно подтверждённым.
- [ ] Каждый tuple открывается заново; результат предыдущего режима не протекает в следующий.
- [ ] Warm-up непрерывно дренирует кадры до сброса counters; burst fake test и аппаратный лог не завышают throughput.
- [ ] Ни один режим не получает `verified_*`, если фактический `Mat` имеет другое разрешение.
- [ ] Boolean результаты `set()` остаются diagnostics: `false` не даёт автоматический fail, `true` не даёт автоматический pass.
- [ ] Delivered FPS вычисляется из monotonic timestamps, а не из `CAP_PROP_FPS` или metadata файла.
- [ ] Для выбранного endpoint+resolution UI показывает максимальный измеренный tuple, requested/reported FourCC и backend; результаты разных endpoints не смешиваются.
- [ ] Backend-issued `verifiedModeId` связан с profile/config/scan generation; stale ID и reopen coercion блокируют запись.
- [ ] Capture-only и full-pipeline результаты сохранены раздельно.
- [ ] Record и preview очереди ограничены; record drops видимы и проваливают run.
- [ ] Preview использует raw binary IPC, single latest frame и не передаёт full-resolution/full-FPS поток.
- [ ] Preview gate измеряет backend encoded и frontend rendered sequence/FPS/age, а не субъективное «видно движение».
- [ ] 30-second запись выбранного режима не снижает FPS ниже configured gate и не теряет writer frames.
- [ ] 3-minute stress не показывает неограниченного роста queue/memory и завершается читаемым файлом.
- [ ] Runtime gate и внешний gate не смешиваются: OpenCV reread даёт только `provisional_verified`; `externally_validated` требует, чтобы внешний `ffprobe` frame count совпал с `writtenFrames`, а duration/PTS прошли заданные формулы и допуски.
- [ ] File metadata не используется как единственное доказательство delivered FPS.
- [ ] Cancel, busy camera, unsupported/coerced mode, writer failure, unplug и window close завершаются управляемым cleanup либо явным `Stuck`/`RestartRequired` с failed run; зависший native worker никогда не маскируется как успешно очищенный `Idle`.
- [ ] Если native `read()` зависает и cleanup невозможен, watchdog сохраняет evidence, UI не блокируется, run проваливается и не объявляется восстановленным; этот результат применяется к stop/go решению.
- [ ] Evidence содержит точные версии, config, JSON/CSV metrics, команды анализа и честно отделяет проверенное от непроверенного.

## 23. Stop/go критерии

### Продолжать с OpenCV

- Все необходимые продуктовые разрешения находятся в candidate set и получают хотя бы один `verified_*` tuple.
- Максимальный нужный FPS выдерживается в full pipeline.
- MJPG/AVI либо другой доступный writer удовлетворяет требованиям PoC или существует отдельно доказанный путь к production codec.
- Numeric device selection/friendly-name limitation приемлема либо решается без смены владельца capture pipeline.
- Packaging OpenCV/DLL приемлем по размеру и сопровождению.

### Перейти на Media Foundation / `MediaCapture`

- Нужен исчерпывающий driver-advertised mode list.
- Нужны стабильные device IDs/friendly names и точное сопоставление.
- OpenCV систематически coerce-ит режимы либо backends расходятся непредсказуемо.
- Требуются расширенные camera controls с надёжным capability discovery.

### Перейти на GStreamer/FFmpeg/native writer

- Камера выдаёт нужный FPS, но `VideoWriter` не успевает или не даёт нужный codec/container.
- Нужны реальные per-frame timestamps/VFR, hardware encoding, segmenting либо crash recovery.
- Требуется единый управляемый pipeline на нескольких ОС.

## 24. Последовательность реализации

1. Зафиксировать manifests/lockfiles, vcpkg commit/triplet/features и wrapper, воспроизвести native build в новом shell.
2. Сгенерировать runtime manifest/staging и реализовать `--self-check`; до работы с камерой доказать установленный synthetic writer round-trip.
3. Реализовать domain types, config validation, metrics и state machine с fake backend tests.
4. Реализовать Windows `VideoCapture` open/read/release для одного заданного index/backend.
5. Добавить bounded device scan.
6. Реализовать capture-only profiler и JSON evidence.
7. Добавить aggregation `maxVerifiedByResolution`.
8. Реализовать preview single-slot, JPEG и raw pull IPC.
9. Добавить bounded writer worker и baseline `CAP_FFMPEG` MJPG/AVI с проверкой backend name.
10. Реализовать full-pipeline profiler, counters, OpenCV reread и cleanup.
11. Добавить Vue diagnostic UI и component tests.
12. Собрать/установить NSIS artifact и выполнить clean-environment bundle verification.
13. Выполнить hardware matrix из installed app; отдельно прогнать external ffprobe validation и сохранить evidence.
14. Сопоставить результаты со stop/go criteria и оформить решение в основном отчёте.

## 25. Открытые вопросы

Они не блокируют начало scaffolding, но должны быть закрыты до финальной аппаратной матрицы:

1. Модели целевых камер и их заявленные режимы.
2. Продуктовый список разрешений и минимально значимых FPS.
3. Допустимый порог отклонения FPS и jitter; default 95% является лишь политикой PoC.
4. Требуется ли пользователю видеть FourCC либо достаточно показывать его в diagnostics.
5. Приемлем ли MJPG/AVI как диагностический output и какой codec/container нужен продукту.
6. Минимальная версия Windows и ограничения размера installer.
7. Нужны ли friendly camera names уже в OpenCV PoC; если да, потребуется отдельное исследование device enumeration/mapping.

## 26. Definition of Done

PoC завершён, когда исходники из чистого checkout в новом shell проходят документированные проверки и создают x64 NSIS Tauri installer; установленное на чистой целевой Windows machine приложение без developer OpenCV/vcpkg проходит headless self-check, воспроизводимо профилирует заданные camera tuples, выбирает максимальный измеренный FPS для каждого endpoint+resolution, повторно валидирует режим, записывает video-only файл, показывает облегчённый preview без потери recording frames, различает runtime и external file validation, сохраняет проверяемые evidence и даёт однозначный stop/go вывод для OpenCV.
