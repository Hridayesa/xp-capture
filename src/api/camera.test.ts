import { afterEach, describe, expect, it, vi } from "vitest";

import {
  CameraClientError,
  CameraPollingController,
  DEFAULT_DEVICE_SCAN_REQUEST,
  decodeCameraPublicError,
  decodeDeviceScanSnapshot,
  decodeDeviceScanStarted,
  getDeviceScan,
  startDeviceScan,
  type DeviceScanSnapshotV1,
} from "./camera";

const policy = {
  first_index: 0,
  last_index: 0,
  backends: ["MSMF"],
  first_frame_deadline_ms: 5_000,
  operation_deadline_ms: 90_000,
  shutdown_deadline_ms: 3_000,
  reopen_delay_ms: 500,
};

const scanningSnapshot: DeviceScanSnapshotV1 = {
  schema_version: 1,
  operation_id: "scan-opaque",
  scan_generation: 1,
  policy: { ...policy, backends: ["MSMF"] },
  service_state: "scanning",
  status: "scanning",
  completed_probes: 0,
  total_probes: 1,
  current_probe: { backend: "MSMF", numeric_index: 0 },
  outcomes: [],
  endpoints: [],
  failure_code: null,
};

afterEach(() => {
  vi.useRealTimers();
});

describe("camera API", () => {
  it("decodes supported start and snapshot DTOs", () => {
    expect(
      decodeDeviceScanStarted({
        schema_version: 1,
        operation_id: "scan-opaque",
        scan_generation: 1,
        policy,
      }),
    ).toMatchObject({ operation_id: "scan-opaque", scan_generation: 1 });
    expect(decodeDeviceScanSnapshot(scanningSnapshot)).toEqual(scanningSnapshot);
  });

  it("rejects unknown schema and malformed or extended DTOs", () => {
    expect(() =>
      decodeDeviceScanSnapshot({ ...scanningSnapshot, schema_version: 2 }),
    ).toThrowError(expect.objectContaining({ code: "UNSUPPORTED_SCHEMA" }));
    expect(() =>
      decodeDeviceScanSnapshot({ ...scanningSnapshot, completed_probes: 2 }),
    ).toThrowError(expect.objectContaining({ code: "TRANSPORT_ERROR" }));
    expect(() =>
      decodeDeviceScanSnapshot({ ...scanningSnapshot, native_path: "C:\\private" }),
    ).toThrowError(expect.objectContaining({ code: "TRANSPORT_ERROR" }));
  });

  it("maps unknown public errors without exposing native text", () => {
    const error = decodeCameraPublicError({
      schema_version: 1,
      code: "FUTURE_NATIVE_ERROR",
      message: "C:\\private\\NATIVE_MARKER.dll",
    });
    expect(error.code).toBe("UNKNOWN_ERROR_CODE");
    expect(error.message).not.toContain("NATIVE_MARKER");
  });

  it("uses one centralized versioned command contract", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        schema_version: 1,
        operation_id: "scan-opaque",
        scan_generation: 1,
        policy,
      })
      .mockResolvedValueOnce(scanningSnapshot);

    await startDeviceScan(DEFAULT_DEVICE_SCAN_REQUEST, invoke);
    await getDeviceScan("scan-opaque", invoke);

    expect(invoke).toHaveBeenNthCalledWith(1, "start_device_scan", {
      requestV1: DEFAULT_DEVICE_SCAN_REQUEST,
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "get_device_scan", {
      requestV1: { schema_version: 1, operation_id: "scan-opaque" },
    });
  });

  it("never overlaps poll requests and stops at every terminal state", async () => {
    vi.useFakeTimers();
    for (const terminal of ["completed", "cancelled", "failed", "stuck"] as const) {
      let resolveFirst: ((value: DeviceScanSnapshotV1) => void) | undefined;
      const getSnapshot = vi.fn(
        () =>
          new Promise<DeviceScanSnapshotV1>((resolve) => {
            resolveFirst = resolve;
          }),
      );
      const controller = new CameraPollingController(getSnapshot, 10);
      const onSnapshot = vi.fn();
      const onError = vi.fn();
      controller.start("scan-opaque", onSnapshot, onError);

      await vi.advanceTimersByTimeAsync(100);
      expect(getSnapshot).toHaveBeenCalledTimes(1);
      resolveFirst?.({
        ...scanningSnapshot,
        service_state: terminal === "stuck" ? "stuck" : "idle",
        status: terminal,
      });
      await Promise.resolve();
      await vi.advanceTimersByTimeAsync(100);

      expect(onSnapshot).toHaveBeenCalledOnce();
      expect(onError).not.toHaveBeenCalled();
      expect(getSnapshot).toHaveBeenCalledTimes(1);
      expect(controller.isRunning()).toBe(false);
    }
  });

  it("stops polling on a controlled client error", async () => {
    const controller = new CameraPollingController(async () => {
      throw new CameraClientError("INTERNAL", "Безопасная ошибка.");
    });
    const onError = vi.fn();
    controller.start("scan-opaque", vi.fn(), onError);
    await Promise.resolve();
    await Promise.resolve();
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ code: "INTERNAL" }),
    );
    expect(controller.isRunning()).toBe(false);
  });
});
