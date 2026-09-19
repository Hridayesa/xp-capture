<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";

import type { DeviceEndpointV1 } from "../api/camera";
import {
  DEFAULT_MODE_CANDIDATE_POLICY,
  ProfilePollingController,
  cancelProfile,
  getProfileResult,
  startProfile,
  type ProfileProgressV1,
  type ProfileReportV1,
} from "../api/profile";

const props = defineProps<{
  endpoint: DeviceEndpointV1 | null;
  scanBusy: boolean;
  resetToken: number;
}>();
const emit = defineEmits<{ busyChange: [busy: boolean] }>();

const starting = ref(false);
const cancelling = ref(false);
const progress = ref<ProfileProgressV1 | null>(null);
const report = ref<ProfileReportV1 | null>(null);
const errorMessage = ref<string | null>(null);
const selectedVerifiedModeId = ref<string | null>(null);
const polling = new ProfilePollingController();

const busy = computed(() => starting.value || cancelling.value || progress.value?.status === "profiling" || polling.isRunning());
const stuck = computed(() => progress.value?.status === "stuck" || progress.value?.service_state === "stuck");
const canStart = computed(() => props.endpoint !== null && !props.scanBusy && !busy.value && !stuck.value);
const candidateCount = computed(() => DEFAULT_MODE_CANDIDATE_POLICY.fourcc.length * DEFAULT_MODE_CANDIDATE_POLICY.resolutions.length * DEFAULT_MODE_CANDIDATE_POLICY.fps.length);
const statusText = computed(() => {
  if (stuck.value) return "Native read не завершился. Требуется перезапуск приложения.";
  if (cancelling.value) return "Ожидаем release текущего camera handle…";
  if (starting.value) return "Запускаем profiling…";
  if (!progress.value) return "Profiling ещё не запускался.";
  const current = progress.value.current_candidate;
  const tuple = current ? ` ${current.fourcc} ${current.width}×${current.height} @ ${current.requested_fps} FPS` : "";
  const phase = progress.value.current_phase ? `, phase ${progress.value.current_phase}` : "";
  return `Candidate ${Math.min(progress.value.completed_candidates + 1, progress.value.total_candidates)} of ${progress.value.total_candidates}${tuple}${phase}. Status: ${progress.value.status}.`;
});

watch(busy, (value) => emit("busyChange", value), { immediate: true });
watch(() => props.resetToken, reset);
watch(() => props.endpoint?.endpoint_key ?? null, (next, previous) => {
  if (previous !== null && next !== previous) reset();
});

async function begin(): Promise<void> {
  if (!canStart.value || !props.endpoint) return;
  starting.value = true;
  errorMessage.value = null;
  report.value = null;
  progress.value = null;
  selectedVerifiedModeId.value = null;
  try {
    const accepted = await startProfile(props.endpoint.endpoint_key);
    polling.start(accepted.profile_id, handleProgress, handleError);
  } catch (error: unknown) {
    handleError(error);
  } finally {
    starting.value = false;
  }
}

async function handleProgress(next: ProfileProgressV1): Promise<void> {
  progress.value = next;
  if (next.status === "completed" || next.status === "cancelled" || next.status === "failed" || next.status === "stuck") {
    try {
      report.value = await getProfileResult(next.profile_id);
    } catch (error: unknown) {
      handleError(error);
    }
  }
}

async function cancel(): Promise<void> {
  if (!progress.value || progress.value.status !== "profiling" || cancelling.value) return;
  cancelling.value = true;
  errorMessage.value = null;
  try {
    const terminal = await cancelProfile(progress.value.profile_id);
    polling.stop();
    await handleProgress(terminal);
  } catch (error: unknown) {
    handleError(error);
  } finally {
    cancelling.value = false;
  }
}

function handleError(error: unknown): void {
  errorMessage.value = error instanceof Error ? error.message : "Не удалось выполнить profiling.";
}

function reset(): void {
  polling.stop();
  starting.value = false;
  cancelling.value = false;
  progress.value = null;
  report.value = null;
  errorMessage.value = null;
  selectedVerifiedModeId.value = null;
}

function formatMaximumOrdinals(ordinals: number[]): string {
  const currentReport = report.value;
  if (!currentReport) return "";
  return ordinals
    .map((ordinal) => `${ordinal}: ${currentReport.results[ordinal]?.requested.fourcc ?? "?"}`)
    .join(", ");
}

onBeforeUnmount(() => polling.stop());
</script>

<template>
  <section class="diagnostics profile-diagnostics" aria-labelledby="profile-title">
    <div class="section-heading">
      <div>
        <p class="eyebrow">Capture-only evidence</p>
        <h2 id="profile-title">Профилирование camera modes</h2>
        <p class="version">OpenCV проверяет конечный эмпирический список; это не полный driver-advertised mode list.</p>
      </div>
    </div>

    <fieldset :disabled="busy || scanBusy || stuck || endpoint === null">
      <legend>Применяемая policy schema v{{ DEFAULT_MODE_CANDIDATE_POLICY.schema_version }}</legend>
      <p>{{ candidateCount }} candidates · warm-up {{ DEFAULT_MODE_CANDIDATE_POLICY.warmup_ms }} ms · capture-only {{ DEFAULT_MODE_CANDIDATE_POLICY.capture_only_ms }} ms</p>
      <p>Threshold: {{ DEFAULT_MODE_CANDIDATE_POLICY.minimum_fps_ratio * 100 }}% requested FPS. Operation budget {{ DEFAULT_MODE_CANDIDATE_POLICY.operation_deadline_ms }} ms.</p>
    </fieldset>

    <p v-if="endpoint" class="version">Endpoint: {{ endpoint.display_name }} · generation {{ endpoint.scan_generation }}</p>
    <p v-else class="empty">Выберите endpoint завершённого текущего scan.</p>

    <div class="button-row">
      <button type="button" data-testid="profile-start" :disabled="!canStart" @click="begin">
        {{ starting ? "Запускаем…" : "Начать profiling" }}
      </button>
      <button v-if="busy" type="button" class="button-secondary" data-testid="profile-cancel" :disabled="cancelling" @click="cancel">
        {{ cancelling ? "Освобождаем…" : "Отменить profiling" }}
      </button>
    </div>

    <p class="loading" role="status" aria-live="polite">{{ statusText }}</p>
    <div v-if="errorMessage" class="error" role="alert"><strong>Profiling недоступен</strong><span>{{ errorMessage }}</span></div>
    <div v-if="stuck" class="restart-required" role="alert"><strong>Требуется перезапуск</strong><span>Новые scan/profile запрещены до restart process.</span></div>

    <template v-if="report">
      <div v-if="report.results.length === 0" class="empty" data-testid="profile-empty">Terminal report не содержит завершённых candidates.</div>
      <div v-else class="table-scroll">
        <table data-testid="profile-results">
          <caption>Ordered candidate results</caption>
          <thead><tr><th>Requested</th><th>Set</th><th>Reported</th><th>Actual / measured</th><th>Status / reasons</th><th>Verified</th></tr></thead>
          <tbody>
            <tr v-for="result in report.results" :key="result.ordinal">
              <td>{{ result.requested.fourcc }} · {{ result.requested.width }}×{{ result.requested.height }} · {{ result.requested.requested_fps }} FPS</td>
              <td>FourCC {{ result.set.fourcc }} · size {{ result.set.width && result.set.height }} · FPS {{ result.set.fps }}</td>
              <td>{{ result.reported.fourcc ?? "unavailable" }} · {{ result.reported.width ?? "?" }}×{{ result.reported.height ?? "?" }} · {{ result.reported.fps ?? "?" }} FPS</td>
              <td>{{ result.metrics.actual_resolutions[0]?.width ?? "?" }}×{{ result.metrics.actual_resolutions[0]?.height ?? "?" }} · {{ result.metrics.measured_fps.toFixed(2) }} measured FPS</td>
              <td>{{ result.capture_mode_status }}<small v-if="result.failure_reasons.length">{{ result.failure_reasons.join(", ") }}</small></td>
              <td><label v-if="result.verified_mode_id"><input v-model="selectedVerifiedModeId" type="radio" name="verified-mode" :value="result.verified_mode_id" />capture-verified</label><span v-else>not selectable</span></td>
            </tr>
          </tbody>
        </table>
      </div>

      <div v-if="report.max_verified_by_resolution.length === 0" class="empty">Нет capture-verified modes.</div>
      <table v-else data-testid="profile-maxima">
        <caption>Maximum measured FPS by requested resolution (все ties)</caption>
        <thead><tr><th>Resolution</th><th>Maximum measured FPS</th><th>Result ordinals / FourCC</th><th>Backend</th></tr></thead>
        <tbody><tr v-for="maximum in report.max_verified_by_resolution" :key="`${maximum.resolution.width}x${maximum.resolution.height}`"><td>{{ maximum.resolution.width }}×{{ maximum.resolution.height }}</td><td>{{ maximum.measured_fps.toFixed(2) }}</td><td>{{ formatMaximumOrdinals(maximum.result_ordinals) }}</td><td>{{ report.backend }}</td></tr></tbody>
      </table>
      <p class="version">Reported FourCC match — OpenCV diagnostic, не доказательство native media subtype. Runtime/external validation: not_run.</p>
    </template>
  </section>
</template>
