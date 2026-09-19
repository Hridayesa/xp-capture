import { invoke } from "@tauri-apps/api/core";

import modeCandidates from "../../config/mode-candidates.json";
import {
  CAMERA_TRANSPORT_SCHEMA_VERSION,
  CameraClientError,
  decodeCameraPublicError,
  type CaptureBackend,
} from "./camera";

export type ProfileServiceState =
  | "idle"
  | "scanning"
  | "profiling"
  | "profile_ready"
  | "faulted"
  | "stuck";
export type ProfileOperationStatus =
  | "profiling"
  | "completed"
  | "cancelled"
  | "failed"
  | "stuck";
export type CandidatePhase =
  | "opening"
  | "applying"
  | "reported"
  | "first_frame"
  | "warmup"
  | "measuring"
  | "release"
  | "reopen_delay";
export type CaptureModeStatus =
  | "opening_failed"
  | "applying_failed"
  | "read_failed"
  | "release_failed"
  | "first_frame_timeout"
  | "read_stalled"
  | "candidate_timed_out"
  | "operation_timed_out"
  | "coerced_resolution"
  | "capture_under_target"
  | "capture_unstable"
  | "verified_fourcc_reported_match"
  | "verified_fourcc_unconfirmed"
  | "cancelled";

export interface ResolutionV1 {
  width: number;
  height: number;
}

export interface ModeCandidatePolicyV1 {
  schema_version: 1;
  fourcc: string[];
  resolutions: ResolutionV1[];
  fps: number[];
  warmup_ms: number;
  capture_only_ms: number;
  first_frame_deadline_ms: number;
  candidate_deadline_ms: number;
  operation_deadline_ms: number;
  shutdown_deadline_ms: number;
  reopen_delay_ms: number;
  minimum_fps_ratio: number;
  maximum_read_failure_ratio: number;
  maximum_gap_periods: number;
  maximum_long_gap_ratio: number;
}

export interface ModeTupleV1 extends ResolutionV1 {
  fourcc: string;
  requested_fps: number;
}

export interface ProfileStartedV1 {
  schema_version: 1;
  profile_id: string;
  endpoint_key: string;
  scan_generation: number;
  backend: CaptureBackend;
  policy: ModeCandidatePolicyV1;
  config_hash: string;
  total_candidates: number;
}

export interface ProfileProgressV1 extends ProfileStartedV1 {
  service_state: ProfileServiceState;
  status: ProfileOperationStatus;
  completed_candidates: number;
  current_candidate: ModeTupleV1 | null;
  current_phase: CandidatePhase | null;
  failure_code: string | null;
}

export interface CandidateResultV1 {
  ordinal: number;
  attempt_count: number;
  retry_reason: string | null;
  requested: ModeTupleV1;
  set: { fourcc: boolean; width: boolean; height: boolean; fps: boolean };
  reported: {
    fourcc: string | null;
    width: number | null;
    height: number | null;
    fps: number | null;
  };
  metrics: {
    elapsed_ms: number;
    read_attempts: number;
    captured_frames: number;
    empty_frames: number;
    read_failures: number;
    measured_fps: number;
    median_interval_ms: number | null;
    p95_interval_ms: number | null;
    p99_interval_ms: number | null;
    maximum_gap_ms: number | null;
    long_gap_count: number;
    long_gap_ratio: number;
    actual_resolutions: ResolutionV1[];
  };
  capture_mode_status: CaptureModeStatus;
  failure_reasons: string[];
  verified_mode_id: string | null;
}

export interface ProfileReportV1 {
  schema_version: 1;
  profile_id: string;
  started_at_unix_ms: number;
  environment: {
    app_version: string;
    rust_version: string;
    tauri_version: string;
    opencv_version: string;
    opencv_crate_version: string;
  };
  endpoint_key: string;
  scan_generation: number;
  backend: CaptureBackend;
  policy: ModeCandidatePolicyV1;
  config_hash: string;
  status: Exclude<ProfileOperationStatus, "profiling">;
  failure_code: string | null;
  runtime_validation_status: "not_run";
  external_validation_status: "not_run";
  results: CandidateResultV1[];
  max_verified_by_resolution: Array<{
    resolution: ResolutionV1;
    result_ordinals: number[];
    measured_fps: number;
  }>;
}

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type ProgressGetter = (profileId: string) => Promise<ProfileProgressV1>;

const backends = new Set<CaptureBackend>(["MSMF", "DSHOW"]);
const serviceStates = new Set<ProfileServiceState>([
  "idle", "scanning", "profiling", "profile_ready", "faulted", "stuck",
]);
const operationStatuses = new Set<ProfileOperationStatus>([
  "profiling", "completed", "cancelled", "failed", "stuck",
]);
const phases = new Set<CandidatePhase>([
  "opening", "applying", "reported", "first_frame", "warmup", "measuring", "release", "reopen_delay",
]);
const modeStatuses = new Set<CaptureModeStatus>([
  "opening_failed", "applying_failed", "read_failed", "release_failed",
  "first_frame_timeout", "read_stalled", "candidate_timed_out", "operation_timed_out",
  "coerced_resolution", "capture_under_target", "capture_unstable",
  "verified_fourcc_reported_match", "verified_fourcc_unconfirmed", "cancelled",
]);
const terminalStatuses = new Set<ProfileOperationStatus>([
  "completed", "cancelled", "failed", "stuck",
]);

export const DEFAULT_MODE_CANDIDATE_POLICY = decodePolicy(modeCandidates);

export async function startProfile(
  endpointKey: string,
  candidateConfig: ModeCandidatePolicyV1 = DEFAULT_MODE_CANDIDATE_POLICY,
  invokeCommand: Invoke = invoke,
): Promise<ProfileStartedV1> {
  try {
    return decodeProfileStarted(await invokeCommand<unknown>("start_profile", {
      requestV1: {
        schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
        endpoint_key: endpointKey,
        candidate_config: candidateConfig,
      },
    }));
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export async function getProfileStatus(profileId: string, invokeCommand: Invoke = invoke): Promise<ProfileProgressV1> {
  try {
    return decodeProfileProgress(await invokeCommand<unknown>("get_profile_status", {
      requestV1: operationRequest(profileId),
    }));
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export async function getProfileResult(profileId: string, invokeCommand: Invoke = invoke): Promise<ProfileReportV1> {
  try {
    return decodeProfileReport(await invokeCommand<unknown>("get_profile_result", {
      requestV1: operationRequest(profileId),
    }));
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export async function cancelProfile(profileId: string, invokeCommand: Invoke = invoke): Promise<ProfileProgressV1> {
  try {
    return decodeProfileProgress(await invokeCommand<unknown>("cancel_profile", {
      requestV1: operationRequest(profileId),
    }));
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export class ProfilePollingController {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private token = 0;
  private running = false;

  constructor(private readonly getter: ProgressGetter = getProfileStatus, private readonly intervalMs = 250) {}

  start(profileId: string, onProgress: (progress: ProfileProgressV1) => void, onError: (error: CameraClientError) => void): void {
    this.stop();
    this.running = true;
    const token = this.token;
    void this.tick(token, profileId, onProgress, onError);
  }

  stop(): void {
    this.running = false;
    this.token += 1;
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  isRunning(): boolean { return this.running; }

  private async tick(token: number, profileId: string, onProgress: (progress: ProfileProgressV1) => void, onError: (error: CameraClientError) => void): Promise<void> {
    try {
      const progress = await this.getter(profileId);
      if (!this.running || token !== this.token) return;
      onProgress(progress);
      if (terminalStatuses.has(progress.status)) {
        this.stop();
        return;
      }
      this.timer = setTimeout(() => {
        this.timer = null;
        void this.tick(token, profileId, onProgress, onError);
      }, this.intervalMs);
    } catch (error: unknown) {
      if (!this.running || token !== this.token) return;
      this.stop();
      onError(normalizeError(error));
    }
  }
}

export function decodeProfileStarted(value: unknown): ProfileStartedV1 {
  const record = exact(value, ["schema_version", "profile_id", "endpoint_key", "scan_generation", "backend", "policy", "config_hash", "total_candidates"]);
  schema(record);
  baseStarted(record);
  return {
    schema_version: 1,
    profile_id: record.profile_id as string,
    endpoint_key: record.endpoint_key as string,
    scan_generation: record.scan_generation as number,
    backend: record.backend as CaptureBackend,
    policy: decodePolicy(record.policy),
    config_hash: record.config_hash as string,
    total_candidates: record.total_candidates as number,
  };
}

export function decodeProfileProgress(value: unknown): ProfileProgressV1 {
  const record = exact(value, ["schema_version", "profile_id", "service_state", "status", "endpoint_key", "scan_generation", "backend", "policy", "config_hash", "completed_candidates", "total_candidates", "current_candidate", "current_phase", "failure_code"]);
  schema(record);
  baseStarted(record);
  if (typeof record.service_state !== "string" || !serviceStates.has(record.service_state as ProfileServiceState)
    || typeof record.status !== "string" || !operationStatuses.has(record.status as ProfileOperationStatus)
    || !integer(record.completed_candidates) || (record.completed_candidates as number) > (record.total_candidates as number)
    || (record.current_phase !== null && (typeof record.current_phase !== "string" || !phases.has(record.current_phase as CandidatePhase)))
    || (record.failure_code !== null && typeof record.failure_code !== "string")) throw transportError();
  return {
    ...decodeProfileStarted({ schema_version: record.schema_version, profile_id: record.profile_id, endpoint_key: record.endpoint_key, scan_generation: record.scan_generation, backend: record.backend, policy: record.policy, config_hash: record.config_hash, total_candidates: record.total_candidates }),
    service_state: record.service_state as ProfileServiceState,
    status: record.status as ProfileOperationStatus,
    completed_candidates: record.completed_candidates as number,
    current_candidate: record.current_candidate === null ? null : decodeTuple(record.current_candidate),
    current_phase: record.current_phase as CandidatePhase | null,
    failure_code: record.failure_code as string | null,
  };
}

export function decodeProfileReport(value: unknown): ProfileReportV1 {
  const record = exact(value, ["schema_version", "profile_id", "started_at_unix_ms", "environment", "endpoint_key", "scan_generation", "backend", "policy", "config_hash", "status", "failure_code", "runtime_validation_status", "external_validation_status", "results", "max_verified_by_resolution"]);
  schema(record);
  if (typeof record.profile_id !== "string" || record.profile_id.length === 0 || typeof record.endpoint_key !== "string"
    || !integer(record.started_at_unix_ms) || !integer(record.scan_generation)
    || typeof record.backend !== "string" || !backends.has(record.backend as CaptureBackend)
    || typeof record.config_hash !== "string" || !/^[0-9a-f]{64}$/.test(record.config_hash)
    || typeof record.status !== "string" || record.status === "profiling" || !operationStatuses.has(record.status as ProfileOperationStatus)
    || (record.failure_code !== null && typeof record.failure_code !== "string")
    || record.runtime_validation_status !== "not_run" || record.external_validation_status !== "not_run"
    || !Array.isArray(record.results) || !Array.isArray(record.max_verified_by_resolution)) throw transportError();
  const environmentRecord = decodeStringRecord(record.environment, ["app_version", "rust_version", "tauri_version", "opencv_version", "opencv_crate_version"]);
  const environment = {
    app_version: environmentRecord.app_version,
    rust_version: environmentRecord.rust_version,
    tauri_version: environmentRecord.tauri_version,
    opencv_version: environmentRecord.opencv_version,
    opencv_crate_version: environmentRecord.opencv_crate_version,
  };
  return {
    schema_version: 1, profile_id: record.profile_id, started_at_unix_ms: record.started_at_unix_ms,
    environment, endpoint_key: record.endpoint_key, scan_generation: record.scan_generation,
    backend: record.backend as CaptureBackend, policy: decodePolicy(record.policy), config_hash: record.config_hash,
    status: record.status as Exclude<ProfileOperationStatus, "profiling">, failure_code: record.failure_code as string | null,
    runtime_validation_status: "not_run", external_validation_status: "not_run",
    results: record.results.map(decodeCandidateResult),
    max_verified_by_resolution: record.max_verified_by_resolution.map(decodeMaximum),
  };
}

function decodeCandidateResult(value: unknown): CandidateResultV1 {
  const record = exact(value, ["ordinal", "attempt_count", "retry_reason", "requested", "set", "reported", "metrics", "capture_mode_status", "failure_reasons", "verified_mode_id"]);
  const set = exact(record.set, ["fourcc", "width", "height", "fps"]);
  const reported = exact(record.reported, ["fourcc", "width", "height", "fps"]);
  const metrics = exact(record.metrics, ["elapsed_ms", "read_attempts", "captured_frames", "empty_frames", "read_failures", "measured_fps", "median_interval_ms", "p95_interval_ms", "p99_interval_ms", "maximum_gap_ms", "long_gap_count", "long_gap_ratio", "actual_resolutions"]);
  if (!integer(record.ordinal) || !positive(record.attempt_count) || (record.retry_reason !== null && typeof record.retry_reason !== "string")
    || !Object.values(set).every((item) => typeof item === "boolean")
    || !nullableString(reported.fourcc) || ![reported.width, reported.height, reported.fps].every(nullableNumber)
    || ![metrics.elapsed_ms, metrics.measured_fps, metrics.long_gap_ratio].every(finiteNumber)
    || ![metrics.read_attempts, metrics.captured_frames, metrics.empty_frames, metrics.read_failures, metrics.long_gap_count].every(integer)
    || ![metrics.median_interval_ms, metrics.p95_interval_ms, metrics.p99_interval_ms, metrics.maximum_gap_ms].every(nullableNumber)
    || !Array.isArray(metrics.actual_resolutions)
    || typeof record.capture_mode_status !== "string" || !modeStatuses.has(record.capture_mode_status as CaptureModeStatus)
    || !Array.isArray(record.failure_reasons) || !record.failure_reasons.every((item) => typeof item === "string")
    || !nullableString(record.verified_mode_id)) throw transportError();
  return {
    ordinal: record.ordinal, attempt_count: record.attempt_count, retry_reason: record.retry_reason as string | null,
    requested: decodeTuple(record.requested), set: set as CandidateResultV1["set"],
    reported: reported as unknown as CandidateResultV1["reported"],
    metrics: { ...(metrics as unknown as Omit<CandidateResultV1["metrics"], "actual_resolutions">), actual_resolutions: metrics.actual_resolutions.map(decodeResolution) },
    capture_mode_status: record.capture_mode_status as CaptureModeStatus,
    failure_reasons: record.failure_reasons as string[], verified_mode_id: record.verified_mode_id as string | null,
  };
}

function decodeMaximum(value: unknown): ProfileReportV1["max_verified_by_resolution"][number] {
  const record = exact(value, ["resolution", "result_ordinals", "measured_fps"]);
  if (!Array.isArray(record.result_ordinals) || !record.result_ordinals.every(integer) || !finiteNumber(record.measured_fps)) throw transportError();
  return { resolution: decodeResolution(record.resolution), result_ordinals: record.result_ordinals, measured_fps: record.measured_fps };
}

function decodePolicy(value: unknown): ModeCandidatePolicyV1 {
  const keys = ["schema_version", "fourcc", "resolutions", "fps", "warmup_ms", "capture_only_ms", "first_frame_deadline_ms", "candidate_deadline_ms", "operation_deadline_ms", "shutdown_deadline_ms", "reopen_delay_ms", "minimum_fps_ratio", "maximum_read_failure_ratio", "maximum_gap_periods", "maximum_long_gap_ratio"] as const;
  const record = exact(value, keys); schema(record);
  const integerKeys = keys.slice(4, 11);
  if (!Array.isArray(record.fourcc) || !record.fourcc.every(isFourCc)
    || !Array.isArray(record.resolutions) || !Array.isArray(record.fps) || !record.fps.every((item) => finiteNumber(item) && item > 0)
    || !integerKeys.every((key) => positive(record[key]))
    || ![record.minimum_fps_ratio, record.maximum_read_failure_ratio, record.maximum_gap_periods, record.maximum_long_gap_ratio].every(finiteNumber)) throw transportError();
  return { ...(record as unknown as ModeCandidatePolicyV1), resolutions: record.resolutions.map(decodeResolution), fourcc: [...record.fourcc] as string[], fps: [...record.fps] as number[] };
}

function decodeTuple(value: unknown): ModeTupleV1 {
  const record = exact(value, ["fourcc", "width", "height", "requested_fps"]);
  if (typeof record.fourcc !== "string" || !positive(record.width) || !positive(record.height) || !finiteNumber(record.requested_fps) || record.requested_fps <= 0) throw transportError();
  return record as unknown as ModeTupleV1;
}

function decodeResolution(value: unknown): ResolutionV1 {
  const record = exact(value, ["width", "height"]);
  if (!positive(record.width) || !positive(record.height)) throw transportError();
  return record as unknown as ResolutionV1;
}

function baseStarted(record: Record<string, unknown>): void {
  if (typeof record.profile_id !== "string" || record.profile_id.length === 0 || typeof record.endpoint_key !== "string" || record.endpoint_key.length === 0
    || !integer(record.scan_generation) || typeof record.backend !== "string" || !backends.has(record.backend as CaptureBackend)
    || typeof record.config_hash !== "string" || !/^[0-9a-f]{64}$/.test(record.config_hash) || !positive(record.total_candidates)) throw transportError();
  decodePolicy(record.policy);
}

function operationRequest(profileId: string) { return { schema_version: 1, profile_id: profileId }; }
function schema(record: Record<string, unknown>): void { if (record.schema_version !== 1) throw new CameraClientError("UNSUPPORTED_SCHEMA", "Версия profile transport не поддерживается."); }
function exact(value: unknown, keys: readonly string[]): Record<string, unknown> { if (typeof value !== "object" || value === null || Array.isArray(value)) throw transportError(); const record = value as Record<string, unknown>; if (Object.keys(record).length !== keys.length || keys.some((key) => !Object.hasOwn(record, key))) throw transportError(); return record; }
function decodeStringRecord<T extends readonly string[]>(value: unknown, keys: T): Record<T[number], string> { const record = exact(value, keys); if (!Object.values(record).every((item) => typeof item === "string")) throw transportError(); return record as Record<T[number], string>; }
function integer(value: unknown): value is number { return typeof value === "number" && Number.isSafeInteger(value) && value >= 0; }
function positive(value: unknown): value is number { return integer(value) && value > 0; }
function finiteNumber(value: unknown): value is number { return typeof value === "number" && Number.isFinite(value); }
function nullableNumber(value: unknown): boolean { return value === null || finiteNumber(value); }
function nullableString(value: unknown): boolean { return value === null || typeof value === "string"; }
function isFourCc(value: unknown): value is string { return typeof value === "string" && value.length === 4 && [...value].every((character) => character.charCodeAt(0) <= 0x7f); }
function normalizeError(error: unknown): CameraClientError { return error instanceof CameraClientError ? error : decodeCameraPublicError(error); }
function transportError(): CameraClientError { return new CameraClientError("TRANSPORT_ERROR", "Не удалось прочитать ответ profile service."); }
