---
name: safe-archive-change
description: Архивировать завершённый OpenSpec change только после обязательной проверки tasks, strict validation и project quality gate. Использовать вместо generated openspec-archive-change; не применять к незавершённым или непроверенным изменениям.
---

# Safe OpenSpec archive

Этот skill является управляющей границей archive. Не вызывай и не восстанавливай upstream `openspec-archive-change`, потому что его permissive warning policy слабее правил проекта.

1. Получи точное kebab-case имя change из запроса или `bun run tools/openspec.ts list`; не угадывай его.
2. Выполни только `bun run tools/archive-change.ts <name>`.
3. Скрипт fail-closed проверяет status, каждый artifact, unchecked tasks, strict validation и полный project quality gate, затем вызывает archive через pinned launcher.
4. При любом non-zero exit остановись, покажи причину и оставь change активным. Не обходи скрипт прямым вызовом OpenSpec и не используй `--no-validate` или `--skip-specs`.
5. После успеха кратко перечисли evidence, напечатанный скриптом, и путь архива.
