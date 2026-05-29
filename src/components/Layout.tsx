import { NavLink, Outlet } from "react-router-dom";
import { LayoutGrid } from "lucide-react";
import { cn } from "@/lib/utils";

const railItems = [
  { to: "/toolbox", icon: LayoutGrid, label: "工具箱" },
];

export default function Layout() {
  return (
    <div className="flex h-full w-full bg-zinc-950 text-zinc-100">
      {/* Icon rail */}
      <nav className="flex w-12 flex-col items-center gap-1 border-r border-zinc-800 bg-zinc-900 py-3">
        {railItems.map(({ to, icon: Icon, label }) => (
          <NavLink
            key={to}
            to={to}
            title={label}
            className={({ isActive }) =>
              cn(
                "group relative flex h-10 w-10 items-center justify-center rounded-lg transition-colors",
                isActive
                  ? "bg-zinc-700 text-white"
                  : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
              )
            }
          >
            <Icon size={20} />
            <span className="pointer-events-none absolute left-12 z-50 whitespace-nowrap rounded-md bg-zinc-800 px-2 py-1 text-xs text-zinc-100 opacity-0 shadow-lg transition-opacity group-hover:opacity-100">
              {label}
            </span>
          </NavLink>
        ))}
      </nav>

      {/* Content */}
      <main className="flex flex-1 flex-col overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
