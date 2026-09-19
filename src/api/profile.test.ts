import { afterEach, describe, expect, it, vi } from "vitest";

import { CameraClientError } from "./camera";
import {
  DEFAULT_MODE_CANDIDATE_POLICY,
  ProfilePollingController,
  cancelProfile,
  decodeProfileProgress,
  decodeProfileReport,
  decodeProfileStarted,
  getProfileResult,
  getProfileStatus,
  startProfile,
  type CaptureModeStatus,
  type CandidatePhase,
  type ProfileOperationStatus,
  type ProfileServiceState,
} from "./profile";

const hash = "a".repeat(64);

function started() {
  return {
    schema_version: 1,
    profile_id: "profile-1",
    endpoint_key: "endpoint-1",
    scan_generation: 1,
    backend: "DSHOW",
    policy: DEFAULT_MODE_CANDIDATE_POLICY,
    config_hash: hash,
    total_candidates: 30,
  };
}

function progress(overrides: Record<string, unknown> = {}) {
  return {
    ...started(),
    service_state: "profiling",
    status: "profiling",
    completed_candidates: 0,
    current_candidate: {
      fourcc: "MJPG",
      width: 640,
      height: 480,
      requested_fps: 30,
    },
    current_phase: "opening",
    failure_code: null,
    ...overrides,
  };
}

function candidate(status: CaptureModeStatus) {
  return {
    ordinal: 0,
    attempt_count: 1,
    retry_reason: null,
    requested: { fourcc: "MJPG", width: 640, height: 480, requested_fps: 30 },
    set: { fourcc: true, width: true, height: true, fps: true },
    reported: { fourcc: "MJPG", width: 640, height: 480, fps: 30 },
    metrics: {
      elapsed_ms: 1000,
      read_attempts: 30,
      captured_frames: 30,
      empty_frames: 0,
      read_failures: 0,
      measured_fps: 30,
      median_interval_ms: 33,
      p95_interval_ms: 34,
      p99_interval_ms: 35,
      maximum_gap_ms: 35,
      long_gap_count: 0,
      long_gap_ratio: 0,
      actual_resolutions: [{ width: 640, height: 480 }],
    },
    capture_mode_status: status,
    failure_reasons: [],
    verified_mode_id: status.startsWith("verified_") ? "verified-1" : null,
  };
}

function report(status: CaptureModeStatus = "verified_fourcc_reported_match") {
  return {
    schema_version: 1,
    profile_id: "profile-1",
    started_at_unix_ms: 1,
    environment: {
      app_version: "0.1.0",
      rust_version: "1.98.1",
      tauri_version: "2.11.5",
      opencv_version: "4.12.0",
      opencv_crate_version: "0.100.1",
    },
    endpoint_key: "endpoint-1",
    scan_generation: 1,
    backend: "DSHOW",
    policy: DEFAULT_MODE_CANDIDATE_POLICY,
    config_hash: hash,
    status: "completed",
    failure_code: null,
    runtime_validation_status: "not_run",
    external_validation_status: "not_run",
    results: [candidate(status)],
    max_verified_by_resolution: status.startsWith("verified_")
      ? [{ resolution: { width: 640, height: 480 }, result_ordinals: [0], measured_fps: 30 }]
      : [],
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("profile API", () => {
  it("decodes every service, operation, phase, and candidate status", () => {
    const states: ProfileServiceState[] = ["idle", "scanning", "profiling", "profile_ready", "faulted", "stuck"];
    const statuses: ProfileOperationStatus[] = ["profiling", "completed", "cancelled", "failed", "stuck"];
    const phases: CandidatePhase[] = ["opening", "applying", "reported", "first_frame", "warmup", "measuring", "release", "reopen_delay"];
    const modes: CaptureModeStatus[] = ["opening_failed", "applying_failed", "read_failed", "release_failed", "first_frame_timeout", "read_stalled", "candidate_timed_out", "operation_timed_out", "coerced_resolution", "capture_under_target", "capture_unstable", "verified_fourcc_reported_match", "verified_fourcc_unconfirmed", "cancelled"];
    for (const service_state of states) expect(decodeProfileProgress(progress({ service_state })).service_state).toBe(service_state);
    for (const status of statuses) expect(decodeProfileProgress(progress({ status })).status).toBe(status);
    for (const current_phase of phases) expect(decodeProfileProgress(progress({ current_phase })).current_phase).toBe(current_phase);
    for (const mode of modes) expect(decodeProfileReport(report(mode)).results[0]?.capture_mode_status).toBe(mode);
  });

  it("rejects unknown schemas, fields, and enum values", () => {
    expect(() => decodeProfileStarted({ ...started(), schema_version: 2 })).toThrow(CameraClientError);
    expect(() => decodeProfileStarted({ ...started(), extra: true })).toThrow(CameraClientError);
    expect(() => decodeProfileProgress(progress({ current_phase: "native_subtype" }))).toThrow(CameraClientError);
    expect(() => decodeProfileReport({ ...report(), source: "C:\\private" })).toThrow(CameraClientError);
  });

  it("uses exact versioned command argument shapes", async () => {
    const invokeMock = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      void args;
      if (command === "start_profile") return started();
      if (command === "get_profile_result") return report();
      return progress({ status: "cancelled", service_state: "idle", current_candidate: null, current_phase: null });
    });
    const invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> =>
      invokeMock(command, args) as Promise<T>;
    await startProfile("endpoint-1", DEFAULT_MODE_CANDIDATE_POLICY, invoke);
    await getProfileStatus("profile-1", invoke);
    await getProfileResult("profile-1", invoke);
    await cancelProfile("profile-1", invoke);
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual(["start_profile", "get_profile_status", "get_profile_result", "cancel_profile"]);
    expect(invokeMock.mock.calls[0]?.[1]).toEqual({ requestV1: { schema_version: 1, endpoint_key: "endpoint-1", candidate_config: DEFAULT_MODE_CANDIDATE_POLICY } });
    expect(invokeMock.mock.calls[1]?.[1]).toEqual({ requestV1: { schema_version: 1, profile_id: "profile-1" } });
  });

  it("polls single-in-flight and stops on every terminal status", async () => {
    vi.useFakeTimers();
    for (const terminal of ["completed", "cancelled", "failed", "stuck"] as const) {
      let resolveFirst: ((value: ReturnType<typeof decodeProfileProgress>) => void) | undefined;
      const getter = vi.fn(() => new Promise<ReturnType<typeof decodeProfileProgress>>((resolve) => { resolveFirst = resolve; }));
      const controller = new ProfilePollingController(getter, 10);
      controller.start("profile-1", vi.fn(), vi.fn());
      await vi.advanceTimersByTimeAsync(100);
      expect(getter).toHaveBeenCalledTimes(1);
      resolveFirst?.(decodeProfileProgress(progress({ status: terminal, service_state: terminal === "stuck" ? "stuck" : "idle", current_candidate: null, current_phase: null })));
      await Promise.resolve();
      await vi.advanceTimersByTimeAsync(100);
      expect(getter).toHaveBeenCalledTimes(1);
      expect(controller.isRunning()).toBe(false);
    }
  });

  it("maps unknown public errors without exposing native text", async () => {
    const invoke = async <T>(): Promise<T> => Promise.reject({ schema_version: 1, code: "NATIVE_PRIVATE", message: "C:\\private\\marker" });
    await expect(startProfile("endpoint-1", DEFAULT_MODE_CANDIDATE_POLICY, invoke)).rejects.toMatchObject({ code: "UNKNOWN_ERROR_CODE" });
  });
});
