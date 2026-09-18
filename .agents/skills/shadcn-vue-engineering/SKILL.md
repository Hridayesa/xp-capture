---
name: shadcn-vue-engineering
description: Проектируй и изменяй shadcn-vue UI по фактической конфигурации проекта, актуальной документации и проверенному registry source. Применяй при явном упоминании shadcn-vue или после подтверждения существующего Vue/shadcn-vue проекта; не применяй только из-за Vue, Tailwind, UI или произвольного components.json и не применяй к React shadcn/ui либо другой UI-библиотеке.
---

# Инженерия shadcn-vue

Ответы и отчёты пиши на русском; API, команды и identifiers сохраняй в original spelling. Считай generated components локальным исходным кодом проекта, а не неизменяемой vendor-копией.

## Подтверди применимость и контекст

Если пользователь явно просит инициализировать shadcn-vue, получи актуальную документацию, но оформляй саму инициализацию как отдельное изменение через `$sdd-change`.

Во всех остальных случаях продолжай этот workflow только когда одновременно:

1. найден `components.json`;
2. проект использует Vue/Nuxt либо целевой UI directory содержит `.vue` components;
3. команда context успешно возвращает применимую конфигурацию:

```powershell
bun run tools/shadcn-vue.ts info --json
```

До проектирования или изменения прочитай `.sdd/stack.md`, `package.json`, lockfile и `components.json`, если они существуют. Зафиксируй исходный `git status --short`, не присваивая существующие изменения. Из context используй реальные `framework`, `tailwindVersion`, `tailwindCssFile`, aliases, `resolvedPaths`, `iconLibrary`, configured registries и installed components.

Если признаки не подтверждают Vue/shadcn-vue проект, прекрати применение skill. Отсутствие CLI/network или невалидный context обозначь как gap: не выдумывай API, component paths или конфигурацию.

## Получи документацию и выбери source

Для каждого затрагиваемого компонента запроси документацию и фактически открой полученные URL доступным web/fetch mechanism:

```powershell
bun run tools/shadcn-vue.ts docs button dialog
```

Проверяй сначала уже установленные компоненты, затем registry, и только потом создавай custom primitive. Для поиска и просмотра item используй registry-qualified имена:

```powershell
bun run tools/shadcn-vue.ts search @shadcn -q "dialog form"
bun run tools/shadcn-vue.ts view @shadcn/dialog
```

Built-in `@shadcn` допустим после `view`. Configured project registry допустим, когда его однозначно выбирают задача или локальные правила. Новый community/private registry автоматически не выбирай; при неоднозначности запроси выбор до mutation. Перед third-party item прочитай source, dependency declarations и target paths. Не выводи registry secrets из headers или environment в ответ, лог или git.

Сверяй version-sensitive composition с полученными docs. Если docs недоступны, явно отдели проверенное по local source от предположений. Учитывай фактические aliases, icon library и Tailwind version; предпочитай существующие variants и semantic design tokens. Не вводи raw colors, spacing conventions или новые tokens вопреки локальному design system. Для затронутого поведения проверь labels, titles, focus, keyboard behavior и error/loading/empty states.

## Безопасно внеси изменение

Перед добавлением определи точный registry-qualified item, выполни `view` и проверь files, dependencies, devDependencies, CSS/env mutations и target paths относительно запроса и исходного `git status`.

```powershell
bun run tools/shadcn-vue.ts add @shadcn/button
```

Не используй unsupported add preview flags. Не запускай CLI напрямую или через другой package runner. `--overwrite` допустим только после явного запроса пользователя и перечисления затрагиваемых файлов; молчание не является разрешением.

Для обновления существующего компонента standalone `diff` служит только вспомогательной read-only проверкой:

```powershell
bun run tools/shadcn-vue.ts diff button
```

Он не является historical base или three-way merge. Дополнительно прочитай local source и текущий `view` соответствующего registry item, объясни различия и примени узкий manual patch с сохранением local changes.

После `add` проверь полный diff, включая component files, CSS, Tailwind config, environment files, `package.json` и lockfile. Исправляй только относящиеся к задаче imports, aliases и icon usage.

Если mutation завершилась ненулевым exit code, CLI сообщил ошибку либо ожидаемые files отсутствуют, не считай проект неизменившимся: повтори `git status --short`, проверь component directories, CSS/Tailwind/env, manifests и lockfile, перечисли partial mutations и состояние quality gate. Не выполняй destructive rollback автоматически; предложи recoverable plan или запроси решение, если нужны удаление либо откат.

## Заверши проверкой

После успешной mutation прочитай все изменённые generated files. Проверь imports, aliases, missing subcomponents, dependency changes и относящиеся к задаче accessibility/keyboard states. Узкие проверки допустимы как промежуточные, затем запусти:

```powershell
bun run tools/verify.ts
```

Перед завершением выполни полный `$quality-gate`. Сообщи выполненные команды, результаты и оставшиеся gaps. Не обещай transaction rollback, smart merge, dry-run или bit-for-bit reproducibility.
