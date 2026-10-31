export function markStartup(name: string): void {
  if (performance.getEntriesByName(name, "mark").length > 0) return;
  const mark = performance.mark(name);
  if (import.meta.env.DEV) console.info(`[startup] ${name}: ${mark.startTime.toFixed(1)} ms`);
  if (name === "local_cache_loaded") {
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const splash = document.getElementById("startup-screen");
      if (!splash) return;
      splash.classList.add("leaving");
      window.setTimeout(() => splash.remove(), 180);
    }));
  }
}
