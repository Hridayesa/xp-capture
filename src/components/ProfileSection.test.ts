import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { CameraClientError, type DeviceEndpointV1 } from "../api/camera";
import ProfileSection from "./ProfileSection.vue";

const mocks = vi.hoisted(() => ({
  startProfile: vi.fn(),
  cancelProfile: vi.fn(),
  getProfileResult: vi.fn(),
  onProgress: undefined as ((value: unknown) => void) | undefined,
  onError: undefined as ((value: Error) => void) | undefined,
}));

vi.mock("../api/profile", async (importOriginal) => {
  const original = await importOriginal<typeof import("../api/profile")>();
  return {
    ...original,
    startProfile: mocks.startProfile,
    cancelProfile: mocks.cancelProfile,
    getProfileResult: mocks.getProfileResult,
    ProfilePollingController: class {
      private running = false;
      start(_id: string, onProgress: (value: unknown) => void, onError: (value: Error) => void) {
        this.running = true;
        mocks.onProgress = onProgress;
        mocks.onError = onError;
      }
      stop() { this.running = false; }
      isRunning() { return this.running; }
    },
  };
});

const endpoint: DeviceEndpointV1 = {
  endpoint_key: "endpoint-1",
  scan_generation: 1,
  backend: "DSHOW",
  numeric_index: 0,
  display_name: "Camera 0 / DSHOW",
};

function progress(status = "profiling") {
  return {
    schema_version: 1,
    profile_id: "profile-1",
    endpoint_key: "endpoint-1",
    scan_generation: 1,
    backend: "DSHOW",
    policy: {},
    config_hash: "a".repeat(64),
    total_candidates: 1,
    service_state: status === "stuck" ? "stuck" : status === "profiling" ? "profiling" : "profile_ready",
    status,
    completed_candidates: status === "profiling" ? 0 : 1,
    current_candidate: status === "profiling" ? { fourcc: "MJPG", width: 640, height: 480, requested_fps: 30 } : null,
    current_phase: status === "profiling" ? "measuring" : null,
    failure_code: null,
  };
}

function terminalReport() {
  return {
    results: [{
      ordinal: 0,
      requested: { fourcc: "MJPG", width: 640, height: 480, requested_fps: 30 },
      set: { fourcc: false, width: true, height: true, fps: true },
      reported: { fourcc: "MJPG", width: 640, height: 480, fps: 30 },
      metrics: { measured_fps: 29.9, actual_resolutions: [{ width: 640, height: 480 }] },
      capture_mode_status: "verified_fourcc_reported_match",
      failure_reasons: [],
      verified_mode_id: "verified-1",
    }],
    max_verified_by_resolution: [{ resolution: { width: 640, height: 480 }, measured_fps: 29.9, result_ordinals: [0] }],
    backend: "DSHOW",
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.onProgress = undefined;
  mocks.onError = undefined;
  mocks.startProfile.mockResolvedValue({ profile_id: "profile-1" });
  mocks.cancelProfile.mockResolvedValue(progress("cancelled"));
  mocks.getProfileResult.mockResolvedValue(terminalReport());
});

describe("ProfileSection", () => {
  it("never starts on mount, scan, selection, or reset and blocks an empty selection", async () => {
    const wrapper = mount(ProfileSection, { props: { endpoint: null, scanBusy: false, resetToken: 0 } });
    expect(mocks.startProfile).not.toHaveBeenCalled();
    expect(wrapper.get("[data-testid='profile-start']").attributes("disabled")).toBeDefined();
    await wrapper.setProps({ endpoint, resetToken: 1 });
    expect(mocks.startProfile).not.toHaveBeenCalled();
    expect(wrapper.get("[data-testid='profile-start']").attributes("disabled")).toBeUndefined();
    await wrapper.setProps({ scanBusy: true });
    expect(wrapper.get("[data-testid='profile-start']").attributes("disabled")).toBeDefined();
  });

  it("starts explicitly, renders authoritative progress and all result labels/ties", async () => {
    const wrapper = mount(ProfileSection, { props: { endpoint, scanBusy: false, resetToken: 0 } });
    await wrapper.get("[data-testid='profile-start']").trigger("click");
    await flushPromises();
    expect(mocks.startProfile).toHaveBeenCalledTimes(1);
    expect(wrapper.find("[data-testid='profile-results']").exists()).toBe(false);
    await mocks.onProgress?.(progress("profiling"));
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("phase measuring");
    await mocks.onProgress?.(progress("completed"));
    await flushPromises();
    expect(wrapper.get("[data-testid='profile-results']").text()).toContain("Requested");
    expect(wrapper.get("[data-testid='profile-results']").text()).toContain("Reported");
    expect(wrapper.get("[data-testid='profile-results']").text()).toContain("measured FPS");
    expect(wrapper.get("[data-testid='profile-maxima']").text()).toContain("0: MJPG");
    expect(wrapper.text()).toContain("не доказательство native media subtype");
    await wrapper.setProps({ resetToken: 1 });
    expect(wrapper.find("[data-testid='profile-results']").exists()).toBe(false);
  });

  it("waits for confirmed cancellation and exposes sticky restart-required state", async () => {
    const wrapper = mount(ProfileSection, { props: { endpoint, scanBusy: false, resetToken: 0 } });
    await wrapper.get("[data-testid='profile-start']").trigger("click");
    await flushPromises();
    await mocks.onProgress?.(progress("profiling"));
    await wrapper.vm.$nextTick();
    await wrapper.get("[data-testid='profile-cancel']").trigger("click");
    await flushPromises();
    expect(mocks.cancelProfile).toHaveBeenCalledWith("profile-1");
    await mocks.onProgress?.(progress("stuck"));
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("Требуется перезапуск");
    expect(wrapper.get("[data-testid='profile-start']").attributes("disabled")).toBeDefined();
  });

  it("shows PROFILE_NOT_READY as a controlled public error", async () => {
    mocks.getProfileResult.mockRejectedValueOnce(
      new CameraClientError("PROFILE_NOT_READY", "Результат profiling ещё не готов."),
    );
    const wrapper = mount(ProfileSection, { props: { endpoint, scanBusy: false, resetToken: 0 } });
    await wrapper.get("[data-testid='profile-start']").trigger("click");
    await flushPromises();
    await mocks.onProgress?.(progress("completed"));
    await flushPromises();
    expect(wrapper.text()).toContain("Результат profiling ещё не готов.");
    expect(wrapper.find("[data-testid='profile-results']").exists()).toBe(false);
  });
});
