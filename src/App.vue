<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";

import {
  CameraClientError,
  CameraPollingController,
  DEFAULT_DEVICE_SCAN_REQUEST,
  cancelDeviceScan,
  startDeviceScan,
  type CaptureBackend,
  type DeviceScanSnapshotV1,
} from "./api/camera";
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

const firstIndex = ref(DEFAULT_DEVICE_SCAN_REQUEST.first_index);
const lastIndex = ref(DEFAULT_DEVICE_SCAN_REQUEST.last_index);
const selectedBackends = ref<CaptureBackend[]>([
  ...DEFAULT_DEVICE_SCAN_REQUEST.backends,
]);
const cameraStarting = ref(false);
const cameraCancelling = ref(false);
const cameraSnapshot = ref<DeviceScanSnapshotV1 | null>(null);
const cameraError = ref<string | null>(null);
const operationId = ref<string | null>(null);
const selectedEndpointKey = ref<string | null>(null);
const polling = new CameraPollingController();

const cameraScanning = computed(
  () => cameraSnapshot.value?.status === "scanning" || polling.isRunning(),
);
const cameraStuck = computed(
  () =>
    cameraSnapshot.value?.service_state === "stuck" ||
    cameraSnapshot.value?.status === "stuck",
);
const canStartCameraScan = computed(
  () =>
    !cameraStarting.value &&
    !cameraCancelling.value &&
    !cameraScanning.value &&
    !cameraStuck.value &&
    selectedBackends.value.length > 0 &&
    firstIndex.value >= 0 &&
    lastIndex.value >= firstIndex.value &&
    lastIndex.value - firstIndex.value < 32,
);
const cameraStatusText = computed(() => {
  if (cameraStuck.value) {
    return "Camera driver не ответил. Для продолжения перезапустите приложение.";
  }
  if (cameraCancelling.value) {
    return "Ожидаем освобождения camera handle…";
  }
  if (cameraStarting.value) {
    return "Запускаем ограниченный scan…";
  }
  const snapshot = cameraSnapshot.value;
  if (!snapshot) {
    return "Scan ещё не запускался.";
  }
  if (snapshot.status === "scanning") {
    const target = snapshot.current_probe;
    const current = target
      ? ` Сейчас: ${target.backend} / index ${target.numeric_index}.`
      : "";
    return `Проверено ${snapshot.completed_probes} из ${snapshot.total_probes}.${current}`;
  }
  if (snapshot.status === "completed") {
    return `Scan завершён: найдено ${snapshot.endpoints.length} endpoint.`;
  }
  if (snapshot.status === "cancelled") {
    return "Scan отменён после подтверждённого освобождения camera handle.";
  }
  return "Camera scan завершился безопасной ошибкой.";
});

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

async function startCameraScan(): Promise<void> {
  if (!canStartCameraScan.value) {
    return;
  }
  cameraStarting.value = true;
  cameraError.value = null;
  try {
    const started = await startDeviceScan({
      ...DEFAULT_DEVICE_SCAN_REQUEST,
      first_index: firstIndex.value,
      last_index: lastIndex.value,
      backends: [...selectedBackends.value],
    });
    selectedEndpointKey.value = null;
    operationId.value = started.operation_id;
    cameraSnapshot.value = null;
    polling.start(
      started.operation_id,
      (snapshot) => {
        if (
          cameraSnapshot.value &&
          cameraSnapshot.value.scan_generation !== snapshot.scan_generation
        ) {
          selectedEndpointKey.value = null;
        }
        cameraSnapshot.value = snapshot;
      },
      (cameraClientError) => {
        cameraError.value = cameraClientError.message;
      },
    );
  } catch (cameraClientError: unknown) {
    cameraError.value = safeCameraMessage(cameraClientError);
  } finally {
    cameraStarting.value = false;
  }
}

async function cancelCameraScan(): Promise<void> {
  if (!operationId.value || !cameraScanning.value || cameraCancelling.value) {
    return;
  }
  cameraCancelling.value = true;
  cameraError.value = null;
  try {
    const snapshot = await cancelDeviceScan(operationId.value);
    polling.stop();
    cameraSnapshot.value = snapshot;
  } catch (cameraClientError: unknown) {
    cameraError.value = safeCameraMessage(cameraClientError);
  } finally {
    cameraCancelling.value = false;
  }
}

function safeCameraMessage(error: unknown): string {
  return error instanceof CameraClientError
    ? error.message
    : "Не удалось выполнить camera scan.";
}

onBeforeUnmount(() => {
  polling.stop();
});
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
        <button
          type="button"
          data-testid="self-check-start"
          :disabled="loading"
          @click="startSelfCheck"
        >
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

    <section class="diagnostics camera-diagnostics" aria-labelledby="camera-title">
      <div class="section-heading">
        <div>
          <p class="eyebrow">Explicit device access</p>
          <h2 id="camera-title">Поиск camera endpoints</h2>
          <p class="version">
            Numeric index эмпирический и не является стабильным device ID.
          </p>
        </div>
      </div>

      <form class="scan-form" @submit.prevent="startCameraScan">
        <div class="range-fields">
          <label>
            Первый index
            <input
              v-model.number="firstIndex"
              data-testid="camera-first-index"
              type="number"
              min="0"
              max="31"
              :disabled="cameraScanning || cameraStarting || cameraStuck"
            />
          </label>
          <label>
            Последний index
            <input
              v-model.number="lastIndex"
              data-testid="camera-last-index"
              type="number"
              min="0"
              max="31"
              :disabled="cameraScanning || cameraStarting || cameraStuck"
            />
          </label>
        </div>

        <fieldset :disabled="cameraScanning || cameraStarting || cameraStuck">
          <legend>Backends и порядок scan</legend>
          <label>
            <input v-model="selectedBackends" type="checkbox" value="MSMF" />
            MSMF
          </label>
          <label>
            <input v-model="selectedBackends" type="checkbox" value="DSHOW" />
            DSHOW
          </label>
        </fieldset>

        <div class="button-row">
          <button
            type="submit"
            data-testid="camera-start"
            :disabled="!canStartCameraScan"
          >
            {{ cameraStarting ? "Запускаем…" : "Начать scan" }}
          </button>
          <button
            v-if="cameraScanning"
            type="button"
            class="button-secondary"
            data-testid="camera-cancel"
            :disabled="cameraCancelling"
            @click="cancelCameraScan"
          >
            {{ cameraCancelling ? "Освобождаем…" : "Отменить" }}
          </button>
        </div>
      </form>

      <p class="loading" role="status" aria-live="polite">
        {{ cameraStatusText }}
      </p>

      <div v-if="cameraError" class="error" role="alert">
        <strong>Camera scan недоступен</strong>
        <span>{{ cameraError }}</span>
      </div>

      <div v-if="cameraStuck" class="restart-required" role="alert">
        <strong>Требуется перезапуск</strong>
        <span>
          Native camera call не завершился к shutdown deadline. Новый scan в этом
          process запрещён.
        </span>
      </div>

      <template v-if="cameraSnapshot && cameraSnapshot.status !== 'scanning'">
        <div
          v-if="
            cameraSnapshot.status === 'completed' &&
            cameraSnapshot.endpoints.length === 0
          "
          class="empty"
          data-testid="camera-empty"
        >
          Камеры не найдены. Probe outcomes сохранены ниже; runtime self-check остаётся
          доступным.
        </div>

        <fieldset
          v-if="cameraSnapshot.endpoints.length > 0"
          class="endpoint-list"
        >
          <legend>Доступные endpoints текущего scan</legend>
          <label
            v-for="endpoint in cameraSnapshot.endpoints"
            :key="endpoint.endpoint_key"
          >
            <input
              v-model="selectedEndpointKey"
              type="radio"
              name="camera-endpoint"
              :value="endpoint.endpoint_key"
            />
            <span>
              <strong>{{ endpoint.display_name }}</strong>
              <small>generation {{ endpoint.scan_generation }}</small>
            </span>
          </label>
        </fieldset>

        <ol
          v-if="cameraSnapshot.outcomes.length > 0"
          class="outcome-list"
          aria-label="Результаты camera probes"
        >
          <li
            v-for="outcome in cameraSnapshot.outcomes"
            :key="`${outcome.backend}-${outcome.numeric_index}`"
          >
            <span>{{ outcome.backend }} / index {{ outcome.numeric_index }}</span>
            <strong>{{ outcome.status }}</strong>
          </li>
        </ol>
      </template>
    </section>
  </main>
</template>
