/** Format an ISO timestamp as HH:MM:SS (UTC). Returns "—" for invalid input. */
export function formatTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return "—";
  }
  return date.toISOString().slice(11, 19);
}

/** Format seconds as a compact uptime string, e.g. 3661 -> "1h 1m". */
export function formatUptime(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) {
    return "—";
  }
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  const seconds = Math.floor(totalSeconds % 60);
  if (minutes > 0) {
    return `${minutes}m ${seconds}s`;
  }
  return `${seconds}s`;
}

/** Truncate an identifier for dense UI surfaces. */
export function shortId(id: string, length = 8): string {
  if (id.length <= length) {
    return id;
  }
  return `${id.slice(0, length)}…`;
}
