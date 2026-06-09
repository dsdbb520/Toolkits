import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import {
  ArrowLeft, Plus, Loader2, AlertCircle, Tag, RefreshCw, Trash2,
  ExternalLink, Globe, Check, Target, TrendingDown, Pencil, Cloud,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";

// ─── Types（对应 wishlist.rs 的 WishItem）─────────────────────

interface HistPoint { t: number; cny: number | null; }
interface WishItem {
  id: string;
  platform: "steam" | "ps" | "ns";
  region: string;
  product_key: string;
  title: string;
  image: string | null;
  store_url: string;
  target_cny: number | null;
  created_at: number;
  status: "ok" | "free" | "unavailable" | "error";
  currency: string | null;
  final_formatted: string | null;
  initial_formatted: string | null;
  discount_percent: number;
  final_cny: number | null;
  checked_at: number;
  low_cny: number | null;
  history: HistPoint[];
  hit_target: boolean;
  unseen_drop: boolean;
}

// ─── 平台 / 区域展示 ──────────────────────────────────────────

const PLATFORM_LABEL: Record<string, string> = { steam: "Steam", ps: "PlayStation", ns: "Switch" };
const PLATFORM_COLOR: Record<string, string> = {
  steam: "bg-sky-600/20 text-sky-400",
  ps: "bg-blue-600/20 text-blue-400",
  ns: "bg-red-600/20 text-red-400",
};

// 常见区域（用于 Steam 选区 + 卡片旗帜显示）
const REGIONS: { cc: string; name: string; flag: string }[] = [
  { cc: "CN", name: "中国", flag: "🇨🇳" }, { cc: "US", name: "美国", flag: "🇺🇸" },
  { cc: "AR", name: "阿根廷", flag: "🇦🇷" }, { cc: "TR", name: "土耳其", flag: "🇹🇷" },
  { cc: "HK", name: "香港", flag: "🇭🇰" }, { cc: "JP", name: "日本", flag: "🇯🇵" },
  { cc: "TW", name: "台湾", flag: "🇹🇼" }, { cc: "RU", name: "俄罗斯", flag: "🇷🇺" },
  { cc: "GB", name: "英国", flag: "🇬🇧" }, { cc: "DE", name: "德国", flag: "🇩🇪" },
  { cc: "AU", name: "澳大利亚", flag: "🇦🇺" }, { cc: "NZ", name: "新西兰", flag: "🇳🇿" },
  { cc: "KR", name: "韩国", flag: "🇰🇷" }, { cc: "BR", name: "巴西", flag: "🇧🇷" },
  { cc: "IN", name: "印度", flag: "🇮🇳" },
];
const regionOf = (cc: string) => REGIONS.find((r) => r.cc === cc) ?? { cc, name: cc, flag: "🏳️" };

// ─── 迷你走势图 ───────────────────────────────────────────────

function Sparkline({ history }: { history: HistPoint[] }) {
  const pts = history.filter((p) => p.cny != null) as { t: number; cny: number }[];
  if (pts.length < 2) return null;
  const w = 120, h = 28, pad = 2;
  const xs = pts.map((p) => p.t), ys = pts.map((p) => p.cny);
  const minX = Math.min(...xs), maxX = Math.max(...xs);
  const minY = Math.min(...ys), maxY = Math.max(...ys);
  const nx = (x: number) => (maxX === minX ? pad : pad + ((x - minX) / (maxX - minX)) * (w - 2 * pad));
  const ny = (y: number) => (maxY === minY ? h / 2 : h - pad - ((y - minY) / (maxY - minY)) * (h - 2 * pad));
  const d = pts.map((p, i) => `${i === 0 ? "M" : "L"}${nx(p.t).toFixed(1)},${ny(p.cny).toFixed(1)}`).join(" ");
  return (
    <svg width={w} height={h} className="overflow-visible">
      <path d={d} fill="none" stroke="currentColor" strokeWidth="1.5" className="text-zinc-500" />
    </svg>
  );
}

// ─── 组件 ──────────────────────────────────────────────────────

export default function Wishlist() {
  const navigate = useNavigate();
  const [items, setItems] = useState<WishItem[]>([]);
  const [input, setInput] = useState("");
  const [region, setRegion] = useState(""); // 空=自动(PS/NS按链接, Steam默认CN)
  const [target, setTarget] = useState("");
  const [adding, setAdding] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState<{ id: string; value: string } | null>(null);

  // 代理设置（与 Steam 价格共用）
  const [proxyMode, setProxyMode] = useState<"system" | "none" | "manual">("system");
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxyNotice, setProxyNotice] = useState("");

  useEffect(() => {
    (async () => {
      try {
        const conf = await invoke<{ mode: string; url: string }>("steam_price_get_proxy");
        setProxyMode((conf.mode as "system" | "none" | "manual") || "system");
        setProxyUrl(conf.url || "");
      } catch { /* ignore */ }
      try {
        setItems(await invoke<WishItem[]>("wishlist_list"));
        // 进入页面即视为已查看降价，清掉工具箱红点（本次仍展示降价标记）
        await invoke("wishlist_mark_seen");
      } catch { /* ignore */ }
    })();
  }, []);

  const sync = async () => {
    setSyncing(true); setError("");
    try {
      setItems(await invoke<WishItem[]>("wishlist_sync"));
      await invoke("wishlist_mark_seen");
    } catch (e) {
      setError(String(e));
    } finally {
      setSyncing(false);
    }
  };

  const saveProxy = async (mode: "system" | "none" | "manual", url: string) => {
    try {
      await invoke("steam_price_set_proxy", { mode, url });
      setProxyMode(mode); setProxyUrl(url);
      setProxyNotice("已应用"); setTimeout(() => setProxyNotice(""), 1500);
    } catch (e) { setError(String(e)); }
  };
  const pickMode = (mode: "system" | "none" | "manual") =>
    mode === "manual" ? setProxyMode("manual") : saveProxy(mode, "");

  const add = async () => {
    const v = input.trim();
    if (!v) return;
    setAdding(true); setError("");
    try {
      const t = target.trim() ? parseFloat(target.trim()) : null;
      const item = await invoke<WishItem>("wishlist_add", {
        input: v, region, targetCny: Number.isFinite(t as number) ? t : null,
      });
      setItems((prev) => [item, ...prev]);
      setInput(""); setTarget("");
    } catch (e) {
      setError(String(e));
    } finally {
      setAdding(false);
    }
  };

  const refresh = async () => {
    setRefreshing(true); setError("");
    try {
      setItems(await invoke<WishItem[]>("wishlist_refresh"));
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  };

  const remove = async (id: string) => {
    try {
      await invoke("wishlist_remove", { id });
      setItems((prev) => prev.filter((i) => i.id !== id));
    } catch (e) { setError(String(e)); }
  };

  const saveTitle = async () => {
    if (!editing) return;
    const v = editing.value.trim();
    const { id } = editing;
    setEditing(null);
    if (!v) return;
    try {
      await invoke("wishlist_set_title", { id, title: v });
      setItems((prev) => prev.map((i) => (i.id === id ? { ...i, title: v } : i)));
    } catch (e) { setError(String(e)); }
  };

  const setItemTarget = async (id: string, raw: string) => {
    const t = raw.trim() ? parseFloat(raw.trim()) : null;
    const val = Number.isFinite(t as number) ? t : null;
    try {
      await invoke("wishlist_set_target", { id, targetCny: val });
      setItems((prev) =>
        prev.map((i) => (i.id === id ? { ...i, target_cny: val, hit_target: val != null && i.final_cny != null && i.final_cny <= val } : i))
      );
    } catch (e) { setError(String(e)); }
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-zinc-800 px-4 py-3">
        <div className="flex items-center gap-3">
          <button onClick={() => navigate("/toolbox")} className="flex items-center gap-1.5 text-sm text-zinc-400 transition-colors hover:text-zinc-100">
            <ArrowLeft size={15} /> 工具箱
          </button>
          <span className="text-zinc-600">/</span>
          <span className="text-sm text-zinc-100">心愿单 · 降价追踪</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={sync}
            disabled={syncing}
            title="重新取价并与服务器同步（跨设备 + 降价提醒）"
            className="flex items-center gap-1.5 rounded-lg border border-zinc-700 px-3 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-500 hover:text-zinc-100 disabled:opacity-40"
          >
            <Cloud size={13} className={cn(syncing && "animate-pulse")} /> 云同步
          </button>
          <button
            onClick={refresh}
            disabled={refreshing || items.length === 0}
            className="flex items-center gap-1.5 rounded-lg border border-zinc-700 px-3 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-500 hover:text-zinc-100 disabled:opacity-40"
          >
            <RefreshCw size={13} className={cn(refreshing && "animate-spin")} /> 刷新价格
          </button>
        </div>
      </header>

      <div className="flex flex-1 flex-col overflow-y-auto p-6">
        {/* 网络 / 代理 */}
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <span className="flex items-center gap-1.5 text-xs text-zinc-500"><Globe size={13} /> 网络</span>
          {([["system", "系统代理"], ["none", "直连"], ["manual", "手动"]] as const).map(([m, label]) => (
            <button key={m} onClick={() => pickMode(m)}
              className={cn("rounded-md border px-2.5 py-1 text-xs transition-colors",
                proxyMode === m ? "border-blue-600 bg-blue-600/15 text-blue-400" : "border-zinc-700 text-zinc-400 hover:border-zinc-500 hover:text-zinc-100")}>
              {label}
            </button>
          ))}
          {proxyMode === "manual" && (
            <div className="flex items-center gap-2">
              <input value={proxyUrl} onChange={(e) => setProxyUrl(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && saveProxy("manual", proxyUrl)}
                placeholder="http://127.0.0.1:7890"
                className="w-52 rounded-md border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-xs text-zinc-100 placeholder:text-zinc-600 focus:border-zinc-600 focus:outline-none" />
              <button onClick={() => saveProxy("manual", proxyUrl)} className="rounded-md bg-blue-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-blue-500">保存</button>
            </div>
          )}
          {proxyNotice && <span className="flex items-center gap-1 text-xs text-green-400"><Check size={12} /> {proxyNotice}</span>}
        </div>

        {/* 添加栏 */}
        <div className="flex flex-wrap items-center gap-2 rounded-xl border border-zinc-800 bg-zinc-900/40 p-3">
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && add()}
            placeholder="粘贴 Steam / PlayStation / Nintendo 商店链接（或 Steam appid）"
            className="min-w-[280px] flex-1 rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-600 focus:border-zinc-600 focus:outline-none"
          />
          <select
            value={region}
            onChange={(e) => setRegion(e.target.value)}
            title="区域：Steam 必选；PS/NS 留「自动」按链接识别"
            className="rounded-lg border border-zinc-800 bg-zinc-900 px-2.5 py-2 text-sm text-zinc-300 focus:border-zinc-600 focus:outline-none"
          >
            <option value="">自动 / Steam默认</option>
            {REGIONS.map((r) => <option key={r.cc} value={r.cc}>{r.flag} {r.name}</option>)}
          </select>
          <div className="relative">
            <Target size={13} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-500" />
            <input
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
              placeholder="目标价¥"
              className="w-28 rounded-lg border border-zinc-800 bg-zinc-900 py-2 pl-8 pr-2 text-sm text-zinc-100 placeholder:text-zinc-600 focus:border-zinc-600 focus:outline-none"
            />
          </div>
          <button onClick={add} disabled={adding || !input.trim()}
            className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-3.5 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:opacity-40">
            {adding ? <Loader2 size={15} className="animate-spin" /> : <Plus size={15} />} 添加
          </button>
        </div>

        {error && (
          <div className="mt-3 flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/50 px-4 py-3 text-sm text-red-400">
            <AlertCircle size={15} className="mt-0.5 shrink-0" />
            <span className="flex-1">{error}</span>
          </div>
        )}

        {/* 列表 */}
        {items.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 text-zinc-600">
            <Target size={30} />
            <p className="text-sm">还没有追踪的游戏。粘贴一个商店链接开始吧。</p>
            <p className="text-xs text-zinc-700">历史价格从你加入这天起，每次刷新记录一次，逐步攒出走势。</p>
          </div>
        ) : (
          <div className="mt-4 grid gap-3">
            {items.map((it) => {
              const reg = regionOf(it.region);
              const onSale = it.status === "ok" && it.discount_percent > 0;
              return (
                <div key={it.id}
                  className={cn("flex items-center gap-4 rounded-xl border bg-zinc-900/40 p-3",
                    it.hit_target ? "border-green-700/60 bg-green-950/20" : "border-zinc-800")}>
                  {it.image
                    ? <img src={it.image} alt="" className="h-16 w-28 shrink-0 rounded-lg object-cover" />
                    : <div className="h-16 w-28 shrink-0 rounded-lg bg-zinc-800" />}

                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className={cn("shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium", PLATFORM_COLOR[it.platform])}>
                        {PLATFORM_LABEL[it.platform]}
                      </span>
                      <span className="shrink-0 text-xs text-zinc-500">{reg.flag} {reg.name}</span>
                      {it.hit_target && (
                        <span className="flex shrink-0 items-center gap-1 rounded bg-green-600/20 px-1.5 py-0.5 text-[10px] font-medium text-green-400">
                          <TrendingDown size={10} /> 达到目标价
                        </span>
                      )}
                      {it.unseen_drop && (
                        <span className="flex shrink-0 items-center gap-1 rounded bg-amber-500/20 px-1.5 py-0.5 text-[10px] font-medium text-amber-400">
                          <TrendingDown size={10} /> 降价了
                        </span>
                      )}
                    </div>
                    {editing?.id === it.id ? (
                      <input
                        autoFocus
                        value={editing.value}
                        onChange={(e) => setEditing({ id: it.id, value: e.target.value })}
                        onBlur={saveTitle}
                        onKeyDown={(e) => { if (e.key === "Enter") saveTitle(); else if (e.key === "Escape") setEditing(null); }}
                        className="mt-1 w-full rounded border border-zinc-700 bg-zinc-900 px-1.5 py-0.5 text-sm font-medium text-zinc-100 focus:border-zinc-500 focus:outline-none"
                      />
                    ) : (
                      <div className="group mt-1 flex items-center gap-1.5">
                        <p className="truncate text-sm font-medium text-zinc-100" title={it.title}>{it.title}</p>
                        <button
                          onClick={() => setEditing({ id: it.id, value: it.title })}
                          title="重命名"
                          className="shrink-0 text-zinc-600 opacity-0 transition-opacity hover:text-zinc-300 group-hover:opacity-100"
                        >
                          <Pencil size={12} />
                        </button>
                      </div>
                    )}

                    <div className="mt-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-1">
                      {it.status === "ok" ? (
                        <>
                          <span className="text-base font-semibold text-zinc-100">{it.final_formatted}</span>
                          {onSale && it.initial_formatted && (
                            <span className="text-xs text-zinc-600 line-through">{it.initial_formatted}</span>
                          )}
                          {onSale && (
                            <span className="flex items-center gap-1 rounded bg-green-600/20 px-1.5 py-0.5 text-xs font-medium text-green-400">
                              <Tag size={11} /> -{it.discount_percent}%
                            </span>
                          )}
                          {it.final_cny != null && <span className="text-xs text-zinc-500">≈¥{it.final_cny.toFixed(2)}</span>}
                        </>
                      ) : it.status === "free" ? (
                        <span className="text-sm text-blue-400">免费</span>
                      ) : (
                        <span className="text-sm text-zinc-600">暂不可购买 / 未取到价</span>
                      )}
                      {it.low_cny != null && (
                        <span className="text-xs text-zinc-600">史低 ¥{it.low_cny.toFixed(2)}</span>
                      )}
                    </div>
                  </div>

                  <div className="shrink-0 text-zinc-500"><Sparkline history={it.history} /></div>

                  <div className="flex shrink-0 flex-col items-end gap-2">
                    <div className="relative">
                      <Target size={12} className="absolute left-2 top-1/2 -translate-y-1/2 text-zinc-600" />
                      <input
                        defaultValue={it.target_cny ?? ""}
                        onBlur={(e) => setItemTarget(it.id, e.target.value)}
                        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                        placeholder="目标¥"
                        className="w-24 rounded-md border border-zinc-800 bg-zinc-900 py-1 pl-7 pr-1.5 text-xs text-zinc-100 placeholder:text-zinc-600 focus:border-zinc-600 focus:outline-none"
                      />
                    </div>
                    <div className="flex items-center gap-1">
                      <button onClick={() => openUrl(it.store_url)} title="打开商店页"
                        className="rounded-md border border-zinc-800 p-1.5 text-zinc-400 transition-colors hover:border-zinc-600 hover:text-zinc-100">
                        <ExternalLink size={13} />
                      </button>
                      <button onClick={() => remove(it.id)} title="移除"
                        className="rounded-md border border-zinc-800 p-1.5 text-zinc-400 transition-colors hover:border-red-700 hover:text-red-400">
                        <Trash2 size={13} />
                      </button>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {items.length > 0 && (
          <p className="mt-4 text-xs leading-relaxed text-zinc-600">
            说明：用确切商品 ID 取价，可靠不乱配。历史价格从加入心愿单起、每次「刷新价格」记录一次快照逐步积累，「史低」为我们记录到的最低人民币价。
            设了目标价后，当前价 ≤ 目标价会高亮（后续可接系统通知做降价提醒）。人民币为按实时汇率换算的估算值。
          </p>
        )}
      </div>
    </div>
  );
}
