const version = "1.13.1";
const bun = Bun.which("bun") ?? "bun";
const child = Bun.spawn(
  [bun, "x", "--bun", `@fission-ai/openspec@${version}`, ...Bun.argv.slice(2)],
  { cwd: process.cwd(), stdin: "inherit", stdout: "inherit", stderr: "inherit" },
);

process.exitCode = await child.exited;
