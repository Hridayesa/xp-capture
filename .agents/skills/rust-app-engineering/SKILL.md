---
name: rust-app-engineering
description: Проектировать и изменять приложения на Rust с профилями Axum/PostgreSQL или Tauri/Vue, включая границы модулей, типизированные ошибки и интеграционные тесты. Не применять к задачам, не затрагивающим Rust или выбранный application stack.
---

# Rust application engineering

Сначала прочитай `.sdd/stack.md` и manifests/lockfiles проекта. Используй только относящиеся к задаче ссылки:

- Для модели ошибок прочитай [references/error-handling.md](references/error-handling.md).
- Для Axum/PostgreSQL прочитай [references/axum-postgres.md](references/axum-postgres.md).
- Для Tauri/Vue прочитай [references/tauri-vue.md](references/tauri-vue.md).

## Общие правила

- Разделяй domain, application/use-cases, adapters и delivery. Не передавай типы Axum, SQLx или Tauri в доменное ядро без необходимости.
- Делай транзакционные границы явными на уровне use-case; repository не должен неожиданно commit отдельные части одной операции.
- Публичные DTO и persistence models отделяй от domain types, если их жизненные циклы или инварианты различаются.
- Для конкурентных и фоновых операций указывай ownership, cancellation, retry и idempotency semantics.
- Выбирай зависимости по фактической задаче. Упомянутый профиль не является разрешением добавлять все компоненты сразу.
- Проверяй выбранные API по документации применимой версии. Lockfiles проекта являются источником установленной версии.
