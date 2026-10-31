import { NavLink } from "react-router-dom";
import { cn } from "../../lib/cn.js";
import { GLYPHS, NAV_ITEMS } from "./nav.js";

export function LeftRail() {
  return (
    <nav
      aria-label="Primary"
      className="flex w-16 shrink-0 flex-col items-center gap-1 border-r border-white/10 bg-void-900/60 py-3 lg:w-48 lg:items-stretch lg:px-3"
    >
      {NAV_ITEMS.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          end={item.to === "/"}
          title={item.hint}
          className={({ isActive }) =>
            cn(
              "flex w-11 flex-col items-center gap-1 rounded-control px-2 py-2.5 text-[11px] text-slate-400 transition-colors hover:bg-white/5 hover:text-slate-100 lg:w-full lg:flex-row lg:gap-3 lg:px-3 lg:text-sm",
              isActive &&
                "border border-cyan-300/30 bg-cyan-400/10 text-cyan-100",
              !isActive && "border border-transparent",
            )
          }
        >
          <span aria-hidden="true" className="text-base leading-none">
            {GLYPHS[item.label] ?? "•"}
          </span>
          <span className="lg:inline">{item.label}</span>
        </NavLink>
      ))}
    </nav>
  );
}
