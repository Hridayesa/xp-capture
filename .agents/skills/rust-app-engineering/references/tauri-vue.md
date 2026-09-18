# Tauri и Vue

- Считай Tauri commands внешней границей приложения: валидируй input и возвращай versioned serializable DTO, а не внутренние domain/persistence types.
- Долгие операции не блокируют UI thread. Явно моделируй progress, cancellation и повторный запуск.
- Минимизируй Tauri capabilities/permissions и CSP; не расширяй allowlist ради удобства разработки.
- TypeScript слой централизует вызовы commands и преобразование transport errors; Vue components не знают формат внутренних Rust errors.
- Server state и локальное UI state имеют разных владельцев. Не дублируй authoritative state без стратегии синхронизации.
- Компоненты shadcn-vue рассматривай как код проекта: сохраняй локальные изменения и проверяй accessibility/keyboard behavior после генерации.
- PostgreSQL для desktop требует отдельного эксплуатационного решения: сеть, credentials, offline behavior и migrations. Не добавляй его автоматически, если приложению подходит локальное хранилище.
