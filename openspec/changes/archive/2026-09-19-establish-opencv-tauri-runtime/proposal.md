# Proposal

## Why

До реализации camera pipeline необходимо отдельно доказать наиболее ранний и дорогой риск PoC: что зафиксированная связка Tauri 2, Rust и OpenCV воспроизводимо собирается на Windows, упаковывает точный набор native DLL и работает из установленного приложения без developer environment. Это создаёт проверяемый фундамент и stop/go gate, после которого имеет смысл реализовывать захват с реальной камеры.

## What Changes

- Создать минимальное Windows-first Tauri 2/Vue приложение с закреплёнными manifests, lockfiles и Rust toolchain; стабильная связка фиксирует Rust crate `tauri` 2.11.5 и соответствующий latest stable `@tauri-apps/cli` 2.11.4 без перехода на Tauri 3 alpha.
- Добавить идемпотентную repository-local подготовку vcpkg/OpenCV и process-local build/run environment без изменения глобального `PATH`.
- Зафиксировать native supply contract: `x86_64-pc-windows-msvc`, dynamic triplet `x64-windows`, vcpkg commit `9e593bb18ea69cc5095e012465dcd675a822ed0d`, `opencv4` 4.12.0#7 и минимальный согласованный feature set.
- Формировать schema-versioned runtime manifest с точными именами, SHA-256, PE architecture, источником и license/notice для всех поставляемых OpenCV/FFmpeg/transitive DLL; staging обязан копировать только перечисленные файлы.
- Реализовать общий Rust self-check, доступный до создания GUI через `--self-check --json <path>` и из минимального diagnostic UI. Он проверяет загрузку OpenCV, обязательные backends, JPEG round-trip и synthetic `CAP_FFMPEG` MJPG/AVI round-trip.
- Добавить воспроизводимые команды bootstrap, install, test, dev, build и bundle verification, включая x64 NSIS installer с offline WebView2 и VC runtime policy.
- Проверять установленное приложение в очищенном окружении, сохранять installer/self-check/module evidence и отклонять загрузку native runtime из build tree или случайного `PATH`.

Изменение намеренно не реализует camera enumeration, profiling, preview, запись реального видеопотока, аппаратную матрицу или итоговый выбор OpenCV против Media Foundation. Эти возможности составят следующий change.

Обязательными являются перечисленный native supply contract, детерминированный self-check и независимость установленного приложения от build environment. Версии Tauri, Vue, Bun и Rust выбираются при bootstrap, фиксируются фактическими manifests/lockfiles и записываются в evidence. Предпочтительным является current-user NSIS install; иной install mode допустим только после явного пересогласования требований hardware lab.

## Capabilities

### New Capabilities

- `opencv-desktop-runtime`: воспроизводимая подготовка, сборка, упаковка и проверка установленного Tauri/OpenCV runtime, включая headless/UI self-check и machine-readable evidence.

### Modified Capabilities

Нет.

## Impact

- Появится новый Tauri/Vue/Rust application scaffold в корне репозитория, прямой bootstrap script `./tools/bootstrap-opencv.ps1` и package scripts `test`, `app:dev`, `app:build`, `verify:bundle`.
- Появятся `vcpkg.json`, `vcpkg-configuration.json`, `rust-toolchain.toml`, Tauri bundle configuration, PowerShell wrappers, runtime manifest/schema и каталог компактных evidence.
- Native dependencies будут занимать repository-local build storage и увеличат NSIS installer из-за OpenCV/FFmpeg DLL, VC runtime и offline WebView2 installer.
- Внешний CLI-контракт приложения пополнится режимом `--self-check --json <path>` с фиксированными exit codes; GUI получит минимальный diagnostic экран, использующий тот же Rust implementation.
- Альтернативы: системная установка OpenCV отвергается как невоспроизводимая; статический vcpkg triplet — как расходящийся с исходным supply contract; переход к Media Foundation откладывается до отдельного camera-pipeline change и его аппаратных результатов.
