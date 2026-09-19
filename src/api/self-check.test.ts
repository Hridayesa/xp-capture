import { describe, expect, it, vi } from "vitest";

import {
  SelfCheckClientError,
  decodePublicError,
  decodeSelfCheckReport,
  runSelfCheck,
} from "./self-check";

const validReport = {
  schema_version: 1,
  checks: [
    {
      check: "opencv_load",
      status: "passed",
    },
  ],
  opencv_version: "4.12.0",
};

describe("self-check API", () => {
  it("decodes the supported report schema", () => {
    expect(decodeSelfCheckReport(validReport)).toEqual(validReport);
  });

  it("rejects an unknown report schema", () => {
    try {
      decodeSelfCheckReport({ ...validReport, schema_version: 2 });
      expect.unreachable("unknown schema must be rejected");
    } catch (error: unknown) {
      expect(error).toBeInstanceOf(SelfCheckClientError);
      expect(error).toMatchObject({ code: "unsupported_schema" });
    }
  });

  it("maps an unknown public error code without exposing its message", () => {
    const error = decodePublicError({
      schema_version: 1,
      code: "future_native_error",
      message: "C:\\private\\native-source.dll",
    });

    expect(error.code).toBe("unknown_error_code");
    expect(error.message).not.toContain("private");
  });

  it("invokes the single camera-independent Tauri command", async () => {
    const invoke = vi.fn().mockResolvedValue(validReport);

    await expect(runSelfCheck(invoke)).resolves.toEqual(validReport);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("run_self_check");
  });

  it("maps malformed command rejection to a controlled transport error", async () => {
    const invoke = vi.fn().mockRejectedValue("raw native debug output");

    await expect(runSelfCheck(invoke)).rejects.toMatchObject({
      code: "transport_error",
    });
  });
});
