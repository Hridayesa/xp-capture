# Axum и PostgreSQL

- Handlers выполняют transport parsing/auth и вызывают application service; бизнес-правила не живут в extractor или response mapper.
- State содержит дешёво клонируемые handles/pools, а не request-specific mutable state.
- Преобразуй application errors в HTTP response централизованно и возвращай стабильный машинно-читаемый code.
- Миграции версионируются и проверяются с чистой базой. Изменение schema описывает forward/rollback или явно объясняет отсутствие rollback.
- Для операции над несколькими записями транзакцией владеет application use-case. Передавай transaction-capable abstraction вниз, не скрывай частичные commit.
- Интеграционные тесты используют реальный PostgreSQL container и применяют те же миграции, что production. У каждого теста должна быть изоляция данных; не полагайся на порядок тестов.
- Testcontainers проверяет совместимость SQL и миграций, но не заменяет нагрузочные, backup/restore и production-network проверки.
