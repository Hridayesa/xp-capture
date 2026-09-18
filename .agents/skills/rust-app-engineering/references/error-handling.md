# Обработка ошибок Rust

За основу возьми идеи Jeremy Chone, но фиксируй архитектурные свойства, а не конкретную derive-библиотеку.

## Production code

- Определи конкретный `enum Error` на осмысленной границе crate/module и локальный `Result<T>` alias.
- Сохраняй причинную ошибку в variants и используй `?`/`From` для прозрачного распространения.
- Ожидаемые ошибки выражай отдельными variants. Не используй `panic`, `unwrap()` и `expect()` на данных запроса, I/O и иных штатно ошибочных путях.
- Не делай один глобальный enum со всеми ошибками системы. Конвертируй ошибку при пересечении архитектурной границы.
- `derive_more` из исходного подхода не обязателен: `thiserror` или ручные impl допустимы, если сохраняют нужные типы и source chain.

## Transport boundary

- Разделяй внутренний `Error` и безопасный `ClientError`/problem response.
- В одном transport mapper сопоставляй domain/application errors со стабильным error code, HTTP/Tauri status и публичным сообщением.
- Не сериализуй `Debug`, SQL, paths, secrets или source chain клиенту. Неизвестная ошибка получает общий internal code.
- Логируй внутреннюю ошибку один раз на границе с correlation/request id; не дублируй один failure на каждом уровне.

## Tests and prototypes

Для небольших тестов и spike допустим `Box<dyn std::error::Error + Send + Sync>`, если тесту не нужно сопоставлять варианты. Production API не должен оставаться на type-erased error только ради краткости.

Первичные материалы: https://rust10x.com/best-practices/error-handling и https://github.com/rust10x/rust-web-app/tree/main/crates/libs/lib-web/src.
