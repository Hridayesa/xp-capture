# Аудит контекста Codex и рекомендации по оркестрации

Дата анализа: 2026-09-19.

Документ фиксирует причины разрастания главного контекста при реализации OpenSpec change `add-opencv-mode-profiling` и предлагает изменения в проектном harness: `AGENTS.md`, skills, custom agents, handoff-протокол и инструменты проверки.

## Краткий вывод

Главный контекст раздулся не из-за одного большого файла или избыточного `AGENTS.md`. Причиной стала комбинация факторов:

1. Один главный агент последовательно выполнил весь change, затрагивающий domain model, OpenCV adapter, service state machine, Tauri transport, TypeScript, Vue UI, hardware harness, installer и evidence.
2. `openspec-apply-change` потребовал прочитать все planning artifacts целиком и продолжать работу до завершения или блокировки.
3. В действовавших инструкциях было запрещено проактивно создавать субагентов, если пользователь, `AGENTS.md` или skill явно не требуют делегирования.
4. В главный контекст попадали крупные листинги исходников, широкие результаты поиска, повторные OpenSpec task lists и подробные test/build logs.
5. Финальные проверки частично дублировали друг друга: `bun run test`, `app:build`, `verify:bundle`, `tools/verify.ts` и отдельная strict validation.
6. Длинный apply-turn потребовал compaction, после которого в контексте осталась большая рабочая сводка.

Субагенты могут существенно уменьшить именно главный рабочий контекст и снизить context contamination. Они не гарантируют уменьшения общего числа использованных токенов: каждый агент имеет собственный контекст и обычно увеличивает суммарное потребление. Их ценность — изоляция noisy exploration, raw logs и локальной реализации от контекста оркестратора. Это соответствует официальным рекомендациям OpenAI: держать главный поток сфокусированным на требованиях, решениях и итогах, делегировать read-heavy задачи и осторожно относиться к параллельной записи в общий workspace.

Источник: [OpenAI Docs — Subagents](https://developers.openai.com/es-419/docs/agent-configuration/subagents).

## Что попало в главный контекст

Точного token ledger для завершённого turn нет. Ниже приведены оценки по фактическим размерам файлов и объёму вывода команд.

| Источник | Оценка | Обстоятельства |
| --- | ---: | --- |
| System/developer instructions, tool schemas, skill catalog | примерно 15–30 тыс. токенов baseline | Автоматически загружаются в контекст; объём зависит от набора доступных tools/plugins/skills. |
| Корневой `AGENTS.md` | около 1 тыс. токенов | Загружается как постоянная проектная инструкция. |
| Активированные skills | несколько тысяч токенов | Полностью читались `openspec-apply-change`, `rust-app-engineering`, его references и `quality-gate`. |
| OpenSpec change artifacts | 76,6 KB, примерно 20–25 тыс. токенов | Apply-skill требует прочитать proposal, design, specs и tasks. |
| Поверхность изменённых файлов | около 613 KB / 11,3 тыс. строк | Значительные части крупных Rust/TS/Vue файлов читались при реализации и отладке. |
| Full test/build output | примерно 5–20 тыс. токенов за запуск | Rust выводил имена всех тестов; полные gates выполнялись несколько раз. |
| Hardware/OpenCV output | примерно 5–20 тыс. токенов | 30 tuples, native warnings, ожидание и итоговые метрики. |
| OpenSpec CLI JSON | до 10–20 тыс. токенов за полный вывод | `instructions apply --json` повторяет полные тексты всех задач. |
| Compaction summary | несколько тысяч токенов | Сформирован после продолжительного turn и сохранил рабочее состояние для дальнейшей работы. |

Крупнейшие локальные источники:

| Файл | Размер / строки | Риск для контекста |
| --- | ---: | --- |
| `openspec/changes/add-opencv-mode-profiling/design.md` | 27,3 KB / 331 строка | Полный архитектурный документ. |
| `openspec/changes/add-opencv-mode-profiling/specs/opencv-mode-profiling/spec.md` | 22,9 KB / 265 строк | Подробные requirements и scenarios. |
| `openspec/changes/add-opencv-mode-profiling/tasks.md` | 11,7 KB / 38 длинных строк | 30 задач с развёрнутыми acceptance criteria. |
| `src-tauri/src/camera/service.rs` | 88,5 KB / 2274 строки | Production code и большой inline test module. |
| `src-tauri/src/camera/profiling.rs` | 56,8 KB / 1613 строк | Domain model и большое число inline tests. |
| `evidence/camera-mode-profile-smoke.json` | 130,9 KB / одна строка | Случайный raw dump стоил бы примерно 35–45 тыс. токенов. |

Сам факт существования большого файла не расходует контекст. Он становится проблемой только при чтении или возврате его содержимого через tool output. Hardware JSON в завершённом turn не был целиком выведен в главный поток, но остаётся существенным риском для следующих запусков.

## Почему субагенты не использовались

На момент apply действовало правило: не создавать субагентов, если пользователь, применимый `AGENTS.md` или skill явно не требуют делегирования. Пользователь вызвал `$openspec-apply-change`, но текущие проектные инструкции и skill не разрешали и не предписывали multi-agent workflow.

Следовательно, для системного изменения поведения необходимо добавить разрешение и критерии делегирования именно в `AGENTS.md` или отдельный project skill. Разовая надежда на то, что оркестратор сам выберет субагентов, при такой policy не работает.

## Целевая модель работы

Рекомендуемая модель: **single orchestrator + clean bounded agents + exclusive writer ownership**.

Оркестратор остаётся единственным владельцем:

- цели и подтверждённых решений;
- task graph и зависимостей между workstreams;
- OpenSpec proposal/design/spec/tasks и task checkboxes;
- shared integration files и окончательной сборки изменений;
- requirement → evidence traceability;
- итогового quality gate и финального отчёта.

Субагенты получают:

- чистый контекст через `fork_turns: "none"`;
- одну ограниченную цель;
- конкретный список required files;
- проверяемые acceptance criteria;
- точные read-only и owned paths;
- stop conditions;
- жёсткий формат короткого результата.

По умолчанию субагент должен быть read-only. Запись разрешается только implementer-агентам с непересекающимися ownership scopes.

## Рекомендуемый phase graph

```text
P0 Contract capsule
   │  status, proposal, task graph, invariants, acceptance IDs
   ▼
P1 Pure domain / policy / metrics
   │  targeted Rust tests
   ▼
P2 Adapter / worker / service state
   │  targeted Rust tests, state-transition summary
   ▼
P3 Rust + TypeScript transport contracts
   │  serde and decoder tests
   ▼
P4 Vue UI
   │  component tests
   ▼
P5 Deterministic integration gate
   │  один канонический полный запуск
   ├───────────────┐
   ▼               ▼
P6a Hardware       P6b Installer / bundle
clean context      clean context
compact metrics    compact stage report
   └───────┬───────┘
           ▼
P7 Fresh final auditor
OpenSpec strict validation, traceability и diff review;
повтор full gate только при изменении исходников после P5
```

Не все фазы должны выполняться параллельно. Например, service worker зависит от domain contracts, а UI worker — от стабилизированного transport contract. Параллелить следует независимые leaf-задачи, read-only exploration, документацию и анализ логов.

## Изменения в `AGENTS.md`

В корневой `AGENTS.md` рекомендуется добавить короткий обязательный раздел:

```markdown
## Делегирование сложных изменений

- Для OpenSpec apply с 8 и более задачами, изменением трёх и более подсистем
  или hardware/bundle evidence используй субагентов с чистым контекстом.
- Главный агент остаётся владельцем цели, решений, OpenSpec-артефактов,
  task checkboxes, integration changes и итогового quality gate.
- Субагенты запускаются с `fork_turns: "none"` и получают bounded task packet:
  цель, required files, acceptance criteria, owned paths и stop conditions.
- По умолчанию субагенты работают read-only. Запись разрешается только
  в явно перечисленные exclusive owned paths; writer scopes не пересекаются.
- Proposal, design, specs, tasks, lockfiles и общие registration/config files
  изменяет только главный агент, если явно не назначен один последовательный владелец.
- Результат субагента — не более 200 слов: outcome, changed files, checks,
  unresolved decisions и integration notes. Raw logs и полный diff не возвращать.
- При необходимости записи вне ownership субагент останавливается и возвращает
  integration note.
```

Текущий `AGENTS.md` имеет размер около 3,9 KB и не является главным источником раздувания. Его можно дополнительно сократить, переместив PostgreSQL, `unwrap/expect` и другие stack-specific правила в `rust-app-engineering`, но ожидаемый выигрыш невелик. При таком переносе trigger `rust-app-engineering` должен остаться обязательным и надёжным.

## Отдельный orchestration skill

Не рекомендуется напрямую расширять `.agents/skills/openspec-apply-change/SKILL.md`: файл помечен `generatedBy: "1.13.1"` и может быть перезаписан при обновлении wrapper.

Следует создать project skill:

```text
.agents/skills/orchestrated-openspec-apply/
  SKILL.md
  references/
    task-packet.md
    result-contract.md
```

Skill должен:

1. Активироваться вместе с `openspec-apply-change`, когда change превышает заданный порог сложности.
2. Явно разрешать и требовать clean-context subagents.
3. Строить workstreams, dependency graph и ownership table.
4. Оставлять OpenSpec artifacts и checkboxes в собственности оркестратора.
5. Запрещать конкурентную запись в пересекающиеся файлы.
6. Ограничивать размер результата субагента.
7. Передавать финальную проверку отдельному verifier-агенту.
8. Возвращать управление существующим quality/archive workflow.

Ключевое изменение относительно текущего apply-skill: в orchestrated mode все `contextFiles` должны быть прочитаны ответственными агентами, но не обязательно главным агентом целиком.

Рекомендуемое распределение:

- оркестратор читает proposal, tasks и компактный decision summary;
- `spec_mapper` читает полные design/specs и формирует contract capsule;
- оркестратор открывает оригинальные секции для спорных, security-sensitive и архитектурных решений;
- финальный verifier независимо сверяет diff с полными specs.

Без этого изменения основной агент всё равно загрузит 20–25 тыс. токенов OpenSpec-контекста, и делегирование даст лишь частичный эффект.

## Custom agents и конфигурация

Рекомендуемые project-local agents:

```text
.codex/agents/spec-mapper.toml
.codex/agents/implementer.toml
.codex/agents/reviewer.toml
.codex/agents/verifier.toml
```

Пример минимальной конфигурации:

```toml
[agents]
enabled = true
max_concurrent_threads_per_session = 3
default_subagent_model = "gpt-5.6-terra"
default_subagent_reasoning_effort = "medium"
```

Назначение ролей:

- `spec_mapper`: read-only; извлекает contracts, invariants, scenarios и ссылки на исходные секции.
- `implementer`: изменяет только explicit owned paths, запускает targeted checks.
- `reviewer`: read-only; ищет correctness, security, regression и missing tests.
- `verifier`: запускает проверки и формирует компактную requirement → evidence matrix без возврата raw logs.

Для сложных security/state-machine проверок можно использовать `high` reasoning effort. Для поиска файлов, классификации и суммаризации достаточно `medium` или `low`.

Custom agent prompts не должны копировать весь `AGENTS.md` или OpenSpec skills. Они должны содержать только специфичную роль и ограничения; остальные настройки наследуются стандартным механизмом.

## Task packet

Каждый субагент должен получать самодостаточный пакет задачи. Пример:

```yaml
schema: delegation-task/v1
run_id: 20260919T120000Z-profile
task_id: impl-profile-decoder
packet_revision: 1
change: add-opencv-mode-profiling
openspec_tasks: ["4.3"]
role: implementer
objective: "Реализовать strict decoder profile DTO."
success:
  - "Unknown schema и enum отклоняются управляемо."
  - "Targeted Vitest проходит."
required_context:
  - AGENTS.md
  - openspec/changes/add-opencv-mode-profiling/design.md
  - openspec/changes/add-opencv-mode-profiling/specs/opencv-mode-profiling/spec.md
  - src/api/camera.ts
write_mode: exclusive
owned_paths:
  - src/api/profile.ts
  - src/api/profile.test.ts
read_only_paths:
  - openspec/changes/**
  - package.json
forbidden:
  - "Редактировать proposal/design/spec/tasks."
  - "Менять dependencies, lockfiles и shared exports."
  - "Запускать repo-wide formatter или git-команды."
checks:
  - bun run test:unit -- src/api/profile.test.ts
stop_if:
  - "Нужна запись вне owned_paths."
  - "Fingerprint входного файла изменился."
  - "Требование допускает несколько несовместимых трактовок."
output_limit_words: 200
```

Для файлов с существующими пользовательскими изменениями пакет должен содержать baseline fingerprint. Несовпадение fingerprint — причина остановиться, а не пытаться автоматически объединить изменения.

## Handoff и рабочие файлы

Для небольших задач достаточно финального сообщения субагента не более 200 слов. Для долгих, прерываемых или возобновляемых change следует использовать versioned immutable handoff-файлы:

```text
.codex/work/<change>/<run-id>/
  manifest.json
  tasks/<task-id>.yaml
  results/<task-id>-v001.json
  logs/<task-id>.log
```

`.codex/work/` следует добавить в `.gitignore`.

Не следует помещать процессные handoff-файлы внутрь `openspec/changes/<change>`: они попадут в архив change. В design, ADR или evidence нужно переносить только долговечные решения и доказательства.

Минимальный результат:

```json
{
  "schema": "delegation-result/v1",
  "task_id": "profile-transport",
  "status": "complete",
  "ownership_released": true,
  "changed_files": [
    "src/api/profile.ts",
    "src/api/profile.test.ts"
  ],
  "checks": [
    {
      "command": "bun run test:unit -- src/api/profile.test.ts",
      "status": "passed",
      "tests": 12
    }
  ],
  "decisions": [],
  "integration_notes": []
}
```

Финальный ответ агента оркестратору должен содержать только:

- `status`;
- handoff path, если он создан;
- changed files;
- выполненные проверки;
- blockers или integration notes.

Запрещается возвращать raw logs, полный diff, повтор OpenSpec-контекста и большие JSON artifacts.

## Ownership и совместная запись

До запуска writer-агентов оркестратор строит таблицу `task → owned_paths`.

Правила:

1. Пересекающиеся writer scopes запрещены.
2. Нельзя выдавать широкое ownership вроде `src/**`.
3. `tasks.md`, proposal/spec/design, manifests, lockfiles, root exports, registration/config и requirement matrix остаются orchestrator-only.
4. Если implementer обнаружил необходимость изменить shared file, он возвращает integration note и не делает правку самостоятельно.
5. Shared workspace не требует cherry-pick: оркестратор проверяет фактический diff ownership set и затем делает glue changes.
6. Task checkbox отмечает только оркестратор после review и релевантной проверки.
7. Quality gate запускается после освобождения всех writer leases.

## Снижение объёма tool output

Это один из наиболее эффективных и наименее рискованных шагов.

### Структурированный quality gate

Нужно расширить `tools/verify.ts` параметрами наподобие:

```text
bun run tools/verify.ts --quiet --report .codex/work/<change>/<run>/quality-gate.json
```

Поведение:

- при успехе печатать одну строку на gate: имя, status, duration и test count;
- полный stdout/stderr сохранять в log-файл;
- при failure показывать только короткую диагностику и последние 20–50 строк;
- сохранять fingerprint входных файлов и конфигурации;
- разрешать повторное использование успешного отчёта, если fingerprint не изменился.

### Исключение дублирующих gates

Следует определить один канонический deterministic gate. Например, `tools/verify.ts` может быть единственной точкой, которая запускает Rust, frontend и runtime-supply.

Если после успешного gate исходники не менялись:

- `quality-gate` должен читать structured report, а не повторять команды;
- strict OpenSpec validation может запускаться отдельно, поскольку она дешёвая;
- installer/bundle verification использует уже подтверждённый build fingerprint;
- повторный full gate нужен только после изменения исходников или build configuration.

### OpenSpec CLI

Когда нужны только progress и state, нельзя возвращать весь `instructions apply --json` в чат. Следует извлекать только:

- `changeName`;
- `schemaName`;
- `progress`;
- `state`;
- remaining task IDs;
- context file paths;
- blockers.

Полные task descriptions читаются из `tasks.md` ответственным агентом и не должны многократно повторяться в tool output.

### Evidence

Большие JSON evidence нужно анализировать агрегатами:

- schema/status;
- количество результатов;
- распределение status/failure reasons;
- extrema метрик;
- decision gates;
- config hash;
- release/reopen result.

Raw JSON не должен выводиться в чат.

### Исходники

Для крупных Rust-файлов:

- сначала искать symbols через `rg`;
- читать узкие диапазоны;
- разделять production code и блок после `#[cfg(test)]`;
- при review использовать изменённые hunks и соседние definitions;
- не выполнять широкий `rg` без ограничения файлов и количества результатов.

## Упрощение существующих инструкций

В `.agents/skills/` находится около 95 KB инструкций. Это не означает, что все они целиком загружаются в каждый turn: постоянно присутствует прежде всего skill catalog, а полный `SKILL.md` читается после срабатывания trigger.

Тем не менее generated OpenSpec skills содержат повторяющиеся блоки `Store selection`, `Project check`, guardrails и большие output examples. Рекомендуется изменять их generator/wrapper, а не generated files:

- механические root/store/no-init проверки перенести в pinned launcher;
- оставить в skills только поведенческие invariants, которые CLI не может проверить;
- удалить повторные полноразмерные examples;
- сохранять короткие, но точные frontmatter descriptions, чтобы не сломать trigger routing;
- использовать progressive disclosure через references только для действительно условного контекста.

`quality-gate`, `safe-archive-change` и `rust-app-engineering` уже достаточно компактны. Их сокращение имеет низкий приоритет.

## Варианты внедрения

### Вариант A: минимальный

Изменения:

- добавить раздел делегирования в `AGENTS.md`;
- для больших apply явно запускать 2–3 clean-context read-only агента;
- ограничить их финальные ответы 200 словами;
- использовать targeted tests и один полный gate.

Плюсы: почти не требует разработки harness.

Минусы: task packets и ownership остаются неформальными; процесс хуже восстанавливается после compaction или новой сессии.

### Вариант B: рекомендуемый

Изменения:

- вариант A;
- `orchestrated-openspec-apply` skill;
- project-local custom agents;
- `.codex/work/` с immutable task/result contracts;
- `verify.ts --quiet --report`;
- один канонический deterministic gate с fingerprint.

Плюсы: существенно уменьшает главный контекст, формализует ownership и делает процесс возобновляемым.

Минусы: нужно поддерживать schema handoff-файлов и дополнительный skill.

### Вариант C: расширенный orchestration harness

Дополнительно:

- генератор workstreams из OpenSpec tasks;
- автоматические ownership leases;
- проверка пересечений changed files;
- автоматический stale-input detection по hash;
- consolidated machine-readable evidence index;
- resumable run manifest.

Плюсы: хорошо подходит для регулярных крупных change.

Минусы: заметная стоимость реализации и риск построить слишком сложный внутренний workflow раньше, чем появится достаточная статистика использования.

## Рекомендуемый порядок внедрения

1. Добавить разрешение и обязательные инварианты делегирования в `AGENTS.md`.
2. Создать `orchestrated-openspec-apply` skill, не изменяя generated apply-skill напрямую.
3. Добавить `spec-mapper`, `implementer`, `reviewer` и `verifier` agents.
4. Добавить `.codex/work/` в `.gitignore` и зафиксировать schemas `delegation-task/v1` и `delegation-result/v1`.
5. Реализовать quiet structured output и fingerprint в `tools/verify.ts`.
6. Убрать дублирование полного deterministic gate между OpenSpec tasks и `$quality-gate`.
7. После нескольких реальных запусков оценить, нужен ли автоматический ownership/lease manager.
8. Только затем оптимизировать generated OpenSpec skills и общую tool/plugin surface.

## Метрики успеха

После внедрения следует измерять:

- количество raw log строк, попавших в главный контекст;
- число полных чтений OpenSpec artifacts главным агентом;
- число повторных full gates при неизменившемся source fingerprint;
- размер финальных subagent handoffs;
- число конфликтов writer scopes;
- число случаев, когда оркестратор вынужден повторно исследовать уже закрытую область;
- продолжительность change и число compaction events;
- суммарное потребление токенов отдельно от размера главного контекста.

Целевой результат для change масштаба `add-opencv-mode-profiling`:

- главный контекст в 3–5 раз меньше;
- не более одного полного deterministic gate при неизменных исходниках;
- raw hardware/build/test logs не попадают в главный поток;
- каждый subagent handoff не превышает 1–2 тыс. токенов, а типовой результат — 200 слов;
- отсутствие параллельной записи в пересекающиеся файлы;
- все долгоживущие решения остаются в OpenSpec/ADR/evidence, а процессные данные — в ignored `.codex/work/`.

## Итоговая рекомендация

Оптимальным является вариант B. Он устраняет главный организационный дефект — невозможность делегировать большой apply при текущих инструкциях — и одновременно ограничивает риски общего workspace.

Главный принцип: оркестратор должен хранить решения, зависимости, ownership и итоговую доказательную базу, но не сырые листинги, полные test logs и локальные подробности реализации каждого workstream.
