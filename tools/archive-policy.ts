export type OpenSpecStatus = {
  isComplete?: boolean;
  artifacts?: Array<{ id?: string; status?: string }>;
};

export function archiveReadinessGaps(status: OpenSpecStatus, tasks: string): string[] {
  const gaps: string[] = [];
  if (status.isComplete !== true) gaps.push("OpenSpec artifacts are incomplete");
  for (const artifact of status.artifacts ?? []) {
    if (artifact.status !== "done" && artifact.status !== "skipped") {
      gaps.push(`artifact '${artifact.id ?? "unknown"}' has status '${artifact.status ?? "unknown"}'`);
    }
  }
  if (/^\s*- \[ \]/m.test(tasks)) gaps.push("tasks.md contains unchecked tasks");
  return gaps;
}
