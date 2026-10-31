export interface KeyValueEntry {
  key: string;
  value: string;
}

export function KeyValueList({ entries }: { entries: KeyValueEntry[] }) {
  return (
    <dl className="divide-y divide-white/5">
      {entries.map((entry) => (
        <div
          key={entry.key}
          className="flex items-baseline justify-between gap-4 py-2"
        >
          <dt className="shrink-0 text-xs font-medium uppercase tracking-wider text-slate-500">
            {entry.key}
          </dt>
          <dd
            className="truncate text-right font-mono text-xs text-slate-200"
            title={entry.value}
          >
            {entry.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
