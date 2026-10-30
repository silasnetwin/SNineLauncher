export function markStartup(name: string): void {
  if (performance.getEntriesByName(name, "mark").length > 0) return;
  const mark = performance.mark(name);
  if (import.meta.env.DEV) console.info(`[startup] ${name}: ${mark.startTime.toFixed(1)} ms`);
}
