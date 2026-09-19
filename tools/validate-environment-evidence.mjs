import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const evidencePath = process.argv[2] ?? resolve(root, "evidence/environment-summary.json");
const [evidence, packageJson, cargoManifest, cargoLock, rustToolchain, vcpkgConfig, runtimeManifest] = await Promise.all([
  readFile(evidencePath, "utf8").then(JSON.parse),
  readFile(resolve(root, "package.json"), "utf8").then(JSON.parse),
  readFile(resolve(root, "src-tauri/Cargo.toml"), "utf8"),
  readFile(resolve(root, "Cargo.lock"), "utf8"),
  readFile(resolve(root, "rust-toolchain.toml"), "utf8"),
  readFile(resolve(root, "vcpkg-configuration.json"), "utf8").then(JSON.parse),
  readFile(resolve(root, "runtime/manifest.json"), "utf8").then(JSON.parse),
]);

const expected = {
  rust: rustToolchain.match(/channel\s*=\s*"([^"]+)"/)?.[1],
  bun: packageJson.packageManager.replace(/^bun@/, ""),
  tauriCrate: cargoManifest.match(/^tauri\s*=\s*\{\s*version\s*=\s*"=([^"]+)"/m)?.[1],
  tauriBuild: cargoManifest.match(/^tauri-build\s*=\s*\{\s*version\s*=\s*"=([^"]+)"/m)?.[1],
  tauriCli: packageJson.devDependencies["@tauri-apps/cli"],
  tauriApi: packageJson.dependencies["@tauri-apps/api"],
  opencvCrate: cargoLock.match(/\[\[package\]\]\s*name = "opencv"\s*version = "([^"]+)"/s)?.[1],
};

const errors = [];
for (const [name, version] of Object.entries(expected)) {
  if (!version || evidence.application[name] !== version) errors.push(`${name} does not match manifests/lockfiles`);
}
if (evidence.schemaVersion !== 1) errors.push("unsupported evidence schemaVersion");
if (evidence.nativeSupply.vcpkgRevision !== vcpkgConfig["default-registry"].baseline) errors.push("vcpkg revision mismatch");
if (evidence.nativeSupply.vcpkgRevision !== runtimeManifest.vcpkgRevision) errors.push("runtime manifest vcpkg revision mismatch");
if (evidence.nativeSupply.triplet !== runtimeManifest.triplet) errors.push("runtime manifest triplet mismatch");
if (evidence.nativeSupply.opencvVersion !== "4.12.0" || evidence.nativeSupply.opencvPortVersion !== 7) errors.push("OpenCV native version is not 4.12.0#7");
if (evidence.runtimeManifest.fileCount !== runtimeManifest.files.length) errors.push("runtime file count mismatch");
if (!runtimeManifest.files.some((file) => file.source.startsWith(`.tools/msvc-redist/${evidence.nativeSupply.msvcRedistributable}/`))) {
  errors.push("MSVC Redistributable version is absent from runtime manifest");
}
if (errors.length > 0) {
  for (const error of errors) console.error(error);
  process.exit(1);
}
console.log(`Environment evidence is consistent: ${evidencePath}`);
