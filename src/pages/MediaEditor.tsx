import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  ArrowLeft, Film, Music, Scissors, RefreshCw, FolderOpen,
  Loader2, CheckCircle2, AlertCircle, Upload, X,
  Play, Pause, SkipBack,
} from "lucide-react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";

// ─── Types ────────────────────────────────────────────────────

interface MediaInfo {
  path: string;
  filename: string;
  duration_secs: number;
  size_bytes: number;
  video_codec: string;
  audio_codec: string;
  width: number;
  height: number;
  fps: string;
  bitrate: number;
  is_audio_only: boolean;
}

type ProcessState = "idle" | "processing" | "done" | "error";

// ─── Helpers ──────────────────────────────────────────────────

function fmtSize(b: number) {
  if (!b) return "—";
  if (b < 1 << 20) return `${(b >> 10).toFixed(0)} KB`;
  if (b < 1 << 30) return `${(b / (1 << 20)).toFixed(1)} MB`;
  return `${(b / (1 << 30)).toFixed(2)} GB`;
}

function secsToHMS(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = Math.floor(s % 60);
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
}

function hmsToSecs(hms: string): number {
  const parts = hms.split(":").map((p) => parseInt(p, 10) || 0);
  if (parts.length === 3) return parts[0] * 3600 + parts[1] * 60 + parts[2];
  if (parts.length === 2) return parts[0] * 60 + parts[1];
  return parts[0] ?? 0;
}

function fmtDuration(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = Math.floor(s % 60);
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
  return `${m}:${String(sec).padStart(2, "0")}`;
}

const AUDIO_FORMATS = ["mp3", "aac", "flac", "wav"] as const;
const VIDEO_FORMATS = ["mp4", "mkv"] as const;

// ─── Component ────────────────────────────────────────────────

export default function MediaEditor() {
  const navigate = useNavigate();
  const location = useLocation();

  // File info
  const [mediaInfo, setMediaInfo] = useState<MediaInfo | null>(null);
  const [loadingInfo, setLoadingInfo] = useState(false);
  const [infoError, setInfoError] = useState("");

  // Player
  const videoRef = useRef<HTMLVideoElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const timelineRef = useRef<HTMLDivElement>(null);
  const [mediaSrc, setMediaSrc] = useState("");
  const [currentTime, setCurrent] = useState(0);
  const [playing, setPlaying] = useState(false);

  // Trim
  const [startSecs, setStartSecs] = useState(0);
  const [endSecs, setEndSecs] = useState(0);
  const [startStr, setStartStr] = useState("00:00:00");
  const [endStr, setEndStr] = useState("00:00:00");
  const startRef = useRef(0);
  const endRef = useRef(0);
  const [dragging, setDragging] = useState<"start" | "end" | null>(null);
  const [trimState, setTrimState] = useState<ProcessState>("idle");
  const [trimResult, setTrimResult] = useState("");
  const [trimError, setTrimError] = useState("");

  // Extract
  const [extractFmt, setExtractFmt] = useState<(typeof AUDIO_FORMATS)[number]>("mp3");
  const [extractState, setExtractState] = useState<ProcessState>("idle");
  const [extractResult, setExtractResult] = useState("");
  const [extractError, setExtractError] = useState("");

  // Convert
  const [convertFmt, setConvertFmt] = useState("mp4");
  const [convertState, setConvertState] = useState<ProcessState>("idle");
  const [convertResult, setConvertResult] = useState("");
  const [convertError, setConvertError] = useState("");

  // Keep refs in sync for drag callbacks
  useEffect(() => { startRef.current = startSecs; }, [startSecs]);
  useEffect(() => { endRef.current = endSecs; }, [endSecs]);

  // Sync seconds → text inputs (only when not actively editing)
  const syncStartStr = (s: number) => setStartStr(secsToHMS(s));
  const syncEndStr = (s: number) => setEndStr(secsToHMS(s));

  const mediaEl = () =>
    mediaInfo?.is_audio_only ? audioRef.current : videoRef.current;

  const seekTo = (secs: number) => {
    const el = mediaEl();
    if (el) el.currentTime = secs;
    setCurrent(secs);
  };

  // ─── File loading ────────────────────────────────────────────

  const loadFile = async (path: string) => {
    setLoadingInfo(true);
    setInfoError("");
    setMediaInfo(null);
    setMediaSrc("");
    setPlaying(false);
    resetOps();
    try {
      const info = await invoke<MediaInfo>("media_get_info", { path });
      setMediaInfo(info);
      setMediaSrc(convertFileSrc(path));
      const end = info.duration_secs;
      setStartSecs(0); startRef.current = 0;
      setEndSecs(end); endRef.current = end;
      syncStartStr(0); syncEndStr(end);
      setCurrent(0);
    } catch (e) {
      setInfoError(String(e));
    } finally {
      setLoadingInfo(false);
    }
  };

  const resetOps = () => {
    setTrimState("idle"); setTrimResult(""); setTrimError("");
    setExtractState("idle"); setExtractResult(""); setExtractError("");
    setConvertState("idle"); setConvertResult(""); setConvertError("");
  };

  useEffect(() => {
    const fp = (location.state as { filePath?: string } | null)?.filePath;
    if (fp) loadFile(fp);
  }, []);

  const handleOpenFile = async () => {
    const p = await invoke<string | null>("media_open_file");
    if (p) loadFile(p);
  };

  // ─── Player controls ─────────────────────────────────────────

  const togglePlay = () => {
    const el = mediaEl();
    if (!el) return;
    if (el.paused) { el.play(); setPlaying(true); }
    else { el.pause(); setPlaying(false); }
  };

  const playPreview = () => {
    const el = mediaEl();
    if (!el) return;
    el.currentTime = startRef.current;
    el.play();
    setPlaying(true);
  };

  const handleTimeUpdate = () => {
    const el = mediaEl();
    if (!el) return;
    const t = el.currentTime;
    setCurrent(t);
    // Auto-stop at trim end point
    if (!el.paused && t >= endRef.current) {
      el.pause();
      el.currentTime = endRef.current;
      setCurrent(endRef.current);
      setPlaying(false);
    }
  };

  const handleEnded = () => setPlaying(false);

  // ─── Timeline drag ───────────────────────────────────────────

  const duration = mediaInfo?.duration_secs ?? 0;
  const startPct = duration ? (startSecs / duration) * 100 : 0;
  const endPct = duration ? (endSecs / duration) * 100 : 100;
  const currentPct = duration ? (currentTime / duration) * 100 : 0;

  const pctToSecs = (clientX: number): number => {
    if (!timelineRef.current || !mediaInfo) return 0;
    const rect = timelineRef.current.getBoundingClientRect();
    return Math.max(0, Math.min(1, (clientX - rect.left) / rect.width)) * mediaInfo.duration_secs;
  };

  const applyDrag = (clientX: number, which: "start" | "end") => {
    const secs = pctToSecs(clientX);
    if (which === "start") {
      const v = Math.max(0, Math.min(secs, endRef.current - 0.5));
      startRef.current = v;
      setStartSecs(v);
      syncStartStr(v);
      seekTo(v);
    } else {
      const v = Math.max(startRef.current + 0.5, Math.min(secs, duration));
      endRef.current = v;
      setEndSecs(v);
      syncEndStr(v);
      seekTo(v);
    }
  };

  // Track click → seek
  const handleTrackPointerDown = (e: React.PointerEvent) => {
    const secs = pctToSecs(e.clientX);
    seekTo(secs);
  };

  // ─── Trim op ─────────────────────────────────────────────────

  const handleTrim = async () => {
    if (!mediaInfo) return;
    setTrimState("processing"); setTrimError("");
    try {
      const out = await invoke<string>("media_process", {
        path: mediaInfo.path,
        operation: "trim",
        startTime: secsToHMS(startSecs),
        endTime: secsToHMS(endSecs),
        outputFormat: null,
      });
      setTrimResult(out); setTrimState("done");
    } catch (e) { setTrimError(String(e)); setTrimState("error"); }
  };

  // ─── Extract op ──────────────────────────────────────────────

  const handleExtract = async () => {
    if (!mediaInfo) return;
    setExtractState("processing"); setExtractError("");
    try {
      const out = await invoke<string>("media_process", {
        path: mediaInfo.path, operation: "extract",
        startTime: null, endTime: null, outputFormat: extractFmt,
      });
      setExtractResult(out); setExtractState("done");
    } catch (e) { setExtractError(String(e)); setExtractState("error"); }
  };

  // ─── Convert op ──────────────────────────────────────────────

  const handleConvert = async () => {
    if (!mediaInfo) return;
    setConvertState("processing"); setConvertError("");
    try {
      const out = await invoke<string>("media_process", {
        path: mediaInfo.path, operation: "convert",
        startTime: null, endTime: null, outputFormat: convertFmt,
      });
      setConvertResult(out); setConvertState("done");
    } catch (e) { setConvertError(String(e)); setConvertState("error"); }
  };

  const convertFormats = mediaInfo?.is_audio_only
    ? AUDIO_FORMATS
    : [...VIDEO_FORMATS, ...AUDIO_FORMATS];

  // ─── Render ──────────────────────────────────────────────────

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <header className="flex items-center gap-3 border-b border-zinc-800 px-4 py-3">
        <button
          onClick={() => navigate("/toolbox")}
          className="flex items-center gap-1.5 text-sm text-zinc-400 hover:text-zinc-100 transition-colors"
        >
          <ArrowLeft size={15} /> 工具箱
        </button>
        <span className="text-zinc-600">/</span>
        <span className="text-sm text-zinc-100">媒体编辑</span>
      </header>

      <div className="flex flex-1 overflow-hidden">
        {/* ── Left: file info panel ── */}
        <div className="flex w-64 shrink-0 flex-col gap-4 overflow-y-auto border-r border-zinc-800 p-4">
          <button
            onClick={handleOpenFile}
            className="flex items-center justify-center gap-2 rounded-xl border border-dashed border-zinc-700 py-4 text-sm text-zinc-400 transition-colors hover:border-zinc-500 hover:text-zinc-100"
          >
            <Upload size={16} />
            {mediaInfo ? "更换文件" : "打开文件"}
          </button>

          {loadingInfo && (
            <div className="flex items-center gap-2 text-xs text-zinc-400">
              <Loader2 size={12} className="animate-spin" /> 读取信息…
            </div>
          )}
          {infoError && (
            <div className="flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/40 px-3 py-2 text-xs text-red-400">
              <AlertCircle size={12} className="mt-0.5 shrink-0" />
              <span className="flex-1">{infoError}</span>
              <button onClick={() => setInfoError("")}><X size={11} /></button>
            </div>
          )}

          {mediaInfo && (
            <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-3 flex flex-col gap-2">
              <div className="flex items-start gap-2">
                {mediaInfo.is_audio_only
                  ? <Music size={14} className="mt-0.5 shrink-0 text-blue-400" />
                  : <Film size={14} className="mt-0.5 shrink-0 text-blue-400" />}
                <p className="text-xs font-medium text-zinc-100 break-all leading-snug">
                  {mediaInfo.filename}
                </p>
              </div>
              <div className="flex flex-col gap-1">
                <InfoRow label="时长" value={fmtDuration(mediaInfo.duration_secs)} />
                <InfoRow label="大小" value={fmtSize(mediaInfo.size_bytes)} />
                {!mediaInfo.is_audio_only && mediaInfo.width > 0 && (
                  <>
                    <InfoRow label="分辨率" value={`${mediaInfo.width}×${mediaInfo.height}`} />
                    {mediaInfo.fps && <InfoRow label="帧率" value={`${mediaInfo.fps} fps`} />}
                    <InfoRow label="视频" value={mediaInfo.video_codec.toUpperCase()} />
                  </>
                )}
                {mediaInfo.audio_codec && (
                  <InfoRow label="音频" value={mediaInfo.audio_codec.toUpperCase()} />
                )}
                {mediaInfo.bitrate > 0 && (
                  <InfoRow label="码率" value={`${Math.round(mediaInfo.bitrate / 1000)} kbps`} />
                )}
              </div>
            </div>
          )}

          {!mediaInfo && !loadingInfo && !infoError && (
            <p className="text-xs text-zinc-600 text-center leading-relaxed">
              支持 MP4 · MKV · AVI · MOV · FLV · MP3 · AAC · FLAC · WAV 等
            </p>
          )}
        </div>

        {/* ── Right: editor ── */}
        <div className="flex flex-1 flex-col overflow-y-auto p-5 gap-5">
          {!mediaInfo && (
            <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-600">
              <Film size={40} />
              <p className="text-sm">打开文件后开始编辑</p>
              <p className="text-xs text-zinc-700">也可以从 B站下载历史记录点击「媒体编辑」直接跳转</p>
            </div>
          )}

          {mediaInfo && (
            <>
              {/* ── Player + Trim timeline ── */}
              <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
                {/* Video */}
                {!mediaInfo.is_audio_only && mediaSrc && (
                  <video
                    ref={videoRef}
                    src={mediaSrc}
                    className="w-full max-h-64 bg-black object-contain"
                    onTimeUpdate={handleTimeUpdate}
                    onPlay={() => setPlaying(true)}
                    onPause={() => setPlaying(false)}
                    onEnded={handleEnded}
                  />
                )}

                {/* Audio */}
                {mediaInfo.is_audio_only && mediaSrc && (
                  <div className="flex items-center justify-center bg-zinc-950 py-6">
                    <Music size={40} className="text-zinc-600" />
                    <audio
                      ref={audioRef}
                      src={mediaSrc}
                      onTimeUpdate={handleTimeUpdate}
                      onPlay={() => setPlaying(true)}
                      onPause={() => setPlaying(false)}
                      onEnded={handleEnded}
                    />
                  </div>
                )}

                {/* Transport bar */}
                <div className="flex items-center gap-3 border-t border-zinc-800 px-4 py-2">
                  <button
                    onClick={playPreview}
                    title="从裁剪起点预览"
                    className="text-zinc-400 hover:text-zinc-100 transition-colors"
                  >
                    <SkipBack size={15} />
                  </button>
                  <button
                    onClick={togglePlay}
                    className="text-zinc-100 hover:text-blue-400 transition-colors"
                  >
                    {playing ? <Pause size={16} /> : <Play size={16} />}
                  </button>
                  <span className="text-xs font-mono text-zinc-400 tabular-nums">
                    {fmtDuration(currentTime)}
                    <span className="text-zinc-600"> / {fmtDuration(duration)}</span>
                  </span>
                </div>

                {/* Timeline */}
                <div className="px-4 pb-4 pt-1">
                  <div
                    ref={timelineRef}
                    className="relative h-10 select-none"
                    onPointerDown={handleTrackPointerDown}
                  >
                    {/* Track background */}
                    <div className="absolute left-0 right-0 top-4 h-2 rounded-full bg-zinc-700" />

                    {/* Selected region */}
                    <div
                      className="pointer-events-none absolute top-4 h-2 rounded-full bg-blue-500/70"
                      style={{
                        left: `${startPct}%`,
                        width: `${Math.max(0, endPct - startPct)}%`,
                      }}
                    />

                    {/* Playhead */}
                    <div
                      className="pointer-events-none absolute top-2 h-6 w-0.5 rounded-full bg-white/80"
                      style={{ left: `calc(${currentPct}% - 1px)` }}
                    />

                    {/* Start handle */}
                    <Handle
                      pct={startPct}
                      onDragStart={(e) => {
                        e.stopPropagation();
                        e.currentTarget.setPointerCapture(e.pointerId);
                        setDragging("start");
                      }}
                      onDragMove={(e) => {
                        if (dragging !== "start") return;
                        applyDrag(e.clientX, "start");
                      }}
                      onDragEnd={() => setDragging(null)}
                    />

                    {/* End handle */}
                    <Handle
                      pct={endPct}
                      onDragStart={(e) => {
                        e.stopPropagation();
                        e.currentTarget.setPointerCapture(e.pointerId);
                        setDragging("end");
                      }}
                      onDragMove={(e) => {
                        if (dragging !== "end") return;
                        applyDrag(e.clientX, "end");
                      }}
                      onDragEnd={() => setDragging(null)}
                    />
                  </div>

                  {/* Time labels */}
                  <div className="flex items-center gap-3 mt-1">
                    <span className="text-xs text-zinc-500">开始</span>
                    <input
                      className="w-24 rounded border border-zinc-700 bg-zinc-800 px-2 py-0.5 text-xs font-mono text-zinc-100 outline-none focus:border-blue-500"
                      value={startStr}
                      onChange={(e) => setStartStr(e.target.value)}
                      onBlur={() => {
                        const s = Math.max(0, Math.min(hmsToSecs(startStr), endRef.current - 0.5));
                        setStartSecs(s); startRef.current = s; syncStartStr(s); seekTo(s);
                      }}
                      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                    />
                    <span className="text-zinc-600">→</span>
                    <span className="text-xs text-zinc-500">结束</span>
                    <input
                      className="w-24 rounded border border-zinc-700 bg-zinc-800 px-2 py-0.5 text-xs font-mono text-zinc-100 outline-none focus:border-blue-500"
                      value={endStr}
                      onChange={(e) => setEndStr(e.target.value)}
                      onBlur={() => {
                        const s = Math.max(startRef.current + 0.5, Math.min(hmsToSecs(endStr), duration));
                        setEndSecs(s); endRef.current = s; syncEndStr(s); seekTo(s);
                      }}
                      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                    />
                    <span className="ml-auto text-xs text-zinc-600 tabular-nums">
                      片段: {fmtDuration(endSecs - startSecs)}
                    </span>
                    <button
                      onClick={handleTrim}
                      disabled={trimState === "processing"}
                      className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors"
                    >
                      {trimState === "processing"
                        ? <Loader2 size={12} className="animate-spin" />
                        : <Scissors size={12} />}
                      裁剪并保存
                    </button>
                  </div>
                  <OpResult state={trimState} result={trimResult} error={trimError} />
                </div>
              </div>

              {/* ── Extract audio ── */}
              {!mediaInfo.is_audio_only && (
                <OpCard icon={<Music size={14} />} title="提取音频" desc="仅提取音轨，转换为目标格式">
                  <div className="flex flex-wrap items-center gap-3">
                    <select
                      value={extractFmt}
                      onChange={(e) => setExtractFmt(e.target.value as typeof extractFmt)}
                      className="rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500"
                    >
                      {AUDIO_FORMATS.map((f) => (
                        <option key={f} value={f}>{f.toUpperCase()}</option>
                      ))}
                    </select>
                    <button
                      onClick={handleExtract}
                      disabled={extractState === "processing"}
                      className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors"
                    >
                      {extractState === "processing"
                        ? <Loader2 size={13} className="animate-spin" />
                        : <Music size={13} />}
                      提取
                    </button>
                  </div>
                  <OpResult state={extractState} result={extractResult} error={extractError} />
                </OpCard>
              )}

              {/* ── Format convert ── */}
              <OpCard
                icon={<RefreshCw size={14} />}
                title="格式转换"
                desc={mediaInfo.is_audio_only ? "转换音频格式" : "重编码视频或提取为音频"}
              >
                <div className="flex flex-wrap items-center gap-3">
                  <select
                    value={convertFmt}
                    onChange={(e) => setConvertFmt(e.target.value)}
                    className="rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500"
                  >
                    {convertFormats.map((f) => (
                      <option key={f} value={f}>{f.toUpperCase()}</option>
                    ))}
                  </select>
                  {!mediaInfo.is_audio_only && ["mp4", "mkv"].includes(convertFmt) && (
                    <span className="text-xs text-zinc-500">H.264 + AAC 重编码</span>
                  )}
                  <button
                    onClick={handleConvert}
                    disabled={convertState === "processing"}
                    className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors"
                  >
                    {convertState === "processing"
                      ? <Loader2 size={13} className="animate-spin" />
                      : <RefreshCw size={13} />}
                    转换
                  </button>
                </div>
                <OpResult state={convertState} result={convertResult} error={convertError} />
              </OpCard>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Sub-components ───────────────────────────────────────────

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-2 text-xs">
      <span className="text-zinc-600">{label}</span>
      <span className="text-zinc-400">{value}</span>
    </div>
  );
}

function Handle({
  pct,
  onDragStart,
  onDragMove,
  onDragEnd,
}: {
  pct: number;
  onDragStart: (e: React.PointerEvent<HTMLDivElement>) => void;
  onDragMove: (e: React.PointerEvent<HTMLDivElement>) => void;
  onDragEnd: () => void;
}) {
  return (
    <div
      className="absolute top-0 flex h-10 w-5 -translate-x-2.5 cursor-ew-resize items-center justify-center"
      style={{ left: `${pct}%` }}
      onPointerDown={onDragStart}
      onPointerMove={onDragMove}
      onPointerUp={onDragEnd}
      onPointerCancel={onDragEnd}
    >
      <div className="h-7 w-3 rounded bg-blue-400 shadow-lg shadow-blue-900/40 flex flex-col items-center justify-center gap-0.5">
        <div className="h-0.5 w-1.5 rounded-full bg-blue-200/60" />
        <div className="h-0.5 w-1.5 rounded-full bg-blue-200/60" />
        <div className="h-0.5 w-1.5 rounded-full bg-blue-200/60" />
      </div>
    </div>
  );
}

function OpCard({
  icon, title, desc, children,
}: {
  icon: React.ReactNode; title: string; desc: string; children: React.ReactNode;
}) {
  return (
    <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-5 flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <span className="text-blue-400">{icon}</span>
        <span className="text-sm font-medium text-zinc-100">{title}</span>
        <span className="text-xs text-zinc-500">{desc}</span>
      </div>
      {children}
    </div>
  );
}

function OpResult({ state, result, error }: { state: ProcessState; result: string; error: string }) {
  if (state === "idle") return null;
  if (state === "processing") return (
    <div className="flex items-center gap-2 text-xs text-zinc-400">
      <Loader2 size={12} className="animate-spin" /> 处理中，请稍候…
    </div>
  );
  if (state === "error") return (
    <div className="flex items-start gap-2 text-xs text-red-400">
      <AlertCircle size={12} className="mt-0.5 shrink-0" /> {error}
    </div>
  );
  return (
    <div className="flex items-center gap-2">
      <CheckCircle2 size={13} className="shrink-0 text-green-400" />
      <span className="min-w-0 flex-1 truncate text-xs text-zinc-500" title={result}>{result}</span>
      <button
        onClick={() => openPath(result.substring(0, result.lastIndexOf("/")))}
        className="shrink-0 flex items-center gap-1 text-xs text-zinc-500 hover:text-zinc-200 transition-colors"
      >
        <FolderOpen size={12} /> 打开
      </button>
    </div>
  );
}
