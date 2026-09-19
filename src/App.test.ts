import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  cancelDeviceScan,
  startDeviceScan,
  type DeviceScanSnapshotV1,
} from "./api/camera";
import { SelfCheckClientError, runSelfCheck } from "./api/self-check";
import App from "./App.vue";

const pollingHarness = vi.hoisted(() => ({
  running: false,
  operationId: "",
  onSnapshot: null as ((snapshot: unknown) => void) | null,
  onError: null as ((error: unknown) => void) | null,
  start: vi.fn(),
  stop: vi.fn(),
}));

vi.mock("./api/camera", async (importOriginal) => {
  const original = await importOriginal<typeof import("./api/camera")>();
  return {
    ...original,
    startDeviceScan: vi.fn(),
    cancelDeviceScan: vi.fn(),
    CameraPollingController: class {
      start(
        operationId: string,
        onSnapshot: (snapshot: unknown) => void,
        onError: (error: unknown) => void,
      ): void {
        pollingHarness.running = true;
        pollingHarness.operationId = operationId;
        pollingHarness.onSnapshot = onSnapshot;
        pollingHarness.onError = onError;
        pollingHarness.start(operationId);
      }

      stop(): void {
        pollingHarness.running = false;
        pollingHarness.stop();
      }

      isRunning(): boolean {
        return pollingHarness.running;
      }
    },
  };
});

vi.mock("./api/self-check", async (importOriginal) => {
  const original = await importOriginal<typeof import("./api/self-check")>();
  return { ...original, runSelfCheck: vi.fn() };
});

const mockedRunSelfCheck = vi.mocked(runSelfCheck);
const mockedStartDeviceScan = vi.mocked(startDeviceScan);
const mockedCancelDeviceScan = vi.mocked(cancelDeviceScan);
const successfulReport = {
  schema_version: 1,
  checks: [
    { check: "opencv_load" as const, status: "passed" as const },
    { check: "image_codec" as const, status: "passed" as const },
    { check: "writer_open" as const, status: "passed" as const },
    { check: "writer_backend" as const, status: "passed" as const },
    { check: "writer_roundtrip" as const, status: "passed" as const },
  ],
  opencv_version: "4.12.0",
};

const policy = {
  first_index: 0,
  last_index: 1,
  backends: ["MSMF", "DSHOW"] as const,
  first_frame_deadline_ms: 5_000,
  operation_deadline_ms: 90_000,
  shutdown_deadline_ms: 3_000,
  reopen_delay_ms: 500,
};

const scanningSnapshot: DeviceScanSnapshotV1 = {
  schema_version: 1,
  operation_id: "scan-opaque",
  scan_generation: 1,
  policy: { ...policy, backends: [...policy.backends] },
  service_state: "scanning",
  status: "scanning",
  completed_probes: 1,
  total_probes: 4,
  current_probe: { backend: "MSMF", numeric_index: 1 },
  outcomes: [{ backend: "MSMF", numeric_index: 0, status: "open_failed" }],
  endpoints: [],
  failure_code: null,
};

function emitCameraSnapshot(snapshot: DeviceScanSnapshotV1): void {
  if (snapshot.status !== "scanning") {
    pollingHarness.running = false;
  }
  pollingHarness.onSnapshot?.(snapshot);
}

beforeEach(() => {
  mockedRunSelfCheck.mockReset();
  mockedStartDeviceScan.mockReset();
  mockedCancelDeviceScan.mockReset();
  pollingHarness.running = false;
  pollingHarness.operationId = "";
  pollingHarness.onSnapshot = null;
  pollingHarness.onError = null;
  pollingHarness.start.mockReset();
  pollingHarness.stop.mockReset();
  mockedStartDeviceScan.mockResolvedValue({
    schema_version: 1,
    operation_id: "scan-opaque",
    scan_generation: 1,
    policy: { ...policy, backends: [...policy.backends] },
  });
});

describe("App", () => {
  it("shows an idle camera-independent diagnostic screen", () => {
    const wrapper = mount(App);

    expect(wrapper.get("h1").text()).toBe("XP Capture");
    expect(wrapper.text()).toContain("без обращения к камере");
    expect(wrapper.get('[data-testid="self-check-start"]').text()).toBe("Запустить");
    expect(wrapper.get('[data-testid="camera-start"]').text()).toBe("Начать scan");
    expect(wrapper.text()).toContain("Scan ещё не запускался");
    expect(mockedStartDeviceScan).not.toHaveBeenCalled();
    expect(mockedCancelDeviceScan).not.toHaveBeenCalled();
  });

  it("shows a responsive loading state while native work is pending", async () => {
    mockedRunSelfCheck.mockReturnValue(new Promise(() => undefined));
    const wrapper = mount(App);

    await wrapper.get('[data-testid="self-check-start"]').trigger("click");

    expect(wrapper.findAll('[role="status"]')[0]?.text()).toContain(
      "Интерфейс остаётся доступным",
    );
    expect(
      wrapper.get('[data-testid="self-check-start"]').attributes("disabled"),
    ).toBeDefined();
  });

  it("renders every successful check separately", async () => {
    mockedRunSelfCheck.mockResolvedValue(successfulReport);
    const wrapper = mount(App);

    await wrapper.get('[data-testid="self-check-start"]').trigger("click");
    await flushPromises();

    expect(wrapper.findAll(".check-list li")).toHaveLength(5);
    expect(wrapper.text()).toContain("OpenCV 4.12.0");
    expect(wrapper.findAll(".status-passed")).toHaveLength(5);
  });

  it("shows a safe native error and supports retry", async () => {
    mockedRunSelfCheck
      .mockRejectedValueOnce(
        new SelfCheckClientError("internal_error", "Внутренняя ошибка самопроверки."),
      )
      .mockResolvedValueOnce(successfulReport);
    const wrapper = mount(App);

    await wrapper.get('[data-testid="self-check-start"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain("Внутренняя ошибка");
    expect(wrapper.get('[data-testid="self-check-start"]').text()).toBe("Повторить");

    await wrapper.get('[data-testid="self-check-start"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.findAll(".check-list li")).toHaveLength(5);
    expect(mockedRunSelfCheck).toHaveBeenCalledTimes(2);
    expect(mockedStartDeviceScan).not.toHaveBeenCalled();
  });

  it("starts only on explicit action and shows progress with disabled concurrent start", async () => {
    const wrapper = mount(App);

    await wrapper.get(".scan-form").trigger("submit");
    await flushPromises();
    expect(mockedStartDeviceScan).toHaveBeenCalledOnce();
    expect(pollingHarness.start).toHaveBeenCalledWith("scan-opaque");

    emitCameraSnapshot(scanningSnapshot);
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("Проверено 1 из 4");
    expect(wrapper.text()).toContain("MSMF / index 1");
    expect(wrapper.get('[data-testid="camera-start"]').attributes("disabled")).toBeDefined();
    expect(wrapper.get('[data-testid="camera-cancel"]').text()).toBe("Отменить");
  });

  it("renders a managed no-camera result with ordered rejections", async () => {
    const wrapper = mount(App);
    await wrapper.get(".scan-form").trigger("submit");
    await flushPromises();
    emitCameraSnapshot({
      ...scanningSnapshot,
      service_state: "idle",
      status: "completed",
      completed_probes: 2,
      total_probes: 2,
      current_probe: null,
      outcomes: [
        { backend: "MSMF", numeric_index: 0, status: "open_failed" },
        { backend: "DSHOW", numeric_index: 0, status: "first_frame_timeout" },
      ],
    });
    await wrapper.vm.$nextTick();

    expect(wrapper.get('[data-testid="camera-empty"]').text()).toContain(
      "Камеры не найдены",
    );
    const outcomes = wrapper.findAll(".outcome-list li");
    expect(outcomes).toHaveLength(2);
    expect(outcomes[0]?.text()).toContain("MSMF");
    expect(outcomes[1]?.text()).toContain("DSHOW");
  });

  it("keeps MSMF and DSHOW endpoints independent and clears selection on accepted rescan", async () => {
    const wrapper = mount(App);
    await wrapper.get(".scan-form").trigger("submit");
    await flushPromises();
    emitCameraSnapshot({
      ...scanningSnapshot,
      service_state: "idle",
      status: "completed",
      completed_probes: 2,
      total_probes: 2,
      current_probe: null,
      outcomes: [
        { backend: "MSMF", numeric_index: 0, status: "available" },
        { backend: "DSHOW", numeric_index: 0, status: "available" },
      ],
      endpoints: [
        {
          endpoint_key: "endpoint-msmf",
          scan_generation: 1,
          backend: "MSMF",
          numeric_index: 0,
          display_name: "Camera 0 / MSMF",
        },
        {
          endpoint_key: "endpoint-dshow",
          scan_generation: 1,
          backend: "DSHOW",
          numeric_index: 0,
          display_name: "Camera 0 / DSHOW",
        },
      ],
    });
    await wrapper.vm.$nextTick();

    const radios = wrapper.findAll('input[name="camera-endpoint"]');
    expect(radios).toHaveLength(2);
    await radios[0]?.setValue(true);
    expect((radios[0]?.element as HTMLInputElement).checked).toBe(true);

    mockedStartDeviceScan.mockResolvedValueOnce({
      schema_version: 1,
      operation_id: "scan-next",
      scan_generation: 2,
      policy: { ...policy, backends: [...policy.backends] },
    });
    await wrapper.get(".scan-form").trigger("submit");
    await flushPromises();
    expect(wrapper.findAll('input[name="camera-endpoint"]')).toHaveLength(0);
  });

  it("cancels through the service and displays confirmed terminal state", async () => {
    mockedCancelDeviceScan.mockResolvedValue({
      ...scanningSnapshot,
      service_state: "idle",
      status: "cancelled",
      current_probe: null,
      failure_code: "CANCELLED",
    });
    const wrapper = mount(App);
    await wrapper.get(".scan-form").trigger("submit");
    await flushPromises();
    emitCameraSnapshot(scanningSnapshot);
    await wrapper.vm.$nextTick();

    await wrapper.get('[data-testid="camera-cancel"]').trigger("click");
    await flushPromises();
    expect(mockedCancelDeviceScan).toHaveBeenCalledWith("scan-opaque");
    expect(wrapper.text()).toContain("подтверждённого освобождения");
  });

  it("shows sticky restart-required state and does not expose raw errors", async () => {
    const wrapper = mount(App);
    await wrapper.get(".scan-form").trigger("submit");
    await flushPromises();
    emitCameraSnapshot({
      ...scanningSnapshot,
      service_state: "stuck",
      status: "stuck",
      current_probe: null,
      failure_code: "READ_STALLED",
    });
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("Требуется перезапуск");
    expect(wrapper.get('[data-testid="camera-start"]').attributes("disabled")).toBeDefined();

    const fresh = mount(App);
    mockedStartDeviceScan.mockRejectedValueOnce(new Error("NATIVE_MARKER C:\\private"));
    await fresh.get(".scan-form").trigger("submit");
    await flushPromises();
    expect(fresh.text()).toContain("Не удалось выполнить camera scan");
    expect(fresh.text()).not.toContain("NATIVE_MARKER");
  });

  it("keeps camera commands untouched by mount, self-check, and self-check retry", async () => {
    mockedRunSelfCheck
      .mockRejectedValueOnce(
        new SelfCheckClientError("internal_error", "Внутренняя ошибка самопроверки."),
      )
      .mockResolvedValueOnce(successfulReport);
    const wrapper = mount(App);

    await wrapper.get('[data-testid="self-check-start"]').trigger("click");
    await flushPromises();
    await wrapper.get('[data-testid="self-check-start"]').trigger("click");
    await flushPromises();

    expect(mockedStartDeviceScan).not.toHaveBeenCalled();
    expect(mockedCancelDeviceScan).not.toHaveBeenCalled();
    expect(pollingHarness.start).not.toHaveBeenCalled();
  });
});
