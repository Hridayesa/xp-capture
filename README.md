# XP Capture runtime foundation

Приложение предоставляет camera-independent Rust self-check через headless CLI и diagnostic GUI, а также отдельный явный bounded scan OpenCV camera endpoints. Startup и self-check не открывают камеру: camera access начинается только после действия «Начать scan» либо запуска opt-in hardware smoke.

## Зафиксированный контракт

- Windows 10/11 x64, Rust target `x86_64-pc-windows-msvc`.
- Rust `1.98.1`, Bun `1.4.2`.
- `tauri = 2.11.5`, `@tauri-apps/cli = 2.11.4`, `@tauri-apps/api = 2.11.1`.
- vcpkg commit `9e593bb18ea69cc5095e012465dcd675a822ed0d`, triplet `x64-windows`.
- Native `opencv4` `4.12.0#7` с features `dshow`, `ffmpeg`, `jpeg`, `msmf`, `thread` и без default features.
- Rust bindings `opencv = 0.100.1`.

Глобальный OpenCV 4.13.0 и унаследованные `OPENCV_*`/`VCPKG_*` не используются: `tools/run-with-opencv.ps1` очищает их для дочернего процесса и задаёт только pinned repository-local paths.

## Build prerequisites

- Windows 10/11 x64 и PowerShell 7.
- Git.
- Visual Studio 2022 или Build Tools с workload C++ x64 и Windows SDK. Из фактического `VC\Redist` берутся только требуемые app-local VC runtime DLL и `Redist.txt`.
- Rustup/Rust toolchain из `rust-toolchain.toml`.
- Bun версии из `packageManager`.
- CMake и Ninja, доступные в новом shell.
- Сеть для первого получения pinned vcpkg checkout/packages и offline WebView2 installer; повторные сборки используют локальные caches.

Runtime host не требует Visual Studio, Rust, Bun, vcpkg, системный OpenCV, сеть или камеру. Installer включает offline WebView2 и manifest-listed native runtime.

## Clean-checkout flow

Каждую команду можно запускать в отдельном новом PowerShell 7 process из корня репозитория:

```powershell
./tools/bootstrap-opencv.ps1
bun install --frozen-lockfile
bun run test
bun run app:build
bun run verify:bundle
```

Для разработки:

```powershell
bun run app:dev
```

`app:dev` и `app:build` сначала валидируют `runtime/manifest.json`, пересоздают allowlisted `runtime/staging`, затем запускают Tauri через process-local OpenCV wrapper. `app:build` создаёт ровно один x64 NSIS artifact в `target/release/bundle/nsis/`; pre-bundle hook проверяет release PE imports и evidence `STATIC_VCRUNTIME=true`.

## Headless self-check и exit codes

```powershell
./xp-capture.exe --self-check --json ./self-check.json
```

| Code | Значение |
| ---: | --- |
| `0` | Все checks и module provenance прошли |
| `10` | OpenCV load/runtime/module provenance failure |
| `11` | Отсутствует обязательный backend `MSMF`, `DSHOW` или `FFMPEG` |
| `12` | JPEG encode/decode round-trip failure |
| `13` | Writer open/backend failure |
| `14` | AVI reread, frame count или dimensions mismatch |
| `20` | Внутренняя ошибка или безопасная запись report невозможна |

Проверки: `opencv_load`, `image_codec`, `writer_open`, `writer_backend`, `writer_roundtrip`. VideoWriter использует synthetic 30-frame `320x240` MJPG/AVI через `CAP_FFMPEG`; камера не открывается.

## Camera session scan

Diagnostic UI запускает только один последовательный scan. Defaults transport v1:

| Поле | Default | Hard limit / правило |
| --- | ---: | --- |
| `first_index` / `last_index` | `0..=5` | не более 32 indices и 64 total probes |
| `backends` | `[MSMF, DSHOW]` | непустой ordered набор без duplicates; `CAP_ANY` запрещён |
| `first_frame_deadline_ms` | `5000` | `1..=600000` |
| `operation_deadline_ms` | `90000` | `1..=600000`, больше first-frame deadline |
| `shutdown_deadline_ms` | `3000` | `1..=600000`, не больше operation deadline |
| `reopen_delay_ms` | `500` | `1..=600000` после подтверждённого release |

Каждый tuple `(backend, numericIndex)` открывается отдельно через явный `CAP_MSMF` или `CAP_DSHOW`. Endpoint публикуется только после первого непустого frame и checked `release()`. Один numeric index через два backend — два независимых endpoint; `DeviceEndpointKey`, display name и numeric index эфемерны и не должны сохраняться или трактоваться как стабильный Windows device ID.

Tauri transport v1 предоставляет команды `start_device_scan`, `get_device_scan`, `cancel_device_scan` и `stop_camera`. Стабильные public error codes: `INVALID_CONFIG`, `BUSY`, `STALE_SCAN_OPERATION`, `OPEN_FAILED`, `READ_TIMEOUT`, `READ_STALLED`, `CANCELLED`, `INTERNAL`. DTO не содержат frame bytes, OpenCV diagnostics, paths или internal source chain.

## Camera mode profiling

Profiling запускается только явной кнопкой после выбора endpoint последнего scan. Startup, self-check, scan и выбор radio сами не применяют mode properties. `config/mode-candidates.json` — общий schema v1 source для Rust и Vue:

| Поле | Default | Hard limit / правило |
| --- | ---: | --- |
| `fourcc` | `MJPG`, `YUY2` | 1–8 уникальных значений ровно из четырёх ASCII characters |
| `resolutions` | `640×480`, `1280×720`, `1920×1080` | 1–16 уникальных; каждая dimension `1..=8192` |
| `fps` | `120`, `60`, `59.94`, `30`, `29.97` | 1–16 уникальных finite значений `0 < FPS <= 1000` |
| total candidates | 30 | полный ordered product, максимум 256 |
| `warmup_ms` / `capture_only_ms` | `3000` / `10000` | каждое `1..=600000`; warm-up непрерывно читает frames |
| `first_frame_deadline_ms` | `5000` | `1..=600000` |
| `candidate_deadline_ms` | `60000` | покрывает first-frame + warm-up + capture-only |
| `operation_deadline_ms` | `600000` | global budget, не короче candidate deadline |
| `shutdown_deadline_ms` / `reopen_delay_ms` | `3000` / `500` | shutdown не длиннее candidate deadline |
| `minimum_fps_ratio` | `0.95` | `(0, 1]`; PoC threshold, не production SLA |
| read/long-gap ratios | `0.01` / `0.01` | `[0, 1]` |
| `maximum_gap_periods` | `5.0` | finite, `>= 1` |

Requested — tuple из policy; `set` — только boolean diagnostics OpenCV; reported — значения `CAP_PROP_*`; actual — dimensions каждого принятого `Mat`; measured — monotonic capture-only metrics. `set=true` не доказывает режим, `set=false` не проваливает его автоматически. Reported FourCC match не называется native media subtype. Numeric index и `DeviceEndpointKey` эфемерны.

`max_verified_by_resolution` строится только из capture-verified tuples и сохраняет все FourCC ties. `verifiedModeId` process-local и инвалидируется новым accepted scan/profile либо `stop_camera`. Это ещё не preview/recording proof: full-pipeline, writer, runtime и external validation остаются `not_run`.

Profile transport v1: `start_profile`, `get_profile_status`, `get_profile_result`, `cancel_profile`. Дополнительные safe codes: `STALE_DEVICE_ENDPOINT`, `STALE_PROFILE_OPERATION`, `PROFILE_NOT_READY`, `STALE_VERIFIED_MODE`, `MODE_COERCED`, `UNDER_TARGET_FPS`. Cancellation ждёт checked release; `Stuck` требует restart и не выдаётся за cleanup.

### Opt-in hardware smoke

Hardware smoke не входит в `bun run test`. Сначала явно проверьте, есть ли на host доступная камера, затем укажите соответствующий context и bounded scope:

```powershell
Get-PnpDevice -Class Camera -PresentOnly

./tools/run-with-opencv.ps1 -- cargo run --locked --bin camera-session-smoke -- `
  --first-index 0 --last-index 2 `
  --backends MSMF,DSHOW `
  --evidence evidence/camera-session-smoke.json `
  --hardware-context camera-present `
  --first-frame-deadline-ms 3000 `
  --operation-deadline-ms 30000 `
  --shutdown-deadline-ms 3000 `
  --reopen-delay-ms 250
```

Для host без present Camera PnP device используйте `--hardware-context camera-less`. Harness выполняет два scan подряд и сохраняет compact schema-versioned evidence с policy, ordered outcomes, generations, release/reopen result и version references. Он не сохраняет frames, secrets или полный environment dump. При `camera-present` отсутствие положительного endpoint в любом из двух run является failing acceptance gate.

### Opt-in mode profile smoke

Команда не входит в `bun run test`. Сначала выполните DSHOW на подтверждённом index, затем отдельной попыткой MSMF. Каждый run использует тот же `CameraService`, полный config, terminal cleanup и повторный bounded scan:

```powershell
./tools/run-with-opencv.ps1 -- cargo run --locked --bin camera-mode-profile-smoke -- `
  --backend DSHOW --index 0 `
  --config config/mode-candidates.json `
  --evidence evidence/camera-mode-profile-smoke.json

./tools/run-with-opencv.ps1 -- cargo run --locked --bin camera-mode-profile-smoke -- `
  --backend MSMF --index 0 `
  --config config/mode-candidates.json `
  --evidence evidence/camera-mode-profile-smoke-msmf.json
```

Exit code non-zero означает failing acceptance gate: endpoint не найден, profile не завершился, нет candidate outcomes либо endpoint нельзя открыть повторно после release. Compact JSON содержит policy/hash, ordered outcomes/metrics, release/reopen и version references; frame bytes, secrets и full environment не записываются.

## Локальное хранение и evidence

- `.tools/vcpkg/` — exact vcpkg checkout; `.tools/msvc-redist/` — выбранные из установленного MSVC Redistributable DLL и notice.
- `.vcpkg_installed/` — pinned dynamic OpenCV install tree.
- `runtime/manifest.json` — tracked versioned allowlist; `runtime/staging/` — generated payload.
- `target/`, `dist/` — build outputs; NSIS installer остаётся локальным.
- `evidence/environment-summary.json` — согласованные версии host/toolchain/native supply.
- `evidence/tauri-build-environment.json` — target и effective `STATIC_VCRUNTIME=true`.
- `evidence/runtime-imports.json` — release PE import closure.
- `evidence/bundle-verification.json` — installer/executable hashes и этапы `manifest`, `install`, `headless_self_check`, `module_provenance`, `gui_smoke`, `uninstall`.
- `evidence/camera-session-smoke.json` — opt-in bounded camera/no-camera outcomes и release/reopen evidence.
- `evidence/camera-mode-profile-smoke.json` — opt-in DSHOW profile evidence; MSMF attempt хранится отдельно до сведения decision gate.
- `evidence/artifacts/` — ignored подробные self-check reports; большие installer/AVI не коммитятся.

Обновить и проверить environment evidence:

```powershell
bun run collect:evidence
bun run validate:environment-evidence
```

## Troubleshooting

- `Pinned OpenCV environment is incomplete`: выполните `./tools/bootstrap-opencv.ps1`; не подставляйте глобальный OpenCV.
- `Vcpkg revision mismatch`: существующий `.tools/vcpkg` не соответствует pinned commit. Сохраните нужные локальные данные и удалите только этот точный generated checkout перед повторным bootstrap.
- `Runtime source hash mismatch`: native tree изменился после генерации manifest; повторите pinned bootstrap, не копируйте DLL из `System32` или произвольного `PATH`.
- `Required app-local MSVC runtime import ... is missing`: проверьте Visual Studio C++ workload и фактический `VC\Redist\MSVC`.
- `Expected exactly one NSIS artifact`: удалите только старые generated installer artifacts из `target/release/bundle/nsis/` и повторите `bun run app:build`.
- Failed `module_provenance`: installed app загрузил DLL вне install directory либо hash не совпал; build-tree и global paths не являются допустимым workaround.
- `gui_smoke` или `uninstall` failure остаётся failing gate; partial evidence сохранено в `evidence/bundle-verification.json`.
- `READ_STALLED` / состояние `Stuck`: native OpenCV call не вернулся к shutdown deadline. Приложение намеренно не объявляет cleanup успешным, запрещает новый scan и требует restart process; retry в том же process небезопасен.
- `camera-present context requires positive open/read/reopen evidence`: освободите камеру в других приложениях, проверьте Windows privacy settings и повторите bounded smoke. Не заменяйте этот gate scripted fake-тестом.

## Cleanup generated data

После завершения процессов можно удалить только конкретные generated roots:

```powershell
Remove-Item -LiteralPath ./dist -Recurse -Force
Remove-Item -LiteralPath ./target -Recurse -Force
Remove-Item -LiteralPath ./runtime/staging -Recurse -Force
Remove-Item -LiteralPath ./runtime/verification -Recurse -Force
Remove-Item -LiteralPath ./.vcpkg_installed -Recurse -Force
Remove-Item -LiteralPath ./.tools/vcpkg -Recurse -Force
Remove-Item -LiteralPath ./.tools/msvc-redist -Recurse -Force
```

Удаление `.tools/vcpkg` и `.vcpkg_installed` приводит к долгому повторному bootstrap. Verifier сам удаляет успешную test installation через NSIS uninstaller; при failed uninstall exact residual path остаётся в evidence для ручной очистки.
