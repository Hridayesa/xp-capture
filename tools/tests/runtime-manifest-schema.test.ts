import Ajv2020 from "ajv/dist/2020.js";
import { describe, expect, it } from "vitest";
import schema from "../runtime-manifest.schema.json";
import invalidArchitecture from "./fixtures/runtime-manifest-invalid-architecture.json";
import invalidHash from "./fixtures/runtime-manifest-invalid-hash.json";
import missingLicense from "./fixtures/runtime-manifest-missing-license.json";
import validManifest from "./fixtures/runtime-manifest-valid.json";
import wildcardManifest from "./fixtures/runtime-manifest-wildcard.json";

const ajv = new Ajv2020({ allErrors: true, strict: true });
const validate = ajv.compile(schema);

describe("runtime manifest schema", () => {
  it("accepts an explicit x64 allowlist", () => {
    expect(validate(validManifest), JSON.stringify(validate.errors)).toBe(true);
  });

  it("accepts repository-local MSVC Redistributable sources", () => {
    const manifest = structuredClone(validManifest);
    manifest.files[0] = {
      ...manifest.files[0],
      name: "VCRUNTIME140.dll",
      source: ".tools/msvc-redist/14.44.35112/x64/Microsoft.VC143.CRT/VCRUNTIME140.dll",
      bundleDestination: "VCRUNTIME140.dll",
      purpose: "transitive",
      licenseNoticePath: ".tools/msvc-redist/14.44.35112/Redist.txt",
    };

    expect(validate(manifest), JSON.stringify(validate.errors)).toBe(true);
  });

  it.each([
    ["wildcard", wildcardManifest],
    ["invalid SHA-256", invalidHash],
    ["invalid architecture", invalidArchitecture],
    ["missing license metadata", missingLicense],
  ])("rejects %s fixture", (_name, fixture) => {
    expect(validate(fixture)).toBe(false);
  });
});
