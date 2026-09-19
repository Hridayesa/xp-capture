# Статус реализации `establish-opencv-tauri-runtime`

> Актуальный handoff на 2026-09-19. Предыдущая версия полностью заменена.

> Финальное обновление: implementation и quality gate завершены, **31/31 tasks complete**. Change остаётся активным и не архивирован; архивирование допустимо только по отдельной команде через `$safe-archive-change`. Разделы ниже сохраняют историю реализации, но списки «Следующие задачи» больше не являются pending work.

## Быстрый старт в новом контексте

Рабочий каталог: `C:\Work\js\xp\xp-capture`.

Активный OpenSpec change: `establish-opencv-tauri-runtime`, schema `spec-driven`.

Перед продолжением:

1. Прочитать корневой `AGENTS.md`, `.agents/skills/openspec-apply-change/SKILL.md`, `.agents/skills/rust-app-engineering/SKILL.md`, `.sdd/stack.md` и этот файл.
2. Выполнить:

   ```powershell
   bun run tools/openspec.ts instructions apply --change establish-opencv-tauri-runtime --json
   ```

3. Перечитать все перечисленные `contextFiles`: `proposal.md`, `design.md`, `specs/opencv-desktop-runtime/spec.md`, `tasks.md`.
4. Проверить финальный status: `31/31 tasks complete`, state `all_done`.
5. Не продолжать camera pipeline в этом change.
6. Архивировать только по отдельной команде через `$safe-archive-change`.

Все OpenSpec-команды выполняются только через `bun run tools/openspec.ts`.

## Текущий прогресс

- Schema: `spec-driven`.
- Progress: **31/31 tasks complete**, 0 remaining.
- Planning artifacts прошли:

  ```powershell
  bun run tools/openspec.ts validate establish-opencv-tauri-runtime --strict
  ```

- Результат: `Change 'establish-opencv-tauri-runtime' is valid`.
- Tasks 3.2–3.4 были ранее выполнены для OpenCV/FFmpeg closure, но намеренно переоткрыты после уточнения стабильного Tauri/VC runtime contract.

## Зафиксированные решения

### Native OpenCV

- Native supply contract остаётся **OpenCV 4.12.0#7** из pinned vcpkg baseline.
- vcpkg commit: `9e593bb18ea69cc5095e012465dcd675a822ed0d`.
- Triplet: `x64-windows`.
- Features: `dshow`, `ffmpeg`, `jpeg`, `msmf`, `thread`; `default-features = false`.
- Rust crate bindings: `opencv = "=0.100.1"`.
- Связка crate `opencv 0.100.1` + native OpenCV `4.12.0` фактически скомпилирована и прошла integration test.

На машине пользователя установлен глобальный OpenCV 4.13.0 и заданы `OPENCV_BIN`, `OPENCV_INCLUDE_PATHS`, `OPENCV_LINK_LIBS`, `OPENCV_LINK_PATHS`. Они не являются частью supply contract и не должны влиять на сборку.

`tools/run-with-opencv.ps1` теперь:

- удаляет у дочернего процесса все унаследованные `OPENCV_*` и `VCPKG_*`;
- задаёт только repository-local pinned paths;
- добавляет `.vcpkg_installed/x64-windows/bin` только в child `PATH`;
- не меняет parent/global environment.

Тест wrapper подтвердил, что `OPENCV_BIN` не протекает в child process.

### Tauri

- Rust crate: `tauri = "=2.11.5"`.
- Build crate: `tauri-build = "=2.6.3"`.
- JavaScript API: `@tauri-apps/api = 2.11.1`.
- Stable CLI: `@tauri-apps/cli = 2.11.4`.
- Tauri 3 alpha не используется.

Важное уточнение: stable config schema CLI 2.11.4 ещё не поддерживает поля `build.windows.staticVCRuntime` и `bundle.windows.bundleVCRuntime`, хотя они присутствуют в более новом online reference.

OpenSpec пересогласован без изменения наблюдаемого требования:

- application binary должен собираться с effective `STATIC_VCRUNTIME=true` (Tauri CLI 2.11.4 выставляет это значение для MSVC build);
- VC runtime, требуемый dynamic OpenCV/FFmpeg DLL, должен поставляться app-local;
- VC runtime DLL должны войти в runtime manifest с точными SHA-256, `x64`, source и license metadata;
- переход на Tauri 3 alpha или непубликуемый Git snapshot запрещён без нового согласования.

## Выполненная реализация

### 1. Scaffold и security boundary

- Создан Tauri 2 + Vue + TypeScript + Bun scaffold.
- Зафиксированы `package.json`, `bun.lock`, workspace `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`.
- Rust toolchain: `1.98.1`, target `x86_64-pc-windows-msvc`.
- Scripts: `typecheck`, `lint`, `test:unit`, `build`, `test`, `app:dev`, `app:build`, `verify:bundle`.
- Tauri capability `diagnostics` содержит пустой permission list.
- CSP ограничен local UI и Tauri IPC.
- Production fallible paths не используют `unwrap()`/`expect()`.

### 2. Repository-local OpenCV supply

- Реализованы:
  - `tools/verify-environment.ps1`;
  - `tools/bootstrap-opencv.ps1`;
  - `tools/run-with-opencv.ps1`.
- Bootstrap проверяет exact vcpkg revision до install и фактические port/triplet/features.
- `.tools/vcpkg` и `.vcpkg_installed` являются local ignored roots; не удалять их без необходимости.
- Wrapper запускается корректно и через `pwsh -File`; его argument forwarding исправлен так, чтобы Cargo separator `--` доходил до clippy/test harness.

### 3. Runtime manifest и staging — базовая OpenCV/FFmpeg часть

- Реализованы schema/model/tooling:
  - `tools/runtime-manifest.schema.json`;
  - `tools/runtime-manifest.psm1`;
  - `tools/pe-tools.psm1`;
  - `tools/generate-runtime-manifest.ps1`;
  - `tools/stage-runtime.ps1`;
  - `tools/validate-runtime-imports.ps1`;
  - `tools/validate-runtime-manifest.mjs`.
- Текущий `runtime/manifest.json` содержит 11 OpenCV/FFmpeg/JPEG/zlib DLL с hashes, x64 architecture и licenses.
- Staging копирует только allowlist и очищает только проверенный `runtime/staging`.
- Есть negative fixtures для wildcard, hash/architecture/license metadata и unresolved import.

Эта часть требует расширения VC runtime и повторной проверки — tasks 3.2–3.4 не завершены в текущем контракте.

### 4. Общий Rust self-check

- `SelfCheckService` реализует checks:
  - `opencv_load`;
  - `image_codec`;
  - `writer_open`;
  - `writer_backend`;
  - `writer_roundtrip`.
- Реализованы typed errors и exit codes `10`, `11`, `12`, `13`, `14`, `20`.
- OpenCV adapter:
  - version и build-information SHA-256;
  - backends `MSMF`, `DSHOW`, `FFMPEG`;
  - `Mat 640x480 -> resize 320x240 -> JPEG encode/decode`;
  - 30 frames 320x240 через `CAP_FFMPEG`, `MJPG`, AVI;
  - finalize/reopen/read с проверкой backend, count и dimensions;
  - deterministic temporary AVI cleanup.
- Native integration test прошёл с OpenCV 4.12.0.

### 5. Module provenance и CLI

- Реализован `WindowsModuleAdapter` через Win32 process-module API.
- Проверяются canonical path, manifest allowlist и SHA-256.
- Unit tests покрывают:
  - installed/system origin;
  - hash mismatch;
  - manifest DLL из build-tree path.
- Реализован parsing `--self-check --json <path>` до создания Tauri GUI.
- Report записывается через sibling temporary file и atomic replace.
- Subprocess tests создают временный installed-like layout:
  - staged runtime возвращает `0`, не создаёт GUI и пишет successful report;
  - DLL из `.vcpkg_installed` возвращают `10` и сохраняют report.
- Реализован Tauri command `run_self_check` через `spawn_blocking` поверх того же `SelfCheckService`.
- Transport DTO не раскрывает internal module paths/source chain.

### 6. Diagnostic UI

- Реализован centralized TypeScript client `src/api/self-check.ts`.
- Decoder поддерживает transport schema v1 и управляемо отклоняет unknown schema/error code.
- Raw native rejection не показывается пользователю.
- Vue UI поддерживает idle/loading/result/error/retry.
- Все пять checks отображаются отдельно.
- Component tests покрывают responsive loading, success, safe native error и retry.
- UI явно camera-independent.
- `src-tauri/capabilities/diagnostics.json` не содержит camera permission.
- Единственный `VideoCapture` в текущем code path — `VideoCapture::from_file` для чтения synthetic temporary AVI; camera device не открывается.
- Evidence: `evidence/camera-independent-gui.json`.

Native window automation была недоступна: Computer Use вернул пустой inventory. При этом Tauri GUI process реально запускался через `bun run app:dev`, staged headless service прошёл, а UI command contract покрыт component tests. Это ограничение явно записано в evidence.

## Последние успешные проверки

### Rust/OpenCV

```powershell
$env:XP_CAPTURE_OPENCV_INTEGRATION='1'
./tools/run-with-opencv.ps1 cargo test --workspace --locked --test opencv_runtime
```

Результат: 1/1 passed; OpenCV version `4.12.0`, required backends, JPEG и 30-frame MJPG/AVI round-trip подтверждены.

```powershell
pwsh -NoProfile -File ./tools/run-with-opencv.ps1 cargo clippy --workspace --all-targets --locked -- -D warnings
pwsh -NoProfile -File ./tools/run-with-opencv.ps1 cargo test --workspace --locked
```

Последний полный Rust run:

- 22 unit tests passed;
- 2 `headless_cli` subprocess tests passed;
- 1 OpenCV runtime integration test passed;
- 2 security config tests passed;
- doc tests passed;
- clippy с `-D warnings` passed.

### Frontend

```powershell
bun run typecheck
bun run lint
bun run test:unit
bun run build
```

Последние результаты:

- typecheck passed;
- lint passed;
- 3 test files / 14 tests passed;
- production Vite build passed.

### Wrapper isolation

```powershell
pwsh -NoProfile -File ./tools/tests/test-run-with-opencv.ps1
```

Результат: `run-with-opencv process isolation verified`.

### GUI smoke

`bun run app:dev` запустил Vite и `target/debug/xp-capture.exe`; процесс был затем остановлен Ctrl+C. Exit `0xc000013a` относится к controlled Ctrl+C shutdown и не является startup failure.

Полный project quality gate выполнен: `bun run tools/verify.ts` — 7/7 command groups passed; strict OpenSpec validation, pinned Rust/OpenCV tests, frontend checks и runtime-supply checks прошли.

## Следующие задачи

### Task 3.2 — app-local VC runtime в manifest

1. Исследовать imports фактических OpenCV/FFmpeg DLL и определить обязательные MSVC runtime DLL для app-local deployment.
2. Найти их только в фактическом MSVC Redistributable toolchain, не в произвольном global `PATH`.
3. Проверить redistribution/license source.
4. Изменить generator так, чтобы VC runtime entries включались в `runtime/manifest.json` как `transitive` с exact source/hash/x64/license metadata.
5. Перегенерировать manifest и проверить каждый entry.

Не считать DLL из `System32` доказательством app-local supply.

### Task 3.3 — staging

1. Проверить, что существующий generic staging корректно копирует новые VC runtime entries.
2. Повторить positive staging.
3. Повторить controlled hash mismatch и unlisted-file cleanup tests.

### Task 3.4 — import validation

1. Изменить классификацию imports: обязательные MSVC runtime imports должны разрешаться из staging, а не приниматься как generic system imports.
2. Проверить release executable + все staged DLL.
3. Сохранить negative unresolved-import fixture.
4. Добавить negative/positive coverage именно для VC runtime origin.

Только после этих проверок вернуть checkboxes 3.2–3.4 в `[x]`.

### Tasks 6.1–6.5 — NSIS и installed verification

- Настроить x64 current-user NSIS и offline WebView2.
- Проверить effective `STATIC_VCRUNTIME=true` по resolved build/evidence.
- Bundle resources должны строиться только из manifest-driven staging.
- `app:dev`/`app:build` должны валидировать manifest и staging.
- Build должен создавать ровно один однозначный NSIS artifact.
- Реализовать `tools/verify-bundle.ps1` с этапами:
  - `manifest`;
  - `install`;
  - `headless_self_check`;
  - `module_provenance`;
  - `gui_smoke`;
  - `uninstall`.
- Installed self-check запускается без `VCPKG_*`/`OPENCV_*` и с system-only `PATH`.
- Проверить installer/executable hashes, module paths/hashes, GUI startup и uninstall.

### Tasks 7.1–7.5 — документация и итоговый gate

- README с prerequisites, clean-checkout flow, storage, exit codes, evidence, troubleshooting, cleanup.
- Environment evidence для Windows/MSVC/SDK/Rust/Bun/Tauri/OpenCV/CMake/Ninja/vcpkg.
- Полный `bun run test`.
- Clean-checkout-like последовательность:

  ```powershell
  ./tools/bootstrap-opencv.ps1
  bun install --frozen-lockfile
  bun run test
  bun run app:build
  bun run verify:bundle
  ```

- Сопоставление requirement → evidence.
- Strict OpenSpec validation и `$quality-gate`.
- Не архивировать при непроверенном installed-app/module-provenance gate.

## Важные инженерные ограничения

- Не менять native OpenCV 4.12.0#7 без нового согласования.
- Не использовать глобальный OpenCV 4.13.0.
- Не обновлять Tauri на 3 alpha.
- Не добавлять неподдерживаемые config fields `build.windows.staticVCRuntime` и `bundle.windows.bundleVCRuntime` в stable CLI 2.11.4.
- Не удалять `.tools/vcpkg` и `.vcpkg_installed`: bootstrap занял значительное время.
- Не разрешать installed self-check за счёт `.vcpkg_installed`, `target`, developer `PATH` или global OpenCV.
- Staging cleanup разрешён только внутри заранее проверенного `runtime/staging`.
- Не использовать destructive Git commands и не переписывать несвязанные пользовательские файлы.
- Рабочее дерево практически целиком untracked; сохранять все существующие файлы как пользовательские/текущие изменения.
- Checkbox отмечается только после реализации и относящейся проверки.

## Быстрые команды ориентации

```powershell
bun run tools/openspec.ts instructions apply --change establish-opencv-tauri-runtime --json
bun run tools/openspec.ts validate establish-opencv-tauri-runtime --strict
Get-Content -Raw openspec/changes/establish-opencv-tauri-runtime/tasks.md
Get-Content -Raw openspec/changes/establish-opencv-tauri-runtime/WORK_STATUS.md
git status --short
```

Текущая точка: **implementation complete; change не архивирован**.
