import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  ArrowLeft, Download, Music, Folder, Loader2, CheckCircle2,
  AlertCircle, Settings, X, AlertTriangle, Clock, Trash2, Film,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";

// ─── Types ────────────────────────────────────────────────────

interface BiliSettings {
  sessdata: string;
  download_dir: string;
  default_quality: number;
}

interface VideoPart { cid: number; page: number; part: string; }

interface VideoInfo {
  bvid: string; title: string; cover: string; parts: VideoPart[];
}

interface PlayInfo {
  quality: number; accept_quality: number[]; accept_description: string[];
}

interface DownloadRecord {
  id: string; bvid: string; title: string; cover: string; part: string;
  quality: number; mode: string; file_path: string; file_size: number; created_at: number;
}

interface ProgressPayload {
  task_id: string; downloaded: number; total: number;
  status: "downloading" | "merging" | "converting" | "done" | "error";
  phase: "video" | "audio" | "flv" | ""; message: string;
}

// ─── Helpers ──────────────────────────────────────────────────

function fmtSize(bytes: number): string {
  if (bytes <= 0) return "—";
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function fmtDate(ms: number): string {
  return new Date(ms).toLocaleString("zh-CN", {
    month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit",
  });
}

const QUALITY_NAME: Record<number, string> = {
  127: "8K", 120: "4K", 116: "1080P 60帧", 112: "1080P+",
  80: "1080P", 74: "720P 60帧", 64: "720P", 32: "480P", 16: "360P",
};

const PHASE_LABEL: Record<string, string> = {
  video: "下载视频流", audio: "下载音频流", flv: "下载中", "": "处理中",
};
const STATUS_LABEL: Record<string, string> = {
  merging: "合并视频与音频…", converting: "转换为 MP3…",
};

function needsDash(qn: number) { return qn >= 112; }

type DownloadMode = "video_mp4" | "audio_mp3";
type Phase = "idle" | "fetching" | "ready" | "downloading" | "done";

// ─── Component ────────────────────────────────────────────────

export default function BiliDownload() {
  const navigate = useNavigate();
  const [tab, setTab] = useState<"download" | "history">("download");
  const [showSettings, setShowSettings] = useState(false);

  // ffmpeg
  const [ffmpegVersion, setFfmpegVersion] = useState<string | null | undefined>(undefined);

  // Settings
  const [settings, setSettings] = useState<BiliSettings>({ sessdata: "", download_dir: "", default_quality: 80 });
  const [settingsDraft, setSettingsDraft] = useState<BiliSettings>({ sessdata: "", download_dir: "", default_quality: 80 });
  const [settingsSaved, setSettingsSaved] = useState(false);
  const [showSessdata, setShowSessdata] = useState(false);

  // Download state
  const [inputUrl, setInputUrl] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState("");
  const [videoInfo, setVideoInfo] = useState<VideoInfo | null>(null);
  const [playInfo, setPlayInfo] = useState<PlayInfo | null>(null);
  const [selectedPart, setSelectedPart] = useState<VideoPart | null>(null);
  const [selectedQuality, setSelectedQuality] = useState(80);
  const [mode, setMode] = useState<DownloadMode>("video_mp4");
  const [progressPayload, setProgressPayload] = useState<ProgressPayload | null>(null);
  const [donePath, setDonePath] = useState("");

  // History
  const [history, setHistory] = useState<DownloadRecord[]>([]);

  const taskIdRef = useRef("");

  const loadHistory = () =>
    invoke<DownloadRecord[]>("bili_get_history").then(setHistory);

  useEffect(() => {
    invoke<BiliSettings>("bili_get_settings").then((s) => {
      setSettings(s); setSettingsDraft(s); setSelectedQuality(s.default_quality);
    });
    invoke<string | null>("bili_check_ffmpeg").then(setFfmpegVersion);
    loadHistory();

    let unlisten: (() => void) | null = null;
    listen<ProgressPayload>("bili_progress", (e) => {
      if (e.payload.task_id !== taskIdRef.current) return;
      setProgressPayload(e.payload);
      if (e.payload.status === "done") { setPhase("done"); setDonePath(e.payload.message); }
      else if (e.payload.status === "error") { setError(e.payload.message); setPhase("ready"); }
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, []);

  const handleFetch = async () => {
    if (!inputUrl.trim()) return;
    setPhase("fetching"); setError(""); setVideoInfo(null); setPlayInfo(null);
    setSelectedPart(null); setProgressPayload(null);
    try {
      const info = await invoke<VideoInfo>("bili_get_video_info", { url: inputUrl, sessdata: settings.sessdata });
      const firstPart = info.parts[0];
      const play = await invoke<PlayInfo>("bili_get_play_info", { bvid: info.bvid, cid: firstPart.cid, sessdata: settings.sessdata });
      setVideoInfo(info); setPlayInfo(play); setSelectedPart(firstPart);
      setSelectedQuality(play.accept_quality.includes(settings.default_quality) ? settings.default_quality : play.quality);
      setPhase("ready");
    } catch (e) { setError(String(e)); setPhase("idle"); }
  };

  const handleDownload = async () => {
    if (!videoInfo || !selectedPart || !playInfo) return;
    if (!settings.download_dir) { setError("请先在设置中配置下载目录"); setShowSettings(true); return; }
    if (mode === "video_mp4" && needsDash(selectedQuality) && !ffmpegVersion) {
      setError("下载 1080P 60帧及以上清晰度需要 ffmpeg，请先安装: brew install ffmpeg"); return;
    }
    if (mode === "audio_mp3" && !ffmpegVersion) {
      setError("下载 MP3 需要 ffmpeg，请先安装: brew install ffmpeg"); return;
    }
    const taskId = `${Date.now()}`;
    taskIdRef.current = taskId;
    setPhase("downloading"); setProgressPayload(null); setError("");
    try {
      await invoke("bili_download", {
        taskId, bvid: videoInfo.bvid, cid: selectedPart.cid,
        quality: selectedQuality, mode, title: videoInfo.title,
        part: selectedPart.part, page: selectedPart.page,
        isMulti: videoInfo.parts.length > 1, cover: videoInfo.cover,
        sessdata: settings.sessdata, downloadDir: settings.download_dir,
      });
      loadHistory();
    } catch (e) { setError(String(e)); setPhase("ready"); }
  };

  const handlePickDir = async () => {
    const dir = await invoke<string | null>("bili_pick_dir");
    if (dir) setSettingsDraft((d) => ({ ...d, download_dir: dir }));
  };

  const handleSaveSettings = async () => {
    try {
      await invoke("bili_save_settings", { sessdata: settingsDraft.sessdata, downloadDir: settingsDraft.download_dir, defaultQuality: settingsDraft.default_quality });
      setSettings(settingsDraft); setSelectedQuality(settingsDraft.default_quality);
      setSettingsSaved(true); setTimeout(() => setSettingsSaved(false), 2000);
    } catch (e) { console.error(e); }
  };

  const handleDeleteHistory = async (id: string) => {
    await invoke("bili_delete_history", { id });
    setHistory((h) => h.filter((r) => r.id !== id));
  };

  const pct = progressPayload?.total ? (progressPayload.downloaded / progressPayload.total) * 100 : null;
  const isDownloading = phase === "downloading";

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <header className="flex items-center justify-between border-b border-zinc-800 px-4 py-3">
        <div className="flex items-center gap-3">
          <button onClick={() => navigate("/toolbox")} className="flex items-center gap-1.5 text-sm text-zinc-400 hover:text-zinc-100 transition-colors">
            <ArrowLeft size={15} /> 工具箱
          </button>
          <span className="text-zinc-600">/</span>
          <span className="text-sm text-zinc-100">B站下载</span>
        </div>
        <button
          onClick={() => setShowSettings((v) => !v)}
          className={cn("flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm transition-colors", showSettings ? "bg-zinc-700 text-zinc-100" : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100")}
        >
          <Settings size={15} /> 设置
        </button>
      </header>

      <div className="flex flex-1 overflow-hidden">
        {/* Main */}
        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Tabs */}
          <div className="flex border-b border-zinc-800">
            {(["download", "history"] as const).map((t) => (
              <button
                key={t}
                onClick={() => { setTab(t); if (t === "history") loadHistory(); }}
                className={cn("flex items-center gap-1.5 px-4 py-2.5 text-sm transition-colors border-b-2 -mb-px", tab === t ? "border-blue-500 text-zinc-100" : "border-transparent text-zinc-400 hover:text-zinc-100")}
              >
                {t === "download" ? <><Download size={14} />下载</> : <><Clock size={14} />历史记录 {history.length > 0 && <span className="ml-1 rounded-full bg-zinc-700 px-1.5 py-0.5 text-xs">{history.length}</span>}</>}
              </button>
            ))}
          </div>

          {/* Download tab */}
          {tab === "download" && (
            <div className="flex flex-1 flex-col overflow-y-auto p-6 gap-4">
              {ffmpegVersion === null && (
                <div className="flex items-start gap-2 rounded-lg border border-yellow-800 bg-yellow-950/40 px-4 py-3 text-sm text-yellow-400">
                  <AlertTriangle size={15} className="mt-0.5 shrink-0" />
                  <span>未检测到 ffmpeg。MP3 下载与 1080P 60帧及以上清晰度不可用。安装：<code className="font-mono">brew install ffmpeg</code></span>
                </div>
              )}

              {/* URL 输入 */}
              <div className="flex gap-2">
                <input type="text" value={inputUrl} onChange={(e) => setInputUrl(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && phase !== "fetching" && handleFetch()}
                  placeholder="粘贴B站视频链接或BV号，例: BV1xx411c7mD"
                  className="flex-1 rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 outline-none focus:border-blue-500"
                />
                <button onClick={handleFetch} disabled={!inputUrl.trim() || phase === "fetching"}
                  className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50">
                  {phase === "fetching" ? <Loader2 size={15} className="animate-spin" /> : <Download size={15} />}
                  获取信息
                </button>
              </div>

              {/* 错误 */}
              {error && (
                <div className="flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/50 px-4 py-3 text-sm text-red-400">
                  <AlertCircle size={15} className="mt-0.5 shrink-0" />
                  <span className="flex-1">{error}</span>
                  <button onClick={() => setError("")}><X size={14} /></button>
                </div>
              )}

              {/* 视频信息卡 */}
              {videoInfo && playInfo && selectedPart && (
                <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-5 flex flex-col gap-4">
                  <div className="flex gap-4">
                    <img src={videoInfo.cover} alt="封面" className="h-24 w-40 shrink-0 rounded-lg object-cover" />
                    <div className="flex min-w-0 flex-1 flex-col gap-2">
                      <p className="text-base font-medium text-zinc-100 leading-snug" title={videoInfo.title}>{videoInfo.title}</p>
                      <p className="text-xs text-zinc-500">{videoInfo.bvid}</p>
                      {videoInfo.parts.length > 1 && (
                        <div className="flex flex-wrap gap-1.5 mt-1">
                          {videoInfo.parts.map((p) => (
                            <button key={p.cid} onClick={() => setSelectedPart(p)} title={p.part}
                              className={cn("rounded-md px-2.5 py-1 text-xs font-medium transition-colors",
                                selectedPart.cid === p.cid ? "bg-blue-600 text-white" : "bg-zinc-800 text-zinc-400 hover:bg-zinc-700 hover:text-zinc-100")}>
                              P{p.page}
                            </button>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center gap-3 border-t border-zinc-800 pt-4">
                    {/* 模式 */}
                    <div className="flex overflow-hidden rounded-lg border border-zinc-700 text-xs">
                      <button onClick={() => setMode("video_mp4")}
                        className={cn("flex items-center gap-1.5 px-3 py-1.5 transition-colors", mode === "video_mp4" ? "bg-blue-600 text-white" : "bg-zinc-800 text-zinc-400 hover:text-zinc-100")}>
                        <Download size={12} /> 视频 MP4
                      </button>
                      <button onClick={() => setMode("audio_mp3")} disabled={!ffmpegVersion}
                        className={cn("flex items-center gap-1.5 px-3 py-1.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40",
                          mode === "audio_mp3" ? "bg-blue-600 text-white" : "bg-zinc-800 text-zinc-400 hover:text-zinc-100")}>
                        <Music size={12} /> 音频 MP3
                      </button>
                    </div>

                    {mode === "video_mp4" && (
                      <>
                        <select value={selectedQuality} onChange={(e) => setSelectedQuality(Number(e.target.value))}
                          className="rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500">
                          {playInfo.accept_quality.map((qn, i) => {
                            const disabled = needsDash(qn) && !ffmpegVersion;
                            return (
                              <option key={qn} value={qn} disabled={disabled}>
                                {playInfo.accept_description[i]}{needsDash(qn) ? " ★" : ""}{disabled ? " (需要ffmpeg)" : ""}
                              </option>
                            );
                          })}
                        </select>
                        {needsDash(selectedQuality) && <span className="text-xs text-zinc-500">★ DASH + ffmpeg</span>}
                      </>
                    )}
                    {mode === "audio_mp3" && <span className="text-xs text-zinc-500">最高码率 AAC → MP3</span>}

                    <button onClick={handleDownload} disabled={isDownloading}
                      className="ml-auto flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50">
                      {isDownloading ? <Loader2 size={14} className="animate-spin" /> : mode === "audio_mp3" ? <Music size={14} /> : <Download size={14} />}
                      {isDownloading ? "下载中…" : "开始下载"}
                    </button>
                  </div>
                </div>
              )}

              {/* 进度 / 完成 */}
              {(isDownloading || phase === "done") && (
                <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-5">
                  {phase === "done" ? (
                    <div className="flex items-start gap-3">
                      <CheckCircle2 size={18} className="mt-0.5 shrink-0 text-green-400" />
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-medium text-zinc-100 mb-1">下载完成</p>
                        <p className="truncate text-xs text-zinc-500" title={donePath}>{donePath}</p>
                      </div>
                      <div className="flex shrink-0 gap-2">
                        <button onClick={() => openPath(donePath.substring(0, donePath.lastIndexOf("/")))}
                          className="flex items-center gap-1.5 rounded-lg border border-zinc-700 px-3 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-500 hover:text-zinc-100">
                          <Folder size={13} /> 打开目录
                        </button>
                        <button onClick={() => navigate("/toolbox/media", { state: { filePath: donePath } })}
                          className="flex items-center gap-1.5 rounded-lg border border-zinc-700 px-3 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-500 hover:text-zinc-100">
                          <Film size={13} /> 媒体编辑
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className="flex flex-col gap-3">
                      {progressPayload ? (
                        <>
                          <div className="flex justify-between text-xs text-zinc-400">
                            <span className="flex items-center gap-1.5">
                              <Loader2 size={13} className="animate-spin" />
                              {progressPayload.status === "downloading"
                                ? `${PHASE_LABEL[progressPayload.phase] ?? "下载中"}…`
                                : STATUS_LABEL[progressPayload.status] ?? "处理中…"}
                            </span>
                            {progressPayload.status === "downloading" && (
                              <span>{fmtSize(progressPayload.downloaded)}{progressPayload.total > 0 && ` / ${fmtSize(progressPayload.total)}`}</span>
                            )}
                          </div>
                          {progressPayload.status === "downloading" && pct !== null ? (
                            <>
                              <div className="h-1.5 overflow-hidden rounded-full bg-zinc-800">
                                <div className="h-full rounded-full bg-blue-500 transition-all duration-150" style={{ width: `${pct}%` }} />
                              </div>
                              <p className="text-right text-xs text-zinc-500">{pct.toFixed(1)}%</p>
                            </>
                          ) : (
                            <div className="h-1.5 overflow-hidden rounded-full bg-zinc-800">
                              <div className="h-full w-full rounded-full bg-blue-500 animate-pulse" />
                            </div>
                          )}
                        </>
                      ) : (
                        <div className="flex items-center gap-2 text-xs text-zinc-400">
                          <Loader2 size={13} className="animate-spin" /> 准备中…
                        </div>
                      )}
                    </div>
                  )}
                </div>
              )}

              {phase === "idle" && !error && (
                <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-600">
                  <Download size={36} /> <p className="text-sm">粘贴B站链接或BV号后点击「获取信息」</p>
                </div>
              )}
            </div>
          )}

          {/* History tab */}
          {tab === "history" && (
            <div className="flex flex-1 flex-col overflow-y-auto">
              {history.length === 0 ? (
                <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-600">
                  <Clock size={36} /> <p className="text-sm">暂无下载记录</p>
                </div>
              ) : (
                <>
                  <div className="flex items-center justify-between border-b border-zinc-800 px-4 py-2">
                    <span className="text-xs text-zinc-500">共 {history.length} 条记录</span>
                    <button onClick={async () => { await invoke("bili_clear_history"); setHistory([]); }}
                      className="text-xs text-zinc-500 hover:text-red-400 transition-colors">
                      清空全部
                    </button>
                  </div>
                  <div className="flex flex-col divide-y divide-zinc-800">
                    {history.map((rec) => (
                      <div key={rec.id} className="flex items-center gap-3 px-4 py-3 hover:bg-zinc-900/50 transition-colors">
                        {rec.cover && <img src={rec.cover} alt="" className="h-12 w-20 shrink-0 rounded object-cover" />}
                        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                          <p className="truncate text-sm font-medium text-zinc-100" title={rec.title}>{rec.title}</p>
                          <p className="text-xs text-zinc-500">
                            {QUALITY_NAME[rec.quality] ?? rec.quality} · {rec.mode === "audio_mp3" ? "MP3" : "MP4"} · {fmtSize(rec.file_size)}
                          </p>
                          <p className="truncate text-xs text-zinc-600" title={rec.file_path}>{rec.file_path}</p>
                          <p className="text-xs text-zinc-600">{fmtDate(rec.created_at)}</p>
                        </div>
                        <div className="flex shrink-0 items-center gap-1.5">
                          <button onClick={() => openPath(rec.file_path.substring(0, rec.file_path.lastIndexOf("/")))} title="打开目录"
                            className="rounded-lg p-1.5 text-zinc-500 hover:bg-zinc-800 hover:text-zinc-100 transition-colors">
                            <Folder size={14} />
                          </button>
                          <button onClick={() => navigate("/toolbox/media", { state: { filePath: rec.file_path } })} title="在媒体编辑器中打开"
                            className="rounded-lg p-1.5 text-zinc-500 hover:bg-zinc-800 hover:text-zinc-100 transition-colors">
                            <Film size={14} />
                          </button>
                          <button onClick={() => handleDeleteHistory(rec.id)} title="删除记录"
                            className="rounded-lg p-1.5 text-zinc-500 hover:bg-zinc-800 hover:text-red-400 transition-colors">
                            <Trash2 size={14} />
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                </>
              )}
            </div>
          )}
        </div>

        {/* 设置侧边栏 */}
        {showSettings && (
          <aside className="flex w-72 shrink-0 flex-col gap-5 overflow-y-auto border-l border-zinc-800 p-5">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-zinc-100">下载设置</h2>
              {ffmpegVersion !== undefined && (
                <span className={cn("rounded-full px-2 py-0.5 text-xs", ffmpegVersion ? "bg-green-900/60 text-green-400" : "bg-red-900/60 text-red-400")}>
                  ffmpeg {ffmpegVersion ? "已安装" : "未安装"}
                </span>
              )}
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-zinc-400">SESSDATA Cookie</label>
              <div className="relative">
                <input type={showSessdata ? "text" : "password"} value={settingsDraft.sessdata}
                  onChange={(e) => setSettingsDraft((d) => ({ ...d, sessdata: e.target.value }))}
                  placeholder="从浏览器Cookie中复制"
                  className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 pr-14 text-xs text-zinc-100 placeholder-zinc-600 outline-none focus:border-blue-500"
                />
                <button onClick={() => setShowSessdata((v) => !v)} className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-zinc-500 hover:text-zinc-300">
                  {showSessdata ? "隐藏" : "显示"}
                </button>
              </div>
              <p className="text-xs text-zinc-600">浏览器开发者工具 → Application → Cookies → bilibili.com → SESSDATA</p>
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-zinc-400">下载目录</label>
              <div className="flex gap-2">
                <input type="text" value={settingsDraft.download_dir}
                  onChange={(e) => setSettingsDraft((d) => ({ ...d, download_dir: e.target.value }))}
                  placeholder="/Users/yourname/Downloads"
                  className="min-w-0 flex-1 rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-xs text-zinc-100 placeholder-zinc-600 outline-none focus:border-blue-500"
                />
                <button onClick={handlePickDir} className="flex shrink-0 items-center gap-1 rounded-lg border border-zinc-700 px-2.5 py-2 text-xs text-zinc-400 transition-colors hover:border-zinc-500 hover:text-zinc-100">
                  <Folder size={13} />
                </button>
              </div>
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-zinc-400">默认清晰度</label>
              <select value={settingsDraft.default_quality}
                onChange={(e) => setSettingsDraft((d) => ({ ...d, default_quality: Number(e.target.value) }))}
                className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-xs text-zinc-100 outline-none focus:border-blue-500">
                <option value={127}>8K 超高清 ★</option>
                <option value={120}>4K 超清 ★</option>
                <option value={116}>1080P 60帧 ★</option>
                <option value={112}>1080P 高码率 ★</option>
                <option value={80}>1080P 高清</option>
                <option value={64}>720P 高清</option>
                <option value={32}>480P 清晰</option>
                <option value={16}>360P 流畅</option>
              </select>
              <p className="text-xs text-zinc-600">★ 需要 ffmpeg</p>
            </div>
            <button onClick={handleSaveSettings}
              className={cn("mt-auto w-full rounded-lg py-2 text-sm font-medium transition-colors", settingsSaved ? "bg-green-700 text-green-100" : "bg-blue-600 text-white hover:bg-blue-500")}>
              {settingsSaved ? "已保存 ✓" : "保存设置"}
            </button>
          </aside>
        )}
      </div>
    </div>
  );
}
