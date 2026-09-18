export const SHADCN_VUE_CLI_VERSION = "2.8.2";

export function buildShadcnVueArgv(args: readonly string[], bunExecutable: string): string[] {
  return [bunExecutable, "x", "--bun", `shadcn-vue@${SHADCN_VUE_CLI_VERSION}`, ...args];
}

export async function runShadcnVue(args: readonly string[]): Promise<number> {
  if (args.length === 0) {
    console.error("Usage: bun run tools/shadcn-vue.ts <command> [args...]");
    return 2;
  }

  const bunExecutable = Bun.which("bun") ?? "bun";
  const child = Bun.spawn(buildShadcnVueArgv(args, bunExecutable), {
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  return await child.exited;
}

if (import.meta.main) process.exitCode = await runShadcnVue(Bun.argv.slice(2));
