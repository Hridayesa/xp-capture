import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { archiveReadinessGaps, type OpenSpecStatus } from "./archive-policy";

const root = process.cwd();
const bun = Bun.which("bun") ?? "bun";
const change = Bun.argv[2];
if (!change || !/^[a-z0-9][a-z0-9-]*$/.test(change)) {
  throw new Error("Usage: bun run tools/archive-change.ts <kebab-case-change-name>");
}

async function capture(args: string[]): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "codex-sdd-archive-"));
  const stdoutPath = join(directory, "stdout.txt");
  try {
    const child = Bun.spawn([bun, "run", "tools/openspec.ts", ...args], { cwd: root, stdout: Bun.file(stdoutPath), stderr: "inherit" });
    const code = await child.exited;
    if (code !== 0) throw new Error(`OpenSpec command failed: ${args.join(" ")}`);
    return await readFile(stdoutPath, "utf8");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function run(label: string, command: string[]): Promise<void> {
  console.log(`[${label}] ${command.join(" ")}`);
  const child = Bun.spawn(command, { cwd: root, stdin: "inherit", stdout: "inherit", stderr: "inherit" });
  const code = await child.exited;
  if (code !== 0) throw new Error(`${label} failed with exit code ${code}`);
}

const status = JSON.parse(await capture(["status", "--change", change, "--json"])) as OpenSpecStatus;
const tasksPath = resolve(root, "openspec", "changes", change, "tasks.md");
const tasks = await readFile(tasksPath, "utf8");
const gaps = archiveReadinessGaps(status, tasks);
if (gaps.length > 0) throw new Error(`Archive blocked:\n${gaps.map((gap) => `- ${gap}`).join("\n")}`);

await run("strict-validation", [bun, "run", "tools/openspec.ts", "validate", change, "--strict", "--no-interactive"]);
await run("quality-gate", [bun, "run", "tools/verify.ts"]);
await run("archive", [bun, "run", "tools/openspec.ts", "archive", change, "--yes"]);
