import { useState, useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import {
  ArrowLeft, Search, Loader2, AlertCircle, Tag, ArrowDownUp,
  Crown, Ban, Gift, Clock, Globe, Check, Heart,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { cn } from "@/lib/utils";

// ─── Types ────────────────────────────────────────────────────

interface AppSearchResult {
  appid: number;
  name: string;
  icon: string | null;
}

interface RegionPrice {
  cc: string;
  region_name: string;
  flag: string;
  status: "ok" | "free" | "unavailable" | "comingsoon" | "error";
  currency: string | null;
  initial_formatted: string | null;
  final_formatted: string | null;
  discount_percent: number;
  final_cny: number | null;
}

interface PriceQueryResult {
  appid: number;
  name: string;
  header_image: string | null;
  regions: RegionPrice[];
  fx_available: boolean;
}

// ─── 状态徽章 ──────────────────────────────────────────────────

function StatusBadge({ r }: { r: RegionPrice }) {
  if (r.status === "ok") {
    if (r.discount_percent > 0) {
      // 由现价人民币反推原价，估算立省金额（折扣明细悬停显示）
      const saveCny =
        r.final_cny != null && r.discount_percent < 100
          ? r.final_cny / (1 - r.discount_percent / 100) - r.final_cny
          : null;
      const tip =
        `原价 ${r.initial_formatted ?? "—"} → 现价 ${r.final_formatted ?? "—"}` +
        (saveCny != null ? `\n约省 ¥${saveCny.toFixed(2)}` : "") +
        `\n（Steam 官方接口不提供折扣截止时间）`;
      return (
        <span
          title={tip}
          className="inline-flex cursor-help items-center gap-1 rounded bg-green-600/20 px-1.5 py-0.5 text-xs font-medium text-green-400"
        >
          <Tag size={11} /> -{r.discount_percent}%
        </span>
      );
    }
    return <span className="text-xs text-zinc-600">原价</span>;
  }
  if (r.status === "free") {
    return (
      <span className="inline-flex items-center gap-1 rounded bg-blue-600/20 px-1.5 py-0.5 text-xs text-blue-400">
        <Gift size={11} /> 免费
      </span>
    );
  }
  if (r.status === "comingsoon") {
    return (
      <span className="inline-flex items-center gap-1 rounded bg-amber-600/20 px-1.5 py-0.5 text-xs text-amber-400">
        <Clock size={11} /> 未发售
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1 rounded bg-red-600/15 px-1.5 py-0.5 text-xs text-red-400/80">
      <Ban size={11} /> 不可购买
    </span>
  );
}

// ─── Component ────────────────────────────────────────────────

export default function SteamPrice() {
  const navigate = useNavigate();
  const [term, setTerm] = useState("");
  const [searching, setSearching] = useState(false);
  const [suggestions, setSuggestions] = useState<AppSearchResult[]>([]);
  const [showSug, setShowSug] = useState(false);
  const [querying, setQuerying] = useState(false);
  const [result, setResult] = useState<PriceQueryResult | null>(null);
  const [error, setError] = useState("");
  const [sortByPrice, setSortByPrice] = useState(false);
  // 已加入愿望单的区域（按 cc）；正在添加的 cc
  const [added, setAdded] = useState<Set<string>>(new Set());
  const [addingCc, setAddingCc] = useState<string | null>(null);

  const addToWishlist = async (cc: string) => {
    if (!result) return;
    setAddingCc(cc);
    setError("");
    try {
      await invoke("wishlist_add", { input: String(result.appid), region: cc, targetCny: null });
      setAdded((prev) => new Set(prev).add(cc));
    } catch (e) {
      setError(String(e));
    } finally {
      setAddingCc(null);
    }
  };

  // 代理设置
  const [proxyMode, setProxyMode] = useState<"system" | "none" | "manual">("system");
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxyNotice, setProxyNotice] = useState("");

  // 初始加载代理配置
  useEffect(() => {
    (async () => {
      try {
        const conf = await invoke<{ mode: string; url: string }>("steam_price_get_proxy");
        setProxyMode((conf.mode as "system" | "none" | "manual") || "system");
        setProxyUrl(conf.url || "");
      } catch { /* ignore */ }
    })();
  }, []);

  const saveProxy = async (mode: "system" | "none" | "manual", url: string) => {
    try {
      await invoke("steam_price_set_proxy", { mode, url });
      setProxyMode(mode);
      setProxyUrl(url);
      setProxyNotice("已应用");
      setTimeout(() => setProxyNotice(""), 1500);
    } catch (e) {
      setError(String(e));
    }
  };

  const pickMode = (mode: "system" | "none" | "manual") => {
    if (mode === "manual") {
      setProxyMode("manual"); // 仅切换 UI，需填地址后点保存
    } else {
      saveProxy(mode, "");
    }
  };

  // 防止旧请求覆盖新请求的结果
  const reqSeq = useRef(0);
  // 选中建议后抑制下一次防抖搜索（避免把游戏名又当关键词搜一遍）
  const skipNext = useRef(false);
  const boxRef = useRef<HTMLDivElement>(null);

  // 输入即搜（防抖 300ms），实时给出带缩略图的下拉建议
  useEffect(() => {
    const t = term.trim();
    if (skipNext.current) {
      skipNext.current = false;
      return;
    }
    if (t.length < 2) {
      setSuggestions([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    const seq = ++reqSeq.current;
    const timer = setTimeout(async () => {
      try {
        const list = await invoke<AppSearchResult[]>("steam_price_search", { term: t });
        if (seq !== reqSeq.current) return; // 已有更新的请求，丢弃旧结果
        setSuggestions(list);
        setShowSug(true);
      } catch {
        if (seq === reqSeq.current) setSuggestions([]);
      } finally {
        if (seq === reqSeq.current) setSearching(false);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [term]);

  // 点击页面其他位置关闭下拉
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) {
        setShowSug(false);
      }
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);

  const pick = async (app: AppSearchResult) => {
    skipNext.current = true;
    setTerm(app.name);
    setShowSug(false);
    setSuggestions([]);
    await doQuery(app);
  };

  // 回车：优先选第一个建议；否则按当前输入直接查（支持 appid / 链接）
  const onEnter = async () => {
    if (suggestions.length > 0) {
      await pick(suggestions[0]);
      return;
    }
    const t = term.trim();
    if (!t) return;
    try {
      const list = await invoke<AppSearchResult[]>("steam_price_search", { term: t });
      if (list.length > 0) await pick(list[0]);
      else setError("未找到匹配的游戏，换个关键词或直接输入 appid / 商店链接");
    } catch (e) {
      setError(String(e));
    }
  };

  const doQuery = async (app: AppSearchResult) => {
    setQuerying(true);
    setError("");
    setResult(null);
    setSortByPrice(false);
    setAdded(new Set());
    try {
      const res = await invoke<PriceQueryResult>("steam_price_query", { appid: app.appid });
      setResult(res);
    } catch (e) {
      setError(String(e));
    } finally {
      setQuerying(false);
    }
  };

  // 排序后的区域列表
  const regions = (() => {
    if (!result) return [];
    if (!sortByPrice) return result.regions;
    const rank = (r: RegionPrice) =>
      r.status === "free" ? -1 : r.final_cny ?? Number.POSITIVE_INFINITY;
    return [...result.regions].sort((a, b) => rank(a) - rank(b));
  })();

  // 找出最便宜的可购买区域（用于高亮）
  const cheapestCc = (() => {
    if (!result) return null;
    let best: RegionPrice | null = null;
    for (const r of result.regions) {
      if (r.status !== "ok" || r.final_cny == null) continue;
      if (!best || (best.final_cny ?? Infinity) > r.final_cny) best = r;
    }
    return best?.cc ?? null;
  })();

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <header className="flex items-center justify-between border-b border-zinc-800 px-4 py-3">
        <div className="flex items-center gap-3">
          <button
            onClick={() => navigate("/toolbox")}
            className="flex items-center gap-1.5 text-sm text-zinc-400 transition-colors hover:text-zinc-100"
          >
            <ArrowLeft size={15} /> 工具箱
          </button>
          <span className="text-zinc-600">/</span>
          <span className="text-sm text-zinc-100">Steam 区域价格</span>
        </div>
      </header>

      <div className="flex flex-1 flex-col overflow-y-auto p-6">
        {/* 网络 / 代理设置 */}
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <span className="flex items-center gap-1.5 text-xs text-zinc-500">
            <Globe size={13} /> 网络
          </span>
          {([
            ["system", "系统代理"],
            ["none", "直连"],
            ["manual", "手动"],
          ] as const).map(([m, label]) => (
            <button
              key={m}
              onClick={() => pickMode(m)}
              className={cn(
                "rounded-md border px-2.5 py-1 text-xs transition-colors",
                proxyMode === m
                  ? "border-blue-600 bg-blue-600/15 text-blue-400"
                  : "border-zinc-700 text-zinc-400 hover:border-zinc-500 hover:text-zinc-100"
              )}
            >
              {label}
            </button>
          ))}
          {proxyMode === "manual" && (
            <div className="flex items-center gap-2">
              <input
                value={proxyUrl}
                onChange={(e) => setProxyUrl(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && saveProxy("manual", proxyUrl)}
                placeholder="http://127.0.0.1:7890"
                className="w-52 rounded-md border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-xs text-zinc-100 placeholder:text-zinc-600 focus:border-zinc-600 focus:outline-none"
              />
              <button
                onClick={() => saveProxy("manual", proxyUrl)}
                className="rounded-md bg-blue-600 px-2.5 py-1 text-xs font-medium text-white transition-colors hover:bg-blue-500"
              >
                保存
              </button>
            </div>
          )}
          {proxyNotice && (
            <span className="flex items-center gap-1 text-xs text-green-400">
              <Check size={12} /> {proxyNotice}
            </span>
          )}
        </div>

        {/* 搜索框 + 实时下拉建议 */}
        <div ref={boxRef} className="relative">
          <Search
            size={16}
            className="absolute left-3 top-[19px] -translate-y-1/2 text-zinc-500"
          />
          <input
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            onFocus={() => suggestions.length > 0 && setShowSug(true)}
            onKeyDown={(e) => {
              if (e.key === "Enter") { setShowSug(false); onEnter(); }
              else if (e.key === "Escape") setShowSug(false);
            }}
            placeholder="输入游戏名实时搜索，也可直接填 appid / 商店链接"
            className="w-full rounded-lg border border-zinc-800 bg-zinc-900 py-2.5 pl-9 pr-9 text-sm text-zinc-100 placeholder:text-zinc-600 focus:border-zinc-600 focus:outline-none"
          />
          {searching && (
            <Loader2 size={15} className="absolute right-3 top-[19px] -translate-y-1/2 animate-spin text-zinc-500" />
          )}

          {/* 下拉建议 */}
          {showSug && suggestions.length > 0 && (
            <div className="absolute z-20 mt-1.5 max-h-[360px] w-full overflow-y-auto rounded-lg border border-zinc-700 bg-zinc-900 shadow-xl shadow-black/40">
              {suggestions.map((s) => (
                <button
                  key={s.appid}
                  onClick={() => pick(s)}
                  className="flex w-full items-center gap-3 border-b border-zinc-800/60 p-2 text-left transition-colors last:border-0 hover:bg-zinc-800"
                >
                  {s.icon ? (
                    <img src={s.icon} alt="" className="h-9 w-[80px] shrink-0 rounded object-cover" />
                  ) : (
                    <div className="h-9 w-[80px] shrink-0 rounded bg-zinc-800" />
                  )}
                  <span className="flex-1 truncate text-sm text-zinc-100">{s.name}</span>
                  <span className="shrink-0 text-xs text-zinc-600">{s.appid}</span>
                </button>
              ))}
            </div>
          )}
        </div>

        {error && (
          <div className="mt-4 flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/50 px-4 py-3 text-sm text-red-400">
            <AlertCircle size={15} className="mt-0.5 shrink-0" />
            <span className="flex-1">{error}</span>
          </div>
        )}

        {/* 查询中 */}
        {querying && (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-500">
            <Loader2 size={32} className="animate-spin" />
            <p className="text-sm">正在查询各区域价格…</p>
          </div>
        )}

        {/* 结果 */}
        {result && !querying && (
          <div className="mt-5 flex flex-col">
            {/* 游戏头部 */}
            <div className="flex items-center gap-4">
              {result.header_image && (
                <img
                  src={result.header_image}
                  alt=""
                  className="h-[88px] w-[188px] shrink-0 rounded-lg object-cover"
                />
              )}
              <div className="min-w-0">
                <h2 className="text-lg font-semibold text-zinc-100">{result.name}</h2>
                <p className="mt-1 text-xs text-zinc-600">appid {result.appid}</p>
              </div>
            </div>

            {/* 工具条 */}
            <div className="mt-5 flex items-center justify-between">
              <p className="text-xs text-zinc-500">
                共 {result.regions.length} 个区域
                {!result.fx_available && "（汇率获取失败，人民币估算不可用）"}
              </p>
              <button
                onClick={() => setSortByPrice((v) => !v)}
                className={cn(
                  "flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs transition-colors",
                  sortByPrice
                    ? "border-blue-600 text-blue-400"
                    : "border-zinc-700 text-zinc-400 hover:border-zinc-500 hover:text-zinc-100"
                )}
              >
                <ArrowDownUp size={13} />
                {sortByPrice ? "按区域顺序" : "按价格排序"}
              </button>
            </div>

            {/* 价格表 */}
            <div className="mt-3 overflow-hidden rounded-xl border border-zinc-800">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-zinc-800 bg-zinc-900/60 text-left text-xs text-zinc-500">
                    <th className="px-4 py-2.5 font-medium">区域</th>
                    <th className="px-4 py-2.5 font-medium">价格</th>
                    <th className="px-4 py-2.5 font-medium">状态</th>
                    <th className="px-4 py-2.5 text-right font-medium">约合人民币</th>
                    <th className="px-4 py-2.5 text-right font-medium"></th>
                  </tr>
                </thead>
                <tbody>
                  {regions.map((r) => {
                    const isCheapest = r.cc === cheapestCc;
                    return (
                      <tr
                        key={r.cc}
                        className={cn(
                          "border-b border-zinc-800/60 last:border-0",
                          isCheapest ? "bg-green-950/30" : "hover:bg-zinc-900/40"
                        )}
                      >
                        <td className="px-4 py-2.5">
                          <div className="flex items-center gap-2">
                            <span className="text-base">{r.flag}</span>
                            <span className="text-zinc-200">{r.region_name}</span>
                            {isCheapest && (
                              <span title="最低价" className="text-green-400">
                                <Crown size={13} />
                              </span>
                            )}
                          </div>
                        </td>
                        <td className="px-4 py-2.5">
                          {r.status === "ok" ? (
                            <div className="flex items-baseline gap-2">
                              <span className="font-medium text-zinc-100">{r.final_formatted}</span>
                              {r.discount_percent > 0 && r.initial_formatted && (
                                <span className="text-xs text-zinc-600 line-through">
                                  {r.initial_formatted}
                                </span>
                              )}
                            </div>
                          ) : (
                            <span className="text-zinc-600">—</span>
                          )}
                        </td>
                        <td className="px-4 py-2.5">
                          <StatusBadge r={r} />
                        </td>
                        <td className="px-4 py-2.5 text-right">
                          {r.status === "free" ? (
                            <span className="text-blue-400">¥0</span>
                          ) : r.final_cny != null ? (
                            <span className={cn("tabular-nums", isCheapest ? "text-green-400" : "text-zinc-400")}>
                              ¥{r.final_cny.toFixed(2)}
                            </span>
                          ) : (
                            <span className="text-zinc-700">—</span>
                          )}
                        </td>
                        <td className="px-4 py-2.5 text-right">
                          {(r.status === "ok" || r.status === "free") && (
                            added.has(r.cc) ? (
                              <span className="inline-flex items-center gap-1 text-xs text-green-400">
                                <Check size={13} /> 已添加
                              </span>
                            ) : (
                              <button
                                onClick={() => addToWishlist(r.cc)}
                                disabled={addingCc === r.cc}
                                title="添加到心愿单追踪"
                                className="inline-flex items-center gap-1 rounded-md border border-zinc-700 px-2 py-1 text-xs text-zinc-300 transition-colors hover:border-blue-600 hover:text-blue-400 disabled:opacity-40"
                              >
                                {addingCc === r.cc ? <Loader2 size={12} className="animate-spin" /> : <Heart size={12} />}
                                加入心愿单
                              </button>
                            )
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>

            <p className="mt-3 text-xs leading-relaxed text-zinc-600">
              说明：「不可购买」表示该区商店未上架/区域限制销售；本工具基于 Steam 官方商店 API，
              不含「可购买但激活/游玩锁区」这类需 SteamDB 的激活限制数据。人民币为按实时汇率换算的估算值（鼠标悬停折扣徽章可看约省金额），仅供对比参考。
              折扣截止时间与历史折扣时间官方接口不提供，故未展示。
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
