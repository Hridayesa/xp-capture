import { describe, expect, it } from "vitest";
import { evidenceStagesPass, requiredBundleStages } from "../bundle-verification-policy";

function stagesWithFailure(failedStage?: string) {
  return requiredBundleStages.map((name) => ({
    name,
    status: name === failedStage ? "failed" : "passed",
  }));
}

describe("bundle verification fail-closed policy", () => {
  it("accepts only the complete ordered passed stage set", () => {
    expect(evidenceStagesPass(stagesWithFailure())).toBe(true);
    expect(evidenceStagesPass(stagesWithFailure().slice(1))).toBe(false);
    expect(evidenceStagesPass(stagesWithFailure().toReversed())).toBe(false);
  });

  it.each(["module_provenance", "gui_smoke", "uninstall"])(
    "rejects a failed %s stage",
    (failedStage) => {
      expect(evidenceStagesPass(stagesWithFailure(failedStage))).toBe(false);
    },
  );
});
