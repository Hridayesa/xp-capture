import { readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";
import { requiredGateGaps } from "./verify-policy";

const root = process.cwd();
const bun = Bun.which("bun") ?? "bun";
const full = !Bun.argv.includes("--quick");
let failures = 0;
let executed = 0;

async function exists(path: string): Promise<boolean> {
  try {
    await stat(resolve(root, path));
    return true;
  } catch {
    return false;
  }
}

async function run(label: string, command: string[]): Promise<void> {
  console.log(`\n[${label}] ${command.join(" ")}`);
  const process = Bun.spawn(command, { cwd: root, stdin: "inherit", stdout: "inherit", stderr: "inherit", env: { ...Bun.env, CI: "1" } });
  const code = await process.exited;
  executed += 1;
  if (code !== 0) failures += 1;
}

const manifest = JSON.parse(await readFile(resolve(root, ".sdd/template.json"), "utf8"));
const hasCargoManifest = await exists("Cargo.toml");
const hasPackageManifest = await exists("package.json");
const packageJson = hasPackageManifest ? JSON.parse(await readFile(resolve(root, "package.json"), "utf8")) : {};
const scripts: Record<string, unknown> = packageJson.scripts ?? {};
const gaps = requiredGateGaps({
  profile: manifest.profile,
  hasCargoManifest,
  cargoAvailable: Boolean(Bun.which("cargo")),
  hasPackageManifest,
  packageScripts: scripts,
});

for (const gap of gaps) console.error(`[missing] ${gap}`);
failures += gaps.length;

await run("openspec", [bun, "run", "tools/openspec.ts", "validate", "--all", "--strict", "--no-interactive"]);

if (hasCargoManifest && Bun.which("cargo")) {
  await run("rustfmt", ["cargo", "fmt", "--all", "--", "--check"]);
  await run("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"]);
  if (full) await run("rust-tests", ["cargo", "test", "--workspace", "--all-features"]);
}

if (hasPackageManifest) {
  const testScript = typeof scripts["test:unit"] === "string" ? "test:unit" : "test";
  const names = full ? ["typecheck", "lint", testScript, "build"] : ["typecheck", "lint"];
  for (const name of names) if (typeof scripts[name] === "string") await run(`frontend:${name}`, ["bun", "run", name]);
}

if (full && hasCargoManifest) {
  const cargoInputs = `${await readFile(resolve(root, "Cargo.toml"), "utf8")}\n${(await exists("Cargo.lock")) ? await readFile(resolve(root, "Cargo.lock"), "utf8") : ""}`;
  if (/\btestcontainers\b/.test(cargoInputs)) {
    if (!Bun.which("docker")) {
      console.error("[missing] docker is required by testcontainers");
      failures += 1;
    } else await run("docker", ["docker", "info"]);
  }
}
if (failures > 0) {
  console.error(`\nQuality gate failed: ${failures} issue(s) found.`);
  process.exitCode = 1;
} else {
  console.log(`\nQuality gate passed: ${executed} command(s) completed.`);
}
