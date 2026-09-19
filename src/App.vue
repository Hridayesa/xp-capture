<script setup lang="ts">
import { ref } from "vue";

import {
  SelfCheckClientError,
  runSelfCheck,
  type CheckCode,
  type SelfCheckReport,
} from "./api/self-check";

const checkLabels: Record<CheckCode, string> = {
  opencv_load: "OpenCV runtime и backends",
  image_codec: "JPEG encode/decode",
  writer_open: "Открытие video writer",
  writer_backend: "Backend FFMPEG",
  writer_roundtrip: "MJPG/AVI round-trip",
};

const loading = ref(false);
const report = ref<SelfCheckReport | null>(null);
const errorMessage = ref<string | null>(null);

async function startSelfCheck(): Promise<void> {
  loading.value = true;
  report.value = null;
  errorMessage.value = null;
  try {
    report.value = await runSelfCheck();
  } catch (error: unknown) {
    errorMessage.value =
      error instanceof SelfCheckClientError
        ? error.message
        : "Не удалось запустить самопроверку.";
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <main>
    <header>
      <p class="eyebrow">Runtime diagnostics</p>
      <h1>XP Capture</h1>
      <p class="intro">
        Проверка OpenCV, JPEG и synthetic video pipeline без обращения к камере.
      </p>
    </header>

    <section class="diagnostics" aria-labelledby="diagnostics-title">
      <div class="section-heading">
        <div>
          <h2 id="diagnostics-title">Самопроверка runtime</h2>
          <p v-if="report?.opencv_version" class="version">
            OpenCV {{ report.opencv_version }}
          </p>
        </div>
        <button type="button" :disabled="loading" @click="startSelfCheck">
          {{ loading ? "Проверяем…" : report || errorMessage ? "Повторить" : "Запустить" }}
        </button>
      </div>

      <p v-if="loading" class="loading" role="status" aria-live="polite">
        Выполняется native self-check. Интерфейс остаётся доступным.
      </p>

      <div v-else-if="errorMessage" class="error" role="alert">
        <strong>Самопроверка не запущена</strong>
        <span>{{ errorMessage }}</span>
      </div>

      <ul v-else-if="report" class="check-list" aria-label="Результаты самопроверки">
        <li v-for="check in report.checks" :key="check.check">
          <div>
            <strong>{{ checkLabels[check.check] }}</strong>
            <p v-if="check.public_message">{{ check.public_message }}</p>
          </div>
          <span class="status" :class="`status-${check.status}`">
            {{ check.status }}
          </span>
        </li>
      </ul>

      <p v-else class="empty">
        Запустите проверку, чтобы увидеть отдельный статус каждого этапа.
      </p>
    </section>
  </main>
</template>
