export const requiredBundleStages = [
  "manifest",
  "install",
  "headless_self_check",
  "module_provenance",
  "gui_smoke",
  "uninstall",
] as const;

type VerificationStage = {
  name: string;
  status: string;
};

export function evidenceStagesPass(stages: VerificationStage[]): boolean {
  return (
    stages.length === requiredBundleStages.length &&
    stages.every(
      (stage, index) => stage.name === requiredBundleStages[index] && stage.status === "passed",
    )
  );
}
