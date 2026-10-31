import { KeyValueList, SectionHeader } from "../../design-system/index.js";
import { useUiStore } from "../../stores/uiStore.js";

export function RightInspector() {
  const selected = useUiStore((state) => state.selectedEntity);
  const selectEntity = useUiStore((state) => state.selectEntity);

  return (
    <aside
      aria-label="Inspector"
      className="hidden w-72 shrink-0 flex-col gap-4 overflow-y-auto border-l border-white/10 bg-void-900/60 p-4 xl:flex"
    >
      <SectionHeader title="Inspector" subtitle="Contextual details" />
      {selected === null ? (
        <div className="glass-panel p-4 text-sm text-slate-400">
          <p className="font-medium text-slate-200">Nothing selected</p>
          <p className="mt-1">
            Select an agent, task, or evidence row to inspect it here.
          </p>
        </div>
      ) : (
        <div className="glass-panel p-4">
          <p className="text-xs uppercase tracking-wider text-slate-500">
            {selected.kind}
          </p>
          <h3 className="mt-1 font-display text-sm font-semibold">
            {selected.label}
          </h3>
          <p className="mt-1 text-sm text-slate-400">{selected.detail}</p>
          <div className="mt-3">
            <KeyValueList entries={[{ key: "id", value: selected.id }]} />
          </div>
          <button
            type="button"
            onClick={() => {
              selectEntity(null);
            }}
            className="mt-3 text-xs text-cyan-300 hover:underline"
          >
            Clear selection
          </button>
        </div>
      )}
      <div className="glass-panel p-4">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
          Next up
        </h3>
        <p className="mt-1 text-sm text-slate-400">
          Live policy evaluations and spatial topology dock here in later
          batches.
        </p>
      </div>
    </aside>
  );
}
