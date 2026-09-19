# XP Capture runtime foundation

Первый change проекта проверяет воспроизводимый Windows runtime для Tauri 2 и dynamic OpenCV до реализации camera pipeline. Приложение предоставляет один Rust self-check через headless CLI и diagnostic GUI; камера не открывается и camera permission не требуется.

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

## Локальное хранение и evidence

- `.tools/vcpkg/` — exact vcpkg checkout; `.tools/msvc-redist/` — выбранные из установленного MSVC Redistributable DLL и notice.
- `.vcpkg_installed/` — pinned dynamic OpenCV install tree.
- `runtime/manifest.json` — tracked versioned allowlist; `runtime/staging/` — generated payload.
- `target/`, `dist/` — build outputs; NSIS installer остаётся локальным.
- `evidence/environment-summary.json` — согласованные версии host/toolchain/native supply.
- `evidence/tauri-build-environment.json` — target и effective `STATIC_VCRUNTIME=true`.
- `evidence/runtime-imports.json` — release PE import closure.
- `evidence/bundle-verification.json` — installer/executable hashes и этапы `manifest`, `install`, `headless_self_check`, `module_provenance`, `gui_smoke`, `uninstall`.
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
