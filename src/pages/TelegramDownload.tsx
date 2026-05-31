import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  ArrowLeft, Settings, RefreshCw, Download, Folder, Loader2,
  CheckCircle2, AlertCircle, X, MessageCircle, Users, Radio,
  User, Image, Video, FileText, FolderSearch, Plus, Trash2,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";

// ─── Types ────────────────────────────────────────────────────

interface TgSettings { api_id: number; api_hash: string; }

interface TgDialogInfo {
  name: string;
  username: string | null;
  kind: "user" | "group" | "channel";
  packed: string;
}

interface TgProgress {
  task_id: string;
  current: number;
  file_name: string;
  status: "saving" | "done" | "error" | "scanning";
  message: string;
}

type AuthStatus = "checking" | "authenticated" | "unauthenticated";
type PhoneStep = "phone" | "code" | "2fa";
type DownloadPhase = "idle" | "running" | "done";

// ─── Helpers ──────────────────────────────────────────────────

const KIND_ICON: Record<string, React.ElementType> = {
  user: User,
  group: Users,
  channel: Radio,
};

// ─── Component ────────────────────────────────────────────────

export default function TelegramDownload() {
  const navigate = useNavigate();
  const [tab, setTab] = useState<"auth" | "download" | "cache">("auth");
  const [showSettings, setShowSettings] = useState(false);

  // ── Settings ────────────────────────────────────────────────
  const [settings, setSettings] = useState<TgSettings>({ api_id: 0, api_hash: "" });
  const [settingsDraft, setSettingsDraft] = useState<TgSettings>({ api_id: 0, api_hash: "" });
  const [settingsSaved, setSettingsSaved] = useState(false);
  const [showHash, setShowHash] = useState(false);

  // ── Auth ─────────────────────────────────────────────────────
  const [authStatus, setAuthStatus] = useState<AuthStatus>("checking");
  const [authUser, setAuthUser] = useState("");
  const [phoneStep, setPhoneStep] = useState<PhoneStep>("phone");
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [twoFaPassword, setTwoFaPassword] = useState("");
  const [authLoading, setAuthLoading] = useState(false);
  const [authError, setAuthError] = useState("");

  // ── Download ─────────────────────────────────────────────────
  const [dialogs, setDialogs] = useState<TgDialogInfo[]>([]);
  const [dialogsLoading, setDialogsLoading] = useState(false);
  const [selectedDialog, setSelectedDialog] = useState<TgDialogInfo | null>(null);
  const [mediaTypes, setMediaTypes] = useState<string[]>(["photo", "video"]);
  const [limit, setLimit] = useState(0);
  const [outputDir, setOutputDir] = useState("");
  const [dlPhase, setDlPhase] = useState<DownloadPhase>("idle");
  const [dlProgress, setDlProgress] = useState<TgProgress | null>(null);
  const [dlError, setDlError] = useState("");
  const dlTaskRef = useRef("");

  // ── Cache (扫描) ───────────────────────────────────────────────
  const [cacheSubTab, setCacheSubTab] = useState<"scan" | "watch">("watch");
  const [sourcePaths, setSourcePaths] = useState<string[]>([]);
  const [cacheOutputDir, setCacheOutputDir] = useState("");
  const [cachePhase, setCachePhase] = useState<DownloadPhase>("idle");
  const [cacheProgress, setCacheProgress] = useState<TgProgress | null>(null);
  const cacheTaskRef = useRef("");


  // ── Init ──────────────────────────────────────────────────────
  useEffect(() => {
    invoke<TgSettings>("tg_get_settings").then((s) => {
      setSettings(s);
      setSettingsDraft(s);
    });
    checkAuth();

    let unlisten: (() => void) | null = null;
    listen<TgProgress>("tg_progress", (e) => {
      const p = e.payload;
      if (p.task_id === dlTaskRef.current) {
        setDlProgress(p);
        if (p.status === "done") setDlPhase("done");
      }
      if (p.task_id === cacheTaskRef.current) {
        setCacheProgress(p);
        if (p.status === "done") setCachePhase("done");
      }
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, []);

  const checkAuth = async () => {
    setAuthStatus("checking");
    try {
      const ok = await invoke<boolean>("tg_is_authenticated");
      setAuthStatus(ok ? "authenticated" : "unauthenticated");
    } catch {
      setAuthStatus("unauthenticated");
    }
  };

  // ── Auth handlers ─────────────────────────────────────────────
  const handleRequestCode = async () => {
    if (!phone.trim()) return;
    setAuthLoading(true);
    setAuthError("");
    try {
      await invoke("tg_request_code", { phone: phone.trim() });
      setPhoneStep("code");
    } catch (e) { setAuthError(String(e)); }
    finally { setAuthLoading(false); }
  };

  const handleSignIn = async () => {
    setAuthLoading(true);
    setAuthError("");
    try {
      const name = await invoke<string>("tg_sign_in", {
        code: code.trim(),
        password: phoneStep === "2fa" ? twoFaPassword : "",
      });
      setAuthUser(name);
      setAuthStatus("authenticated");
      setPhoneStep("phone");
      setCode("");
      setTwoFaPassword("");
    } catch (e) {
      const msg = String(e);
      if (msg === "2FA_REQUIRED") {
        setPhoneStep("2fa");
        setAuthError("此账号已开启两步验证，请输入密码");
      } else {
        setAuthError(msg);
      }
    } finally { setAuthLoading(false); }
  };

  const handleSignOut = async () => {
    setAuthLoading(true);
    setAuthError("");
    try {
      await invoke("tg_sign_out");
      setAuthStatus("unauthenticated");
      setAuthUser("");
      setPhoneStep("phone");
      setDialogs([]);
      setSelectedDialog(null);
    } catch (e) { setAuthError(String(e)); }
    finally { setAuthLoading(false); }
  };

  // ── Settings handlers ─────────────────────────────────────────
  const handleSaveSettings = async () => {
    try {
      await invoke("tg_save_settings", {
        apiId: settingsDraft.api_id,
        apiHash: settingsDraft.api_hash,
      });
      setSettings(settingsDraft);
      setSettingsSaved(true);
      setTimeout(() => setSettingsSaved(false), 2000);
    } catch (e) { console.error(e); }
  };

  // ── Dialog handlers ───────────────────────────────────────────
  const handleLoadDialogs = async () => {
    setDialogsLoading(true);
    try {
      const list = await invoke<TgDialogInfo[]>("tg_get_dialogs");
      setDialogs(list);
    } catch (e) { setDlError(String(e)); }
    finally { setDialogsLoading(false); }
  };

  const toggleMediaType = (type: string) => {
    setMediaTypes((prev) =>
      prev.includes(type) ? prev.filter((t) => t !== type) : [...prev, type]
    );
  };

  const handlePickOutputDir = async () => {
    const dir = await invoke<string | null>("tg_pick_dir");
    if (dir) setOutputDir(dir);
  };

  const handleDownload = async () => {
    if (!selectedDialog || !outputDir) return;
    const taskId = `${Date.now()}`;
    dlTaskRef.current = taskId;
    setDlPhase("running");
    setDlProgress(null);
    setDlError("");
    try {
      await invoke("tg_download_media", {
        taskId,
        packedChat: selectedDialog.packed,
        mediaTypes,
        outputDir,
        limit,
      });
    } catch (e) {
      setDlError(String(e));
      setDlPhase("idle");
    }
  };

  // ── Cache handlers ─────────────────────────────────────────────
  const handleDetectPaths = async () => {
    const paths = await invoke<string[]>("tg_get_suggested_paths");
    setSourcePaths((prev) => {
      const merged = [...prev];
      for (const p of paths) {
        if (!merged.includes(p)) merged.push(p);
      }
      return merged;
    });
  };

  const handleAddPath = async () => {
    const dir = await invoke<string | null>("tg_pick_dir");
    if (dir && !sourcePaths.includes(dir)) setSourcePaths((p) => [...p, dir]);
  };

  const handlePickCacheOutput = async () => {
    const dir = await invoke<string | null>("tg_pick_dir");
    if (dir) setCacheOutputDir(dir);
  };

  const handleScanCache = async () => {
    if (!sourcePaths.length || !cacheOutputDir) return;
    const taskId = `cache_${Date.now()}`;
    cacheTaskRef.current = taskId;
    setCachePhase("running");
    setCacheProgress(null);
    try {
      await invoke("tg_scan_cache", {
        taskId,
        sourceDirs: sourcePaths,
        outputDir: cacheOutputDir,
      });
    } catch (e) {
      setCachePhase("idle");
    }
  };

  // ─────────────────────────────────────────────────────────────

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <header className="flex items-center justify-between border-b border-zinc-800 px-4 py-3">
        <div className="flex items-center gap-3">
          <button
            onClick={() => navigate("/toolbox")}
            className="flex items-center gap-1.5 text-sm text-zinc-400 hover:text-zinc-100 transition-colors"
          >
            <ArrowLeft size={15} /> 工具箱
          </button>
          <span className="text-zinc-600">/</span>
          <span className="text-sm text-zinc-100">Telegram 下载</span>
        </div>
        <div className="flex items-center gap-2">
          {authStatus === "authenticated" && (
            <span className="rounded-full bg-green-900/60 px-2 py-0.5 text-xs text-green-400">
              已登录{authUser ? ` · ${authUser}` : ""}
            </span>
          )}
          <button
            onClick={() => setShowSettings((v) => !v)}
            className={cn(
              "flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm transition-colors",
              showSettings
                ? "bg-zinc-700 text-zinc-100"
                : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
            )}
          >
            <Settings size={15} /> API 设置
          </button>
        </div>
      </header>

      <div className="flex flex-1 overflow-hidden">
        {/* Main */}
        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Tabs */}
          <div className="flex border-b border-zinc-800">
            {(["auth", "download", "cache"] as const).map((t) => {
              const labels = { auth: "登录认证", download: "批量下载", cache: "读取缓存" };
              return (
                <button
                  key={t}
                  onClick={() => setTab(t)}
                  className={cn(
                    "px-4 py-2.5 text-sm transition-colors border-b-2 -mb-px",
                    tab === t
                      ? "border-blue-500 text-zinc-100"
                      : "border-transparent text-zinc-400 hover:text-zinc-100"
                  )}
                >
                  {labels[t]}
                </button>
              );
            })}
          </div>

          {/* ── Auth tab ─────────────────────────────────────── */}
          {tab === "auth" && (
            <div className="flex flex-1 flex-col overflow-y-auto p-6 gap-5 max-w-md">
              {authStatus === "checking" ? (
                <div className="flex items-center gap-2 text-sm text-zinc-400">
                  <Loader2 size={15} className="animate-spin" /> 检查登录状态…
                </div>
              ) : authStatus === "authenticated" ? (
                <div className="flex flex-col gap-4">
                  <div className="flex items-center gap-3 rounded-xl border border-green-800 bg-green-950/30 p-4">
                    <CheckCircle2 size={20} className="text-green-400 shrink-0" />
                    <div>
                      <p className="text-sm font-medium text-zinc-100">已登录 Telegram</p>
                      {authUser && <p className="text-xs text-zinc-400 mt-0.5">{authUser}</p>}
                    </div>
                  </div>
                  <button
                    onClick={handleSignOut}
                    disabled={authLoading}
                    className="flex w-full items-center justify-center gap-2 rounded-lg border border-zinc-700 py-2 text-sm text-zinc-400 transition-colors hover:border-red-800 hover:text-red-400 disabled:opacity-50"
                  >
                    {authLoading ? <Loader2 size={14} className="animate-spin" /> : null}
                    退出登录
                  </button>
                </div>
              ) : (
                <div className="flex flex-col gap-4">
                  <p className="text-xs text-zinc-500 leading-relaxed">
                    使用 Telegram 账号（手机号）登录，可批量下载任意会话中的图片和视频。
                    API 凭据请前往{" "}
                    <span className="text-blue-400">my.telegram.org</span> 创建应用获取。
                  </p>

                  {settings.api_id === 0 && (
                    <div className="flex items-start gap-2 rounded-lg border border-yellow-800 bg-yellow-950/30 px-3 py-2.5 text-xs text-yellow-400">
                      <AlertCircle size={14} className="mt-0.5 shrink-0" />
                      请先在右侧 API 设置中填写 API ID 和 API Hash
                    </div>
                  )}

                  {/* Phone */}
                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium text-zinc-400">手机号（含国家区号）</label>
                    <div className="flex gap-2">
                      <input
                        type="tel"
                        value={phone}
                        onChange={(e) => setPhone(e.target.value)}
                        disabled={phoneStep !== "phone" || authLoading}
                        placeholder="+86 13800138000"
                        className="flex-1 rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 outline-none focus:border-blue-500 disabled:opacity-50"
                      />
                      {phoneStep === "phone" && (
                        <button
                          onClick={handleRequestCode}
                          disabled={!phone.trim() || authLoading || settings.api_id === 0}
                          className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          {authLoading ? <Loader2 size={14} className="animate-spin" /> : null}
                          获取验证码
                        </button>
                      )}
                      {phoneStep !== "phone" && (
                        <button
                          onClick={() => { setPhoneStep("phone"); setCode(""); setTwoFaPassword(""); setAuthError(""); }}
                          className="rounded-lg border border-zinc-700 px-3 py-2 text-xs text-zinc-400 hover:border-zinc-500 hover:text-zinc-100 transition-colors"
                        >
                          重新获取
                        </button>
                      )}
                    </div>
                  </div>

                  {/* Code + 2FA */}
                  {(phoneStep === "code" || phoneStep === "2fa") && (
                    <>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-zinc-400">验证码</label>
                        <input
                          type="text"
                          value={code}
                          onChange={(e) => setCode(e.target.value)}
                          disabled={phoneStep === "2fa" || authLoading}
                          placeholder="Telegram 发送的 5 位验证码"
                          maxLength={6}
                          className="rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 outline-none focus:border-blue-500 disabled:opacity-50"
                        />
                      </div>

                      {phoneStep === "2fa" && (
                        <div className="flex flex-col gap-1.5">
                          <label className="text-xs font-medium text-zinc-400">两步验证密码</label>
                          <input
                            type="password"
                            value={twoFaPassword}
                            onChange={(e) => setTwoFaPassword(e.target.value)}
                            placeholder="你的两步验证密码"
                            className="rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 outline-none focus:border-blue-500"
                          />
                        </div>
                      )}

                      <button
                        onClick={handleSignIn}
                        disabled={
                          authLoading ||
                          (phoneStep === "code" && !code.trim()) ||
                          (phoneStep === "2fa" && !twoFaPassword.trim())
                        }
                        className="flex items-center justify-center gap-2 rounded-lg bg-blue-600 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
                      >
                        {authLoading && <Loader2 size={14} className="animate-spin" />}
                        {phoneStep === "2fa" ? "验证密码" : "验证登录"}
                      </button>
                    </>
                  )}

                  {/* Auth error */}
                  {authError && authError !== "2FA_REQUIRED" && (
                    <div className="flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/50 px-3 py-2.5 text-xs text-red-400">
                      <AlertCircle size={13} className="mt-0.5 shrink-0" />
                      <span className="flex-1">{authError}</span>
                      <button onClick={() => setAuthError("")}><X size={12} /></button>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}

          {/* ── Download tab ──────────────────────────────────── */}
          {tab === "download" && (
            <div className="flex flex-1 overflow-hidden">
              {/* Dialog list */}
              <div className="flex w-72 shrink-0 flex-col border-r border-zinc-800">
                <div className="flex items-center justify-between border-b border-zinc-800 px-3 py-2">
                  <span className="text-xs text-zinc-400">会话列表</span>
                  <button
                    onClick={handleLoadDialogs}
                    disabled={dialogsLoading || authStatus !== "authenticated"}
                    className="flex items-center gap-1 text-xs text-zinc-400 hover:text-zinc-100 transition-colors disabled:opacity-40"
                  >
                    <RefreshCw size={12} className={dialogsLoading ? "animate-spin" : ""} />
                    刷新
                  </button>
                </div>

                {authStatus !== "authenticated" ? (
                  <div className="flex flex-1 flex-col items-center justify-center gap-2 text-zinc-600">
                    <MessageCircle size={28} />
                    <p className="text-xs">请先完成登录认证</p>
                  </div>
                ) : dialogs.length === 0 ? (
                  <div className="flex flex-1 flex-col items-center justify-center gap-2 text-zinc-600">
                    <MessageCircle size={28} />
                    <p className="text-xs">点击刷新加载会话</p>
                  </div>
                ) : (
                  <div className="flex-1 overflow-y-auto">
                    {dialogs.map((d) => {
                      const Icon = KIND_ICON[d.kind] ?? MessageCircle;
                      return (
                        <button
                          key={d.packed}
                          onClick={() => setSelectedDialog(d)}
                          className={cn(
                            "flex w-full items-center gap-2.5 px-3 py-2.5 text-left transition-colors",
                            selectedDialog?.packed === d.packed
                              ? "bg-blue-600/20 text-zinc-100"
                              : "text-zinc-300 hover:bg-zinc-800/60"
                          )}
                        >
                          <Icon size={14} className="shrink-0 text-zinc-500" />
                          <div className="min-w-0 flex-1">
                            <p className="truncate text-xs font-medium">{d.name}</p>
                            {d.username && (
                              <p className="truncate text-xs text-zinc-500">@{d.username}</p>
                            )}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>

              {/* Download controls */}
              <div className="flex flex-1 flex-col overflow-y-auto p-5 gap-4">
                {!selectedDialog ? (
                  <div className="flex flex-1 flex-col items-center justify-center gap-2 text-zinc-600">
                    <Download size={36} />
                    <p className="text-sm">从左侧选择一个会话</p>
                  </div>
                ) : (
                  <>
                    <div className="rounded-xl border border-zinc-800 bg-zinc-900 px-4 py-3 flex items-center gap-2.5">
                      {(() => {
                        const Icon = KIND_ICON[selectedDialog.kind] ?? MessageCircle;
                        return <Icon size={16} className="text-blue-400 shrink-0" />;
                      })()}
                      <div>
                        <p className="text-sm font-medium text-zinc-100">{selectedDialog.name}</p>
                        {selectedDialog.username && (
                          <p className="text-xs text-zinc-500">@{selectedDialog.username}</p>
                        )}
                      </div>
                    </div>

                    {/* Media type filter */}
                    <div className="flex flex-col gap-2">
                      <label className="text-xs font-medium text-zinc-400">下载类型</label>
                      <div className="flex gap-2">
                        {[
                          { key: "photo", icon: Image, label: "图片" },
                          { key: "video", icon: Video, label: "视频" },
                          { key: "document", icon: FileText, label: "文档" },
                        ].map(({ key, icon: Icon, label }) => (
                          <button
                            key={key}
                            onClick={() => toggleMediaType(key)}
                            className={cn(
                              "flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs transition-colors",
                              mediaTypes.includes(key)
                                ? "border-blue-600 bg-blue-600/20 text-blue-300"
                                : "border-zinc-700 text-zinc-400 hover:border-zinc-500 hover:text-zinc-100"
                            )}
                          >
                            <Icon size={12} />
                            {label}
                          </button>
                        ))}
                      </div>
                    </div>

                    {/* Message limit */}
                    <div className="flex flex-col gap-1.5">
                      <label className="text-xs font-medium text-zinc-400">
                        消息数量限制（0 = 不限）
                      </label>
                      <input
                        type="number"
                        min={0}
                        value={limit}
                        onChange={(e) => setLimit(Math.max(0, Number(e.target.value)))}
                        className="w-40 rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 outline-none focus:border-blue-500"
                      />
                    </div>

                    {/* Output dir */}
                    <div className="flex flex-col gap-1.5">
                      <label className="text-xs font-medium text-zinc-400">保存目录</label>
                      <div className="flex gap-2">
                        <input
                          type="text"
                          value={outputDir}
                          onChange={(e) => setOutputDir(e.target.value)}
                          placeholder="选择或输入保存路径"
                          className="min-w-0 flex-1 rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 outline-none focus:border-blue-500"
                        />
                        <button
                          onClick={handlePickOutputDir}
                          className="flex shrink-0 items-center gap-1 rounded-lg border border-zinc-700 px-2.5 py-2 text-xs text-zinc-400 transition-colors hover:border-zinc-500 hover:text-zinc-100"
                        >
                          <Folder size={13} />
                        </button>
                      </div>
                    </div>

                    <button
                      onClick={handleDownload}
                      disabled={dlPhase === "running" || !outputDir || mediaTypes.length === 0}
                      className="flex items-center gap-2 self-start rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      {dlPhase === "running" ? (
                        <Loader2 size={14} className="animate-spin" />
                      ) : (
                        <Download size={14} />
                      )}
                      {dlPhase === "running" ? "下载中…" : "开始下载"}
                    </button>

                    {dlError && (
                      <div className="flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/50 px-3 py-2.5 text-xs text-red-400">
                        <AlertCircle size={13} className="mt-0.5 shrink-0" />
                        <span className="flex-1">{dlError}</span>
                        <button onClick={() => setDlError("")}><X size={12} /></button>
                      </div>
                    )}

                    {(dlPhase === "running" || dlPhase === "done") && dlProgress && (
                      <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4 flex flex-col gap-2">
                        {dlPhase === "done" ? (
                          <div className="flex items-center gap-2">
                            <CheckCircle2 size={16} className="text-green-400 shrink-0" />
                            <div className="flex-1 min-w-0">
                              <p className="text-sm font-medium text-zinc-100">{dlProgress.message}</p>
                            </div>
                            {outputDir && (
                              <button
                                onClick={() => openPath(outputDir)}
                                className="flex items-center gap-1 rounded-lg border border-zinc-700 px-2.5 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-500"
                              >
                                <Folder size={12} /> 打开目录
                              </button>
                            )}
                          </div>
                        ) : (
                          <>
                            <div className="flex items-center gap-2 text-xs text-zinc-400">
                              <Loader2 size={12} className="animate-spin shrink-0" />
                              <span className="truncate">
                                {dlProgress.status === "scanning" ? "扫描中…" : `正在保存: ${dlProgress.file_name}`}
                              </span>
                            </div>
                            <p className="text-xs text-zinc-500">已下载 {dlProgress.current} 个文件</p>
                            <div className="h-1 rounded-full bg-zinc-800">
                              <div className="h-full w-full rounded-full bg-blue-500 animate-pulse" />
                            </div>
                          </>
                        )}
                      </div>
                    )}
                  </>
                )}
              </div>
            </div>
          )}

          {/* ── Cache tab ─────────────────────────────────────── */}
          {tab === "cache" && (
            <div className="flex flex-1 flex-col overflow-hidden">
              {/* Sub-tabs */}
              <div className="flex border-b border-zinc-800 px-4">
                {(["watch", "scan"] as const).map((t) => {
                  const labels = { watch: "实时监控", scan: "扫描复制" };
                  return (
                    <button
                      key={t}
                      onClick={() => setCacheSubTab(t)}
                      className={cn(
                        "flex items-center gap-1.5 px-3 py-2 text-xs transition-colors border-b-2 -mb-px",
                        cacheSubTab === t
                          ? "border-blue-500 text-zinc-100"
                          : "border-transparent text-zinc-400 hover:text-zinc-100"
                      )}
                    >
                      {labels[t]}
                      {t === "watch" && (
                        <span className="rounded bg-zinc-700 px-1 py-0.5 text-[10px] text-zinc-400">
                          暂不可用
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>

              {/* ─ 实时监控 sub-tab ─ */}
              {cacheSubTab === "watch" && (
                <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center">
                  <AlertCircle size={32} className="text-zinc-600" />
                  <p className="text-sm font-medium text-zinc-400">实时监控暂不可用</p>
                  <p className="max-w-sm text-xs leading-relaxed text-zinc-600">
                    文件系统监控功能存在兼容性问题，尚未稳定。
                    请使用「扫描复制」模式手动扫描
                    <span className="text-zinc-400"> %AppData%\Telegram Desktop\tdata\user_data </span>
                    提取已缓存的媒体文件。
                  </p>
                </div>
              )}

              {/* ─ 扫描复制 sub-tab ─ */}
              {cacheSubTab === "scan" && (
                <div className="flex flex-1 flex-col overflow-y-auto p-5 gap-4 max-w-lg">
                  <p className="text-xs text-zinc-500 leading-relaxed">
                    一次性扫描目录，将找到的图片和视频复制到指定位置。
                  </p>

                  <div className="flex flex-col gap-2">
                    <div className="flex items-center justify-between">
                      <label className="text-xs font-medium text-zinc-400">扫描来源目录</label>
                      <div className="flex gap-1.5">
                        <button onClick={handleDetectPaths} className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100 transition-colors">
                          <FolderSearch size={12} /> 自动检测
                        </button>
                        <button onClick={handleAddPath} className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100 transition-colors">
                          <Plus size={12} /> 添加
                        </button>
                      </div>
                    </div>
                    {sourcePaths.length === 0 ? (
                      <div className="rounded-lg border border-dashed border-zinc-700 py-5 text-center text-xs text-zinc-600">
                        点击「自动检测」或「添加」选择目录
                      </div>
                    ) : (
                      <div className="flex flex-col gap-1">
                        {sourcePaths.map((p) => (
                          <div key={p} className="flex items-center gap-2 rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2">
                            <Folder size={13} className="shrink-0 text-zinc-500" />
                            <span className="flex-1 truncate text-xs text-zinc-300" title={p}>{p}</span>
                            <button onClick={() => setSourcePaths((prev) => prev.filter((x) => x !== p))} className="text-zinc-600 hover:text-red-400 transition-colors">
                              <Trash2 size={12} />
                            </button>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>

                  <div className="flex flex-col gap-1.5">
                    <label className="text-xs font-medium text-zinc-400">保存目录</label>
                    <div className="flex gap-2">
                      <input type="text" value={cacheOutputDir} onChange={(e) => setCacheOutputDir(e.target.value)}
                        placeholder="选择或输入保存路径"
                        className="min-w-0 flex-1 rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 outline-none focus:border-blue-500" />
                      <button onClick={handlePickCacheOutput} className="flex shrink-0 items-center gap-1 rounded-lg border border-zinc-700 px-2.5 py-2 text-xs text-zinc-400 transition-colors hover:border-zinc-500 hover:text-zinc-100">
                        <Folder size={13} />
                      </button>
                    </div>
                  </div>

                  <button onClick={handleScanCache}
                    disabled={cachePhase === "running" || !sourcePaths.length || !cacheOutputDir}
                    className="flex items-center gap-2 self-start rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50">
                    {cachePhase === "running" ? <Loader2 size={14} className="animate-spin" /> : <FolderSearch size={14} />}
                    {cachePhase === "running" ? "扫描中…" : "开始扫描并复制"}
                  </button>

                  {(cachePhase === "running" || cachePhase === "done") && cacheProgress && (
                    <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4 flex flex-col gap-2">
                      {cachePhase === "done" ? (
                        <div className="flex items-center gap-2">
                          <CheckCircle2 size={16} className="text-green-400 shrink-0" />
                          <p className="flex-1 text-sm font-medium text-zinc-100">{cacheProgress.message}</p>
                          {cacheOutputDir && (
                            <button onClick={() => openPath(cacheOutputDir)} className="flex items-center gap-1 rounded-lg border border-zinc-700 px-2.5 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-500">
                              <Folder size={12} /> 打开
                            </button>
                          )}
                        </div>
                      ) : (
                        <>
                          <div className="flex items-center gap-2 text-xs text-zinc-400">
                            <Loader2 size={12} className="animate-spin shrink-0" />
                            <span className="truncate">{cacheProgress.status === "scanning" ? `扫描: ${cacheProgress.file_name}` : `复制: ${cacheProgress.file_name}`}</span>
                          </div>
                          <p className="text-xs text-zinc-500">已复制 {cacheProgress.current} 个文件</p>
                          <div className="h-1 rounded-full bg-zinc-800"><div className="h-full w-full rounded-full bg-blue-500 animate-pulse" /></div>
                        </>
                      )}
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>

        {/* API Settings sidebar */}
        {showSettings && (
          <aside className="flex w-72 shrink-0 flex-col gap-5 overflow-y-auto border-l border-zinc-800 p-5">
            <h2 className="text-sm font-semibold text-zinc-100">API 设置</h2>
            <p className="text-xs text-zinc-500 leading-relaxed -mt-3">
              前往 <span className="text-blue-400">my.telegram.org</span> → API development tools 创建应用，获取以下凭据。
            </p>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-zinc-400">API ID</label>
              <input
                type="number"
                value={settingsDraft.api_id || ""}
                onChange={(e) => setSettingsDraft((d) => ({ ...d, api_id: Number(e.target.value) }))}
                placeholder="12345678"
                className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-600 outline-none focus:border-blue-500"
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-zinc-400">API Hash</label>
              <div className="relative">
                <input
                  type={showHash ? "text" : "password"}
                  value={settingsDraft.api_hash}
                  onChange={(e) => setSettingsDraft((d) => ({ ...d, api_hash: e.target.value }))}
                  placeholder="xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
                  className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 pr-14 text-xs text-zinc-100 placeholder-zinc-600 outline-none focus:border-blue-500"
                />
                <button
                  onClick={() => setShowHash((v) => !v)}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-zinc-500 hover:text-zinc-300"
                >
                  {showHash ? "隐藏" : "显示"}
                </button>
              </div>
            </div>
            <button
              onClick={handleSaveSettings}
              className={cn(
                "mt-auto w-full rounded-lg py-2 text-sm font-medium transition-colors",
                settingsSaved
                  ? "bg-green-700 text-green-100"
                  : "bg-blue-600 text-white hover:bg-blue-500"
              )}
            >
              {settingsSaved ? "已保存 ✓" : "保存设置"}
            </button>
          </aside>
        )}
      </div>
    </div>
  );
}
