import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  ArrowLeft, Gamepad2, RefreshCw, Copy, Check, Folder,
  LogIn, Loader2, AlertCircle, Star, User as UserIcon, Play,
  UserPlus, Eye, EyeOff,
} from "lucide-react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";

// ─── Types ────────────────────────────────────────────────────

interface SteamAccount {
  steamid64: string;
  steamid32: string;
  account_name: string;
  persona_name: string;
  avatar: string | null;
  most_recent: boolean;
  remember_password: boolean;
  is_current: boolean;
  timestamp: number;
}

// ─── Helpers ──────────────────────────────────────────────────

function fmtDate(secs: number): string {
  if (!secs) return "—";
  return new Date(secs * 1000).toLocaleString("zh-CN", {
    year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit",
  });
}

// ─── 添加账号对话框 ────────────────────────────────────────────

function AddAccountDialog({
  onClose,
  onSuccess,
}: {
  onClose: () => void;
  onSuccess: () => void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPwd, setShowPwd] = useState(false);
  const [adding, setAdding] = useState(false);
  const [err, setErr] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { inputRef.current?.focus(); }, []);

  const handleAdd = async () => {
    if (!username.trim() || !password) return;
    setAdding(true);
    setErr("");
    try {
      await invoke("steam_add_account", { username: username.trim(), password });
      onSuccess();
      onClose();
    } catch (e) {
      setErr(String(e));
    } finally {
      setAdding(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="w-80 rounded-xl border border-zinc-700 bg-zinc-900 p-5 shadow-2xl">
        <h2 className="mb-4 flex items-center gap-2 text-base font-semibold text-zinc-100">
          <UserPlus size={16} className="text-blue-400" /> 添加新账号
        </h2>

        {err && (
          <div className="mb-3 flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/50 px-3 py-2 text-xs text-red-400">
            <AlertCircle size={13} className="mt-0.5 shrink-0" />
            <span>{err}</span>
          </div>
        )}

        <div className="flex flex-col gap-3">
          <div>
            <label className="mb-1 block text-xs text-zinc-400">账号名</label>
            <input
              ref={inputRef}
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="Steam 登录名"
              autoComplete="username"
              onKeyDown={(e) => e.key === "Enter" && inputRef.current?.blur()}
              className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 outline-none placeholder:text-zinc-600 focus:border-blue-500"
            />
          </div>
          <div>
            <label className="mb-1 block text-xs text-zinc-400">密码</label>
            <div className="relative">
              <input
                type={showPwd ? "text" : "password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Steam 密码"
                autoComplete="current-password"
                onKeyDown={(e) => e.key === "Enter" && handleAdd()}
                className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 pr-9 text-sm text-zinc-100 outline-none placeholder:text-zinc-600 focus:border-blue-500"
              />
              <button
                type="button"
                tabIndex={-1}
                onClick={() => setShowPwd((v) => !v)}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-zinc-500 hover:text-zinc-300"
              >
                {showPwd ? <EyeOff size={14} /> : <Eye size={14} />}
              </button>
            </div>
          </div>
          <p className="text-xs text-zinc-600">
            Steam 将在首次登录后自动记住此账号，之后可从列表一键切换，无需再次输入密码。
          </p>
        </div>

        <div className="mt-4 flex gap-2">
          <button
            onClick={onClose}
            disabled={adding}
            className="flex-1 rounded-lg border border-zinc-700 px-3 py-2 text-sm text-zinc-300 transition-colors hover:border-zinc-500 hover:text-zinc-100 disabled:opacity-50"
          >
            取消
          </button>
          <button
            onClick={handleAdd}
            disabled={adding || !username.trim() || !password}
            className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-blue-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {adding ? (
              <><Loader2 size={14} className="animate-spin" /> 登录中…</>
            ) : (
              <><LogIn size={14} /> 登录</>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── 可复制的字段 ──────────────────────────────────────────────

function CopyField({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch { /* ignore */ }
  };
  return (
    <div className="flex items-center gap-2">
      <span className="w-20 shrink-0 text-xs text-zinc-500">{label}</span>
      <code className="flex-1 truncate font-mono text-xs text-zinc-300" title={value}>{value}</code>
      <button
        onClick={copy}
        title="复制"
        className="rounded p-1 text-zinc-500 transition-colors hover:bg-zinc-800 hover:text-zinc-100"
      >
        {copied ? <Check size={13} className="text-green-400" /> : <Copy size={13} />}
      </button>
    </div>
  );
}

// ─── Component ────────────────────────────────────────────────

export default function SteamSwitch() {
  const navigate = useNavigate();
  const [accounts, setAccounts] = useState<SteamAccount[]>([]);
  const [steamPath, setSteamPath] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [switching, setSwitching] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const [showAddDialog, setShowAddDialog] = useState(false);

  const load = async () => {
    setLoading(true);
    setError("");
    try {
      const path = await invoke<string>("steam_get_path");
      setSteamPath(path);
      const list = await invoke<SteamAccount[]>("steam_get_accounts");
      setAccounts(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, []);

  const handleSwitch = async (acc: SteamAccount) => {
    if (acc.is_current) return;
    const running = await invoke<boolean>("steam_is_running");
    const tip = running
      ? `Steam 正在运行，切换将先关闭 Steam，再以「${acc.persona_name || acc.account_name}」重新登录。继续？`
      : `切换到「${acc.persona_name || acc.account_name}」并启动 Steam？`;
    if (!window.confirm(tip)) return;

    setSwitching(acc.account_name);
    setNotice("");
    setError("");
    try {
      await invoke("steam_switch_account", { accountName: acc.account_name });
      setNotice(`已切换到 ${acc.persona_name || acc.account_name}，Steam 正在启动…`);
      // 注册表已更新，刷新当前账号标记
      setTimeout(load, 1500);
    } catch (e) {
      setError(String(e));
    } finally {
      setSwitching(null);
    }
  };

  const handleLaunch = async () => {
    setError("");
    try {
      await invoke("steam_launch");
      setNotice("Steam 正在启动…");
    } catch (e) {
      setError(String(e));
    }
  };

  const handleOpenUserdata = async (acc: SteamAccount) => {
    try {
      const path = await invoke<string>("steam_get_userdata_path", { steamid32: acc.steamid32 });
      await openPath(path);
    } catch (e) {
      setError(String(e));
    }
  };

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
          <span className="text-sm text-zinc-100">Steam 切换</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowAddDialog(true)}
            disabled={loading}
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-zinc-400 transition-colors hover:bg-zinc-800 hover:text-zinc-100 disabled:opacity-50"
          >
            <UserPlus size={15} /> 添加账号
          </button>
          <button
            onClick={load}
            disabled={loading}
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-zinc-400 transition-colors hover:bg-zinc-800 hover:text-zinc-100 disabled:opacity-50"
          >
            <RefreshCw size={15} className={cn(loading && "animate-spin")} /> 刷新
          </button>
        </div>
      </header>

      <div className="flex flex-1 flex-col overflow-y-auto p-6">
        {/* Steam 路径 */}
        {steamPath && (
          <p className="mb-4 truncate text-xs text-zinc-600" title={steamPath}>
            Steam 路径：{steamPath}
          </p>
        )}

        {/* 提示 */}
        {notice && (
          <div className="mb-4 flex items-center gap-2 rounded-lg border border-green-800 bg-green-950/40 px-4 py-3 text-sm text-green-400">
            <Check size={15} className="shrink-0" /> {notice}
          </div>
        )}
        {error && (
          <div className="mb-4 flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/50 px-4 py-3 text-sm text-red-400">
            <AlertCircle size={15} className="mt-0.5 shrink-0" />
            <span className="flex-1">{error}</span>
          </div>
        )}

        {/* 加载 / 空 / 列表 */}
        {loading ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-500">
            <Loader2 size={32} className="animate-spin" />
            <p className="text-sm">读取账号中…</p>
          </div>
        ) : !error && accounts.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-600">
            <Gamepad2 size={40} />
            <p className="text-sm">未找到已记住的 Steam 账号</p>
            <p className="text-xs">请先在 Steam 中勾选「记住我的密码」登录一次</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
            {accounts.map((acc) => (
              <div
                key={acc.steamid64}
                className={cn(
                  "flex flex-col gap-3 rounded-xl border bg-zinc-900 p-4 transition-colors",
                  acc.is_current ? "border-blue-600" : "border-zinc-800"
                )}
              >
                {/* 头部：头像 + 名称 */}
                <div className="flex items-center gap-3">
                  {acc.avatar ? (
                    <img
                      src={convertFileSrc(acc.avatar)}
                      alt=""
                      className="h-16 w-16 shrink-0 rounded-lg object-cover"
                    />
                  ) : (
                    <div className="flex h-16 w-16 shrink-0 items-center justify-center rounded-lg bg-zinc-800 text-zinc-600">
                      <UserIcon size={28} />
                    </div>
                  )}
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <div className="flex items-center gap-2">
                      <p className="truncate text-base font-medium text-zinc-100" title={acc.persona_name}>
                        {acc.persona_name || "(未知昵称)"}
                      </p>
                      {acc.is_current && (
                        <span className="shrink-0 rounded-full bg-blue-600/20 px-2 py-0.5 text-xs text-blue-400">
                          当前
                        </span>
                      )}
                      {acc.most_recent && !acc.is_current && (
                        <span className="flex shrink-0 items-center gap-0.5 rounded-full bg-zinc-800 px-2 py-0.5 text-xs text-zinc-400">
                          <Star size={10} /> 最近
                        </span>
                      )}
                    </div>
                    <p className="truncate text-xs text-zinc-500" title={acc.account_name}>
                      登录名：{acc.account_name}
                    </p>
                    <p className="text-xs text-zinc-600">上次登录：{fmtDate(acc.timestamp)}</p>
                  </div>
                </div>

                {/* ID 字段 */}
                <div className="flex flex-col gap-1.5 rounded-lg bg-zinc-950/50 px-3 py-2">
                  <CopyField label="SteamID64" value={acc.steamid64} />
                  <CopyField label="SteamID32" value={acc.steamid32} />
                </div>

                {/* 操作 */}
                <div className="flex gap-2">
                  {acc.is_current ? (
                    <button
                      onClick={handleLaunch}
                      disabled={switching !== null}
                      className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-blue-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      <Play size={14} /> 启动 Steam
                    </button>
                  ) : (
                    <button
                      onClick={() => handleSwitch(acc)}
                      disabled={switching !== null}
                      className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-zinc-700 px-3 py-2 text-sm font-medium text-zinc-100 transition-colors hover:bg-zinc-600 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      {switching === acc.account_name ? (
                        <><Loader2 size={14} className="animate-spin" /> 切换中…</>
                      ) : (
                        <><LogIn size={14} /> 切换到此账号</>
                      )}
                    </button>
                  )}
                  <button
                    onClick={() => handleOpenUserdata(acc)}
                    title="打开 userdata 文件夹"
                    className="flex items-center gap-1.5 rounded-lg border border-zinc-700 px-3 py-2 text-sm text-zinc-300 transition-colors hover:border-zinc-500 hover:text-zinc-100"
                  >
                    <Folder size={14} /> userdata
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {showAddDialog && (
        <AddAccountDialog
          onClose={() => setShowAddDialog(false)}
          onSuccess={() => {
            setNotice("Steam 正在以新账号启动，请在 Steam 中完成登录后返回刷新列表…");
            setTimeout(load, 10000);
          }}
        />
      )}
    </div>
  );
}
