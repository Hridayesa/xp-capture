import { invoke } from "@tauri-apps/api/core";

export const SELF_CHECK_TRANSPORT_SCHEMA_VERSION = 1;

export type CheckCode =
  | "opencv_load"
  | "image_codec"
  | "writer_open"
  | "writer_backend"
  | "writer_roundtrip";

export type CheckStatus = "passed" | "failed" | "skipped";

export interface CheckResult {
  check: CheckCode;
  status: CheckStatus;
  public_message?: string;
}

export interface SelfCheckReport {
  schema_version: number;
  checks: CheckResult[];
  opencv_version: string | null;
}

export type PublicErrorCode =
  | "opencv_load_failed"
  | "opencv_backend_missing"
  | "image_codec_failed"
  | "writer_failed"
  | "writer_roundtrip_failed"
  | "internal_error";

export type ClientErrorCode =
  | PublicErrorCode
  | "transport_error"
  | "unsupported_schema"
  | "unknown_error_code";

export class SelfCheckClientError extends Error {
  readonly code: ClientErrorCode;

  constructor(code: ClientErrorCode, message: string) {
    super(message);
    this.name = "SelfCheckClientError";
    this.code = code;
  }
}

type Invoke = <T>(command: string) => Promise<T>;

const checkCodes = new Set<CheckCode>([
  "opencv_load",
  "image_codec",
  "writer_open",
  "writer_backend",
  "writer_roundtrip",
]);
const checkStatuses = new Set<CheckStatus>(["passed", "failed", "skipped"]);
const publicErrorCodes = new Set<PublicErrorCode>([
  "opencv_load_failed",
  "opencv_backend_missing",
  "image_codec_failed",
  "writer_failed",
  "writer_roundtrip_failed",
  "internal_error",
]);

export async function runSelfCheck(
  invokeCommand: Invoke = invoke,
): Promise<SelfCheckReport> {
  try {
    return decodeSelfCheckReport(
      await invokeCommand<unknown>("run_self_check"),
    );
  } catch (error: unknown) {
    if (error instanceof SelfCheckClientError) {
      throw error;
    }
    throw decodePublicError(error);
  }
}

export function decodeSelfCheckReport(value: unknown): SelfCheckReport {
  const record = asRecord(value);
  requireSupportedSchema(record);
  if (!Array.isArray(record.checks)) {
    throw transportError();
  }
  const checks = record.checks.map(decodeCheckResult);
  const opencvVersion = record.opencv_version;
  if (opencvVersion !== null && typeof opencvVersion !== "string") {
    throw transportError();
  }
  return {
    schema_version: SELF_CHECK_TRANSPORT_SCHEMA_VERSION,
    checks,
    opencv_version: opencvVersion,
  };
}

export function decodePublicError(value: unknown): SelfCheckClientError {
  let record: Record<string, unknown>;
  try {
    record = asRecord(value);
    requireSupportedSchema(record);
  } catch (error: unknown) {
    return error instanceof SelfCheckClientError ? error : transportError();
  }
  if (typeof record.code !== "string" || typeof record.message !== "string") {
    return transportError();
  }
  if (!publicErrorCodes.has(record.code as PublicErrorCode)) {
    return new SelfCheckClientError(
      "unknown_error_code",
      "Приложение вернуло неизвестный код ошибки самопроверки.",
    );
  }
  return new SelfCheckClientError(
    record.code as PublicErrorCode,
    record.message,
  );
}

function decodeCheckResult(value: unknown): CheckResult {
  const record = asRecord(value);
  if (
    typeof record.check !== "string" ||
    !checkCodes.has(record.check as CheckCode) ||
    typeof record.status !== "string" ||
    !checkStatuses.has(record.status as CheckStatus) ||
    (record.public_message !== undefined &&
      typeof record.public_message !== "string")
  ) {
    throw transportError();
  }
  return {
    check: record.check as CheckCode,
    status: record.status as CheckStatus,
    ...(record.public_message === undefined
      ? {}
      : { public_message: record.public_message as string }),
  };
}

function asRecord(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw transportError();
  }
  return value as Record<string, unknown>;
}

function requireSupportedSchema(record: Record<string, unknown>): void {
  if (record.schema_version !== SELF_CHECK_TRANSPORT_SCHEMA_VERSION) {
    throw new SelfCheckClientError(
      "unsupported_schema",
      "Версия ответа самопроверки не поддерживается приложением.",
    );
  }
}

function transportError(): SelfCheckClientError {
  return new SelfCheckClientError(
    "transport_error",
    "Не удалось прочитать ответ самопроверки.",
  );
}
