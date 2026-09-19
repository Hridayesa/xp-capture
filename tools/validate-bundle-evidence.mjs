import { readFile } from "node:fs/promises";
import Ajv2020 from "ajv/dist/2020.js";
import { evidenceStagesPass, requiredBundleStages } from "./bundle-verification-policy.ts";

const evidencePath = process.argv[2];
if (!evidencePath) {
  console.error("Usage: bun tools/validate-bundle-evidence.mjs <evidence.json>");
  process.exit(2);
}

const schema = JSON.parse(await readFile(new URL("./bundle-verification.schema.json", import.meta.url), "utf8"));
const evidence = JSON.parse(await readFile(evidencePath, "utf8"));
const ajv = new Ajv2020({ allErrors: true, strict: true });
const validate = ajv.compile(schema);
if (!validate(evidence)) {
  console.error(ajv.errorsText(validate.errors, { separator: "\n" }));
  process.exit(1);
}

if (evidence.stages.some((stage, index) => stage.name !== requiredBundleStages[index])) {
  console.error("Bundle verification stages must use the required order and unique names");
  process.exit(1);
}
if (evidence.status === "passed" && !evidenceStagesPass(evidence.stages)) {
  console.error("Passed bundle evidence requires every stage to pass");
  process.exit(1);
}

console.log(`Bundle verification evidence schema valid: ${evidencePath}`);
