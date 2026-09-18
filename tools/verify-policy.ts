export type StackProfile = "rust" | "web" | "desktop";

export type GateInputs = {
  profile: string;
  hasCargoManifest: boolean;
  cargoAvailable: boolean;
  hasPackageManifest: boolean;
  packageScripts: Record<string, unknown>;
};

export function requiredGateGaps(inputs: GateInputs): string[] {
  const gaps: string[] = [];
  if (!(["rust", "web", "desktop"] as string[]).includes(inputs.profile)) {
    gaps.push(`unknown stack profile '${inputs.profile}'`);
    return gaps;
  }

  if (!inputs.hasCargoManifest) gaps.push("Cargo.toml is missing");
  else if (!inputs.cargoAvailable) gaps.push("cargo is not available on PATH");

  if (inputs.profile === "web" || inputs.profile === "desktop") {
    if (!inputs.hasPackageManifest) gaps.push("package.json is missing");
    for (const name of ["typecheck", "lint", "build"]) {
      if (typeof inputs.packageScripts[name] !== "string") gaps.push(`package script '${name}' is missing`);
    }
    if (typeof inputs.packageScripts["test:unit"] !== "string" && typeof inputs.packageScripts.test !== "string") {
      gaps.push("frontend test script is missing (expected 'test:unit' or 'test')");
    }
  }

  return gaps;
}
