import { readFile } from "node:fs/promises";
import Ajv2020 from "ajv/dist/2020.js";

const manifestPath = process.argv[2];
if (!manifestPath) {
  console.error("Usage: bun tools/validate-runtime-manifest.mjs <manifest.json>");
  process.exit(2);
}

const schemaUrl = new URL("./runtime-manifest.schema.json", import.meta.url);
const [schemaText, manifestText] = await Promise.all([
  readFile(schemaUrl, "utf8"),
  readFile(manifestPath, "utf8"),
]);
const schema = JSON.parse(schemaText);
const manifest = JSON.parse(manifestText);
const ajv = new Ajv2020({ allErrors: true, strict: true });
const validate = ajv.compile(schema);

if (!validate(manifest)) {
  console.error(ajv.errorsText(validate.errors, { separator: "\n" }));
  process.exit(1);
}

console.log(`Runtime manifest schema valid: ${manifestPath}`);
