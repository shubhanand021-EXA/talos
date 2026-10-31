export function Spinner({ label = "Loading" }: { label?: string }) {
  return (
    <span
      role="status"
      className="inline-flex items-center gap-2 text-sm text-slate-400"
    >
      <span
        aria-hidden="true"
        className="inline-block size-4 animate-spin rounded-full border-2 border-cyan-300 border-t-transparent"
      />
      {label}…
    </span>
  );
}
