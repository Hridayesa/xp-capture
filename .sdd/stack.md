# Stack profile: Desktop application

Предпочтительный стек: Rust, Tauri, Vue, shadcn-vue, Tailwind, TypeScript, Bun и Vite.

- Tauri commands и events образуют versioned transport boundary между Rust и TypeScript.
- Capabilities, permissions и CSP минимальны и описаны рядом с затрагивающей их спецификацией.
- Frontend должен определить scripts `typecheck`, `lint`, `test:unit` и `build`; Rust часть проходит fmt, clippy и workspace tests.
- PostgreSQL не подразумевается автоматически для desktop. Если он нужен, спецификация обязана определить connectivity, credentials, offline behavior, migrations и testcontainers integration tests.
- Версии выбираются при bootstrap приложения, фиксируются manifests/lockfiles и проверяются на совместимость. Не заменяй их автоматически на latest.
- `$shadcn-vue-engineering` доступен для подтверждённых shadcn-vue задач, но shadcn-vue остаётся предпочтением профиля, а не обязательством проекта. Фактические версии и design decisions задают manifests, lockfiles и ADR; динамическая документация не заменяет локальный design system.
