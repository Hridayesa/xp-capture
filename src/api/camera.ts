import { invoke } from "@tauri-apps/api/core";

export const CAMERA_TRANSPORT_SCHEMA_VERSION = 1;

export type CaptureBackend = "MSMF" | "DSHOW";
export type CameraServiceState = "idle" | "scanning" | "faulted" | "stuck";
export type DeviceScanStatus =
  | "scanning"
  | "completed"
  | "cancelled"
  | "failed"
  | "stuck";
export type ProbeStatus =
  | "available"
  | "open_failed"
  | "first_frame_timeout"
  | "read_failed"
  | "cancelled";

export type CameraPublicErrorCode =
  | "INVALID_CONFIG"
  | "BUSY"
  | "STALE_SCAN_OPERATION"
  | "OPEN_FAILED"
  | "READ_TIMEOUT"
  | "READ_STALLED"
  | "CANCELLED"
  | "INTERNAL";

export type CameraClientErrorCode =
  | CameraPublicErrorCode
  | "TRANSPORT_ERROR"
  | "UNSUPPORTED_SCHEMA"
  | "UNKNOWN_ERROR_CODE";

export interface DeviceScanRequestV1 {
  schema_version: 1;
  first_index: number;
  last_index: number;
  backends: CaptureBackend[];
  first_frame_deadline_ms: number;
  operation_deadline_ms: number;
  shutdown_deadline_ms: number;
  reopen_delay_ms: number;
}

export type DeviceScanPolicyV1 = Omit<DeviceScanRequestV1, "schema_version">;

export interface DeviceScanStartedV1 {
  schema_version: 1;
  operation_id: string;
  scan_generation: number;
  policy: DeviceScanPolicyV1;
}

export interface ProbeTargetV1 {
  backend: CaptureBackend;
  numeric_index: number;
}

export interface ProbeOutcomeV1 extends ProbeTargetV1 {
  status: ProbeStatus;
}

export interface DeviceEndpointV1 extends ProbeTargetV1 {
  endpoint_key: string;
  scan_generation: number;
  display_name: string;
}

export interface DeviceScanSnapshotV1 {
  schema_version: 1;
  operation_id: string;
  scan_generation: number;
  policy: DeviceScanPolicyV1;
  service_state: CameraServiceState;
  status: DeviceScanStatus;
  completed_probes: number;
  total_probes: number;
  current_probe: ProbeTargetV1 | null;
  outcomes: ProbeOutcomeV1[];
  endpoints: DeviceEndpointV1[];
  failure_code: CameraPublicErrorCode | null;
}

export interface CameraServiceSnapshotV1 {
  schema_version: 1;
  service_state: CameraServiceState;
  operation: DeviceScanSnapshotV1 | null;
}

export class CameraClientError extends Error {
  readonly code: CameraClientErrorCode;

  constructor(code: CameraClientErrorCode, message: string) {
    super(message);
    this.name = "CameraClientError";
    this.code = code;
  }
}

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type SnapshotGetter = (operationId: string) => Promise<DeviceScanSnapshotV1>;

const backends = new Set<CaptureBackend>(["MSMF", "DSHOW"]);
const serviceStates = new Set<CameraServiceState>([
  "idle",
  "scanning",
  "faulted",
  "stuck",
]);
const scanStatuses = new Set<DeviceScanStatus>([
  "scanning",
  "completed",
  "cancelled",
  "failed",
  "stuck",
]);
const probeStatuses = new Set<ProbeStatus>([
  "available",
  "open_failed",
  "first_frame_timeout",
  "read_failed",
  "cancelled",
]);
const publicErrorCodes = new Set<CameraPublicErrorCode>([
  "INVALID_CONFIG",
  "BUSY",
  "STALE_SCAN_OPERATION",
  "OPEN_FAILED",
  "READ_TIMEOUT",
  "READ_STALLED",
  "CANCELLED",
  "INTERNAL",
]);
const terminalStatuses = new Set<DeviceScanStatus>([
  "completed",
  "cancelled",
  "failed",
  "stuck",
]);

export const DEFAULT_DEVICE_SCAN_REQUEST: DeviceScanRequestV1 = {
  schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
  first_index: 0,
  last_index: 5,
  backends: ["MSMF", "DSHOW"],
  first_frame_deadline_ms: 5_000,
  operation_deadline_ms: 90_000,
  shutdown_deadline_ms: 3_000,
  reopen_delay_ms: 500,
};

export async function startDeviceScan(
  request: DeviceScanRequestV1,
  invokeCommand: Invoke = invoke,
): Promise<DeviceScanStartedV1> {
  try {
    return decodeDeviceScanStarted(
      await invokeCommand<unknown>("start_device_scan", { requestV1: request }),
    );
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export async function getDeviceScan(
  operationId: string,
  invokeCommand: Invoke = invoke,
): Promise<DeviceScanSnapshotV1> {
  try {
    return decodeDeviceScanSnapshot(
      await invokeCommand<unknown>("get_device_scan", {
        requestV1: operationRequest(operationId),
      }),
    );
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export async function cancelDeviceScan(
  operationId: string,
  invokeCommand: Invoke = invoke,
): Promise<DeviceScanSnapshotV1> {
  try {
    return decodeDeviceScanSnapshot(
      await invokeCommand<unknown>("cancel_device_scan", {
        requestV1: operationRequest(operationId),
      }),
    );
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export async function stopCamera(
  invokeCommand: Invoke = invoke,
): Promise<CameraServiceSnapshotV1> {
  try {
    return decodeCameraServiceSnapshot(
      await invokeCommand<unknown>("stop_camera"),
    );
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export function decodeDeviceScanStarted(value: unknown): DeviceScanStartedV1 {
  const record = asExactRecord(value, [
    "schema_version",
    "operation_id",
    "scan_generation",
    "policy",
  ]);
  requireSupportedSchema(record);
  if (
    typeof record.operation_id !== "string" ||
    record.operation_id.length === 0 ||
    !isNonNegativeInteger(record.scan_generation)
  ) {
    throw transportError();
  }
  return {
    schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
    operation_id: record.operation_id,
    scan_generation: record.scan_generation,
    policy: decodePolicy(record.policy),
  };
}

export function decodeDeviceScanSnapshot(value: unknown): DeviceScanSnapshotV1 {
  const record = asExactRecord(value, [
    "schema_version",
    "operation_id",
    "scan_generation",
    "policy",
    "service_state",
    "status",
    "completed_probes",
    "total_probes",
    "current_probe",
    "outcomes",
    "endpoints",
    "failure_code",
  ]);
  requireSupportedSchema(record);
  if (
    typeof record.operation_id !== "string" ||
    record.operation_id.length === 0 ||
    !isNonNegativeInteger(record.scan_generation) ||
    typeof record.service_state !== "string" ||
    !serviceStates.has(record.service_state as CameraServiceState) ||
    typeof record.status !== "string" ||
    !scanStatuses.has(record.status as DeviceScanStatus) ||
    !isNonNegativeInteger(record.completed_probes) ||
    !isNonNegativeInteger(record.total_probes) ||
    record.completed_probes > record.total_probes ||
    !Array.isArray(record.outcomes) ||
    !Array.isArray(record.endpoints)
  ) {
    throw transportError();
  }
  const failureCode = decodeOptionalErrorCode(record.failure_code);
  return {
    schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
    operation_id: record.operation_id,
    scan_generation: record.scan_generation,
    policy: decodePolicy(record.policy),
    service_state: record.service_state as CameraServiceState,
    status: record.status as DeviceScanStatus,
    completed_probes: record.completed_probes,
    total_probes: record.total_probes,
    current_probe:
      record.current_probe === null
        ? null
        : decodeProbeTarget(record.current_probe),
    outcomes: record.outcomes.map(decodeProbeOutcome),
    endpoints: record.endpoints.map(decodeEndpoint),
    failure_code: failureCode,
  };
}

export function decodeCameraPublicError(value: unknown): CameraClientError {
  try {
    const record = asExactRecord(value, ["schema_version", "code", "message"]);
    requireSupportedSchema(record);
    if (typeof record.code !== "string" || typeof record.message !== "string") {
      return transportError();
    }
    if (!publicErrorCodes.has(record.code as CameraPublicErrorCode)) {
      return new CameraClientError(
        "UNKNOWN_ERROR_CODE",
        "Приложение вернуло неизвестный код ошибки camera service.",
      );
    }
    return new CameraClientError(
      record.code as CameraPublicErrorCode,
      record.message,
    );
  } catch (error: unknown) {
    return error instanceof CameraClientError ? error : transportError();
  }
}

export class CameraPollingController {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private token = 0;
  private running = false;

  constructor(
    private readonly getSnapshot: SnapshotGetter = getDeviceScan,
    private readonly intervalMs = 250,
  ) {}

  start(
    operationId: string,
    onSnapshot: (snapshot: DeviceScanSnapshotV1) => void,
    onError: (error: CameraClientError) => void,
  ): void {
    this.stop();
    this.running = true;
    const token = this.token;
    void this.tick(token, operationId, onSnapshot, onError);
  }

  stop(): void {
    this.running = false;
    this.token += 1;
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  isRunning(): boolean {
    return this.running;
  }

  private async tick(
    token: number,
    operationId: string,
    onSnapshot: (snapshot: DeviceScanSnapshotV1) => void,
    onError: (error: CameraClientError) => void,
  ): Promise<void> {
    try {
      const snapshot = await this.getSnapshot(operationId);
      if (!this.running || token !== this.token) {
        return;
      }
      onSnapshot(snapshot);
      if (terminalStatuses.has(snapshot.status)) {
        this.stop();
        return;
      }
      this.timer = setTimeout(() => {
        this.timer = null;
        void this.tick(token, operationId, onSnapshot, onError);
      }, this.intervalMs);
    } catch (error: unknown) {
      if (!this.running || token !== this.token) {
        return;
      }
      this.stop();
      onError(normalizeError(error));
    }
  }
}

function operationRequest(operationId: string): Record<string, unknown> {
  return {
    schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
    operation_id: operationId,
  };
}

function decodePolicy(value: unknown): DeviceScanPolicyV1 {
  const record = asExactRecord(value, [
    "first_index",
    "last_index",
    "backends",
    "first_frame_deadline_ms",
    "operation_deadline_ms",
    "shutdown_deadline_ms",
    "reopen_delay_ms",
  ]);
  if (
    !isNonNegativeInteger(record.first_index) ||
    !isNonNegativeInteger(record.last_index) ||
    !Array.isArray(record.backends) ||
    record.backends.length === 0 ||
    !record.backends.every(
      (backend) => typeof backend === "string" && backends.has(backend as CaptureBackend),
    ) ||
    !isPositiveInteger(record.first_frame_deadline_ms) ||
    !isPositiveInteger(record.operation_deadline_ms) ||
    !isPositiveInteger(record.shutdown_deadline_ms) ||
    !isPositiveInteger(record.reopen_delay_ms)
  ) {
    throw transportError();
  }
  return {
    first_index: record.first_index,
    last_index: record.last_index,
    backends: record.backends as CaptureBackend[],
    first_frame_deadline_ms: record.first_frame_deadline_ms,
    operation_deadline_ms: record.operation_deadline_ms,
    shutdown_deadline_ms: record.shutdown_deadline_ms,
    reopen_delay_ms: record.reopen_delay_ms,
  };
}

function decodeProbeTarget(value: unknown): ProbeTargetV1 {
  const record = asExactRecord(value, ["backend", "numeric_index"]);
  if (
    typeof record.backend !== "string" ||
    !backends.has(record.backend as CaptureBackend) ||
    !isNonNegativeInteger(record.numeric_index)
  ) {
    throw transportError();
  }
  return {
    backend: record.backend as CaptureBackend,
    numeric_index: record.numeric_index,
  };
}

function decodeProbeOutcome(value: unknown): ProbeOutcomeV1 {
  const record = asExactRecord(value, ["backend", "numeric_index", "status"]);
  const target = decodeProbeTarget({
    backend: record.backend,
    numeric_index: record.numeric_index,
  });
  if (
    typeof record.status !== "string" ||
    !probeStatuses.has(record.status as ProbeStatus)
  ) {
    throw transportError();
  }
  return { ...target, status: record.status as ProbeStatus };
}

function decodeEndpoint(value: unknown): DeviceEndpointV1 {
  const record = asExactRecord(value, [
    "endpoint_key",
    "scan_generation",
    "backend",
    "numeric_index",
    "display_name",
  ]);
  const target = decodeProbeTarget({
    backend: record.backend,
    numeric_index: record.numeric_index,
  });
  if (
    typeof record.endpoint_key !== "string" ||
    record.endpoint_key.length === 0 ||
    !isNonNegativeInteger(record.scan_generation) ||
    typeof record.display_name !== "string"
  ) {
    throw transportError();
  }
  return {
    ...target,
    endpoint_key: record.endpoint_key,
    scan_generation: record.scan_generation,
    display_name: record.display_name,
  };
}

function decodeCameraServiceSnapshot(value: unknown): CameraServiceSnapshotV1 {
  const record = asExactRecord(value, [
    "schema_version",
    "service_state",
    "operation",
  ]);
  requireSupportedSchema(record);
  if (
    typeof record.service_state !== "string" ||
    !serviceStates.has(record.service_state as CameraServiceState)
  ) {
    throw transportError();
  }
  return {
    schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
    service_state: record.service_state as CameraServiceState,
    operation:
      record.operation === null
        ? null
        : decodeDeviceScanSnapshot(record.operation),
  };
}

function decodeOptionalErrorCode(value: unknown): CameraPublicErrorCode | null {
  if (value === null) {
    return null;
  }
  if (typeof value !== "string" || !publicErrorCodes.has(value as CameraPublicErrorCode)) {
    throw transportError();
  }
  return value as CameraPublicErrorCode;
}

function normalizeError(error: unknown): CameraClientError {
  return error instanceof CameraClientError
    ? error
    : decodeCameraPublicError(error);
}

function asExactRecord(
  value: unknown,
  expectedKeys: readonly string[],
): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw transportError();
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (
    keys.length !== expectedKeys.length ||
    expectedKeys.some((key) => !Object.hasOwn(record, key))
  ) {
    throw transportError();
  }
  return record;
}

function requireSupportedSchema(record: Record<string, unknown>): void {
  if (record.schema_version !== CAMERA_TRANSPORT_SCHEMA_VERSION) {
    throw new CameraClientError(
      "UNSUPPORTED_SCHEMA",
      "Версия ответа camera service не поддерживается приложением.",
    );
  }
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isPositiveInteger(value: unknown): value is number {
  return isNonNegativeInteger(value) && value > 0;
}

function transportError(): CameraClientError {
  return new CameraClientError(
    "TRANSPORT_ERROR",
    "Не удалось прочитать ответ camera service.",
  );
}
