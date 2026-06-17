import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  ArrowLeft, Upload, Crop, Play, Square, Loader2, CheckCircle2,
  AlertCircle, FolderOpen, FileText, Image as ImageIcon, X,
  Download, Package, RefreshCw,
} from "lucide-react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";

// ─── Types ────────────────────────────────────────────────────

interface OcrEnv {
  ffmpeg: boolean;
  python: string | null;
  sidecar: boolean;
}

interface DepItem {
  module: string;
  package: string;
  installed: boolean;
}

interface DepStatus {
  python: string | null;
  python_version: string | null;
  compatible: boolean;
  configured: boolean;
  deps: DepItem[];
  all_ok: boolean;
}

interface DepEvent {
  kind: "log" | "done" | "error";
  message: string;
}

const TSINGHUA_MIRROR = "https://pypi.tuna.tsinghua.edu.cn/simple";

interface OcrEvent {
  task_id: string;
  kind: "progress" | "log" | "done" | "error" | "cancelled";
  current?: number;
  total?: number;
  message?: string;
  output_path?: string;
}

interface Region {
  x: number; y: number; w: number; h: number; // 相对坐标 0~1
}

type RunState = "idle" | "running" | "done" | "error" | "cancelled";

const LANGUAGES = [
  { value: "ch", label: "简体中文 / 通用" },
  { value: "cht", label: "繁体中文" },
];

const ENGINES = [
  { value: "rapidocr", label: "RapidOCR（推荐，轻量快）" },
  { value: "paddle", label: "PaddleOCR（更准，但重且慢）" },
];

// ─── Component ────────────────────────────────────────────────

export default function SubtitleOcr() {
  const navigate = useNavigate();

  const [env, setEnv] = useState<OcrEnv | null>(null);

  // OCR 引擎
  const [engine, setEngine] = useState("rapidocr");

  // 依赖检测 / 一键安装
  const [deps, setDeps] = useState<DepStatus | null>(null);
  const [depsLoading, setDepsLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installLogs, setInstallLogs] = useState<string[]>([]);
  const [useMirror, setUseMirror] = useState(true);
  const [pythonError, setPythonError] = useState("");
  const depUnlistenRef = useRef<UnlistenFn | null>(null);
  const installLogRef = useRef<HTMLDivElement>(null);

  const [videoPath, setVideoPath] = useState("");
  const [previewSrc, setPreviewSrc] = useState("");
  const [previewTime, setPreviewTime] = useState(60);
  const [extracting, setExtracting] = useState(false);

  // 框选
  const imgRef = useRef<HTMLImageElement>(null);
  const [region, setRegion] = useState<Region | null>(null);
  const [drag, setDrag] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);

  // 参数
  const [language, setLanguage] = useState("ch");
  const [sampleFps, setSampleFps] = useState(4);
  const [similarity, setSimilarity] = useState(0.9);

  // 运行
  const [runState, setRunState] = useState<RunState>("idle");
  const [progress, setProgress] = useState({ current: 0, total: 0 });
  const [logs, setLogs] = useState<string[]>([]);
  const [outputPath, setOutputPath] = useState("");
  const [srtText, setSrtText] = useState("");
  const [errorMsg, setErrorMsg] = useState("");
  const taskIdRef = useRef("");
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const logBoxRef = useRef<HTMLDivElement>(null);
  const runStateRef = useRef<RunState>("idle");
  useEffect(() => { runStateRef.current = runState; }, [runState]);

  // ─── 环境自检 + 依赖检测 ───────────────────────────────────
  useEffect(() => {
    invoke<OcrEnv>("subtitle_ocr_check_env").then(setEnv).catch(() => {});
    return () => { unlistenRef.current?.(); depUnlistenRef.current?.(); };
  }, []);

  // 切换引擎时重新检测该引擎的依赖
  useEffect(() => { checkDeps(); }, [engine]);

  useEffect(() => {
    if (logBoxRef.current) logBoxRef.current.scrollTop = logBoxRef.current.scrollHeight;
  }, [logs]);

  useEffect(() => {
    if (installLogRef.current) installLogRef.current.scrollTop = installLogRef.current.scrollHeight;
  }, [installLogs]);

  const checkDeps = async () => {
    setDepsLoading(true);
    try { setDeps(await invoke<DepStatus>("subtitle_ocr_check_deps", { engine })); }
    catch { /* ignore */ }
    finally { setDepsLoading(false); }
  };

  const pickPython = async () => {
    const p = await invoke<string | null>("subtitle_ocr_pick_python");
    if (!p) return;
    setPythonError("");
    try {
      await invoke("subtitle_ocr_set_python", { path: p });
    } catch (e) {
      setPythonError(String(e));
    }
    await checkDeps();
  };

  const resetPython = async () => {
    setPythonError("");
    try { await invoke("subtitle_ocr_set_python", { path: "" }); } catch { /* ignore */ }
    await checkDeps();
  };

  const installDeps = async () => {
    setInstalling(true);
    setInstallLogs([]);
    depUnlistenRef.current?.();
    depUnlistenRef.current = await listen<DepEvent>("subtitle_ocr_dep_event", (ev) => {
      setInstallLogs((prev) => [...prev.slice(-400), ev.payload.message]);
    });
    try {
      await invoke("subtitle_ocr_install_deps", { engine, mirror: useMirror ? TSINGHUA_MIRROR : null });
    } catch { /* 错误已进日志 */ }
    await checkDeps();
    setInstalling(false);
  };

  // ─── 选视频 + 抽预览帧 ─────────────────────────────────────
  const pickVideo = async () => {
    const p = await invoke<string | null>("subtitle_ocr_pick_video");
    if (!p) return;
    setVideoPath(p);
    setRegion(null);
    setPreviewSrc("");
    setRunState("idle");
    setSrtText("");
    setOutputPath("");
    await extractFrame(p, previewTime);
  };

  const extractFrame = async (path: string, time: number) => {
    setExtracting(true);
    try {
      const imgPath = await invoke<string>("subtitle_ocr_extract_frame", {
        videoPath: path, timeSecs: time,
      });
      // 加时间戳防缓存
      setPreviewSrc(convertFileSrc(imgPath) + `?t=${Date.now()}`);
    } catch (e) {
      setErrorMsg(String(e));
    } finally {
      setExtracting(false);
    }
  };

  // ─── 框选字幕区域 ──────────────────────────────────────────
  const relFromEvent = (e: React.PointerEvent) => {
    const el = imgRef.current!;
    const r = el.getBoundingClientRect();
    const x = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    const y = Math.max(0, Math.min(1, (e.clientY - r.top) / r.height));
    return { x, y };
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (!previewSrc) return;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    const { x, y } = relFromEvent(e);
    setDrag({ x0: x, y0: y, x1: x, y1: y });
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!drag) return;
    const { x, y } = relFromEvent(e);
    setDrag({ ...drag, x1: x, y1: y });
  };
  const onPointerUp = () => {
    if (!drag) return;
    const x = Math.min(drag.x0, drag.x1);
    const y = Math.min(drag.y0, drag.y1);
    const w = Math.abs(drag.x1 - drag.x0);
    const h = Math.abs(drag.y1 - drag.y0);
    setDrag(null);
    if (w > 0.01 && h > 0.01) setRegion({ x, y, w, h });
  };

  // 当前要画的矩形（拖拽中用 drag，否则用已确定 region）
  const rect = drag
    ? { left: Math.min(drag.x0, drag.x1), top: Math.min(drag.y0, drag.y1),
        width: Math.abs(drag.x1 - drag.x0), height: Math.abs(drag.y1 - drag.y0) }
    : region
    ? { left: region.x, top: region.y, width: region.w, height: region.h }
    : null;

  // ─── 启动 / 取消 ───────────────────────────────────────────
  const start = async () => {
    if (!videoPath || !region) return;
    const taskId = crypto.randomUUID();
    taskIdRef.current = taskId;
    setRunState("running");
    setProgress({ current: 0, total: 0 });
    setLogs([]);
    setSrtText("");
    setOutputPath("");
    setErrorMsg("");

    unlistenRef.current?.();
    unlistenRef.current = await listen<OcrEvent>("subtitle_ocr_event", (ev) => {
      const p = ev.payload;
      if (p.task_id !== taskIdRef.current) return;
      switch (p.kind) {
        case "progress":
          setProgress({ current: p.current ?? 0, total: p.total ?? 0 });
          if (p.message) appendLog(`[${p.current ?? 0}/${p.total ?? 0}] ${p.message}`);
          break;
        case "log":
          if (p.message) appendLog(p.message);
          break;
        case "done":
          setRunState("done");
          if (p.output_path) { setOutputPath(p.output_path); loadSrt(p.output_path); }
          if (p.message) appendLog(p.message);
          break;
        case "error":
          setRunState("error");
          setErrorMsg(p.message ?? "OCR 失败");
          break;
        case "cancelled":
          setRunState("cancelled");
          break;
      }
    });

    try {
      await invoke("subtitle_ocr_start", {
        taskId,
        videoPath,
        outputPath: null,
        subtitleRegion: region,
        sampleFps,
        engine,
        language,
        similarityThreshold: similarity,
        minDuration: null,
        maxGap: null,
        minConfidence: null,
      });
    } catch (e) {
      // start 返回 Err 时（含取消）：状态已由事件设置，这里兜底
      if (runStateRef.current === "running") {
        setErrorMsg(String(e));
        setRunState("error");
      }
    }
  };

  const cancel = async () => {
    if (taskIdRef.current) {
      await invoke("subtitle_ocr_cancel", { taskId: taskIdRef.current }).catch(() => {});
    }
  };

  const appendLog = (line: string) =>
    setLogs((prev) => [...prev.slice(-300), line]);

  const loadSrt = async (path: string) => {
    try { setSrtText(await invoke<string>("subtitle_ocr_read_text", { path })); }
    catch { /* ignore */ }
  };

  const dirOf = (p: string) => {
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i > 0 ? p.substring(0, i) : ".";
  };

  const pct = progress.total > 0 ? (progress.current / progress.total) * 100 : 0;
  const canStart = !!videoPath && !!region && runState !== "running";

  // ─── Render ────────────────────────────────────────────────
  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-zinc-800 px-4 py-3">
        <button onClick={() => navigate("/toolbox")}
          className="flex items-center gap-1.5 text-sm text-zinc-400 hover:text-zinc-100 transition-colors">
          <ArrowLeft size={15} /> 工具箱
        </button>
        <span className="text-zinc-600">/</span>
        <span className="text-sm text-zinc-100">硬字幕 OCR 对轴</span>
      </header>

      {/* 环境提示 */}
      {env && (!env.ffmpeg || !env.python || !env.sidecar) && (
        <div className="flex items-start gap-2 border-b border-amber-900/50 bg-amber-950/30 px-4 py-2 text-xs text-amber-400">
          <AlertCircle size={13} className="mt-0.5 shrink-0" />
          <div className="flex flex-col gap-0.5">
            {!env.ffmpeg && <span>未检测到 ffmpeg（抽帧需要），请安装并加入 PATH。</span>}
            {!env.python && <span>未检测到 Python 3，请安装并加入 PATH。</span>}
            {!env.sidecar && <span>未找到 OCR sidecar（src-tauri/python/main.py）。</span>}
          </div>
        </div>
      )}

      <div className="flex flex-1 overflow-hidden">
        {/* ── 左：参数面板 ── */}
        <div className="flex w-72 shrink-0 flex-col gap-4 overflow-y-auto border-r border-zinc-800 p-4">
          <button onClick={pickVideo}
            className="flex items-center justify-center gap-2 rounded-xl border border-dashed border-zinc-700 py-4 text-sm text-zinc-400 transition-colors hover:border-zinc-500 hover:text-zinc-100">
            <Upload size={16} /> {videoPath ? "更换视频" : "选择视频"}
          </button>

          {videoPath && (
            <p className="break-all text-xs text-zinc-500" title={videoPath}>
              {videoPath.split(/[\\/]/).pop()}
            </p>
          )}

          {/* 预览时间 */}
          {videoPath && (
            <Field label="预览时间点（秒）">
              <div className="flex gap-2">
                <input type="number" min={0} value={previewTime}
                  onChange={(e) => setPreviewTime(Number(e.target.value))}
                  className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500" />
                <button onClick={() => extractFrame(videoPath, previewTime)} disabled={extracting}
                  className="flex shrink-0 items-center gap-1 rounded-lg bg-zinc-800 px-3 text-sm text-zinc-300 hover:bg-zinc-700 disabled:opacity-50">
                  {extracting ? <Loader2 size={13} className="animate-spin" /> : <ImageIcon size={13} />}
                  抽帧
                </button>
              </div>
            </Field>
          )}

          <Field label="OCR 引擎">
            <select value={engine} onChange={(e) => setEngine(e.target.value)}
              className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500">
              {ENGINES.map((x) => <option key={x.value} value={x.value}>{x.label}</option>)}
            </select>
          </Field>

          <Field label="字幕语言">
            <select value={language} onChange={(e) => setLanguage(e.target.value)}
              className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500">
              {LANGUAGES.map((l) => <option key={l.value} value={l.value}>{l.label}</option>)}
            </select>
          </Field>

          <Field label={`扫描帧率：${sampleFps} fps`}>
            <input type="range" min={1} max={10} step={1} value={sampleFps}
              onChange={(e) => setSampleFps(Number(e.target.value))} className="w-full accent-blue-500" />
            <p className="mt-1 text-xs text-zinc-600">每秒检查几次字幕区域。OCR 只在字幕变化时才跑，所以调高主要是为了不漏短字幕，开销不大</p>
          </Field>

          <Field label={`相似度阈值：${similarity.toFixed(2)}`}>
            <input type="range" min={0.5} max={1} step={0.01} value={similarity}
              onChange={(e) => setSimilarity(Number(e.target.value))} className="w-full accent-blue-500" />
            <p className="mt-1 text-xs text-zinc-600">高于此值的相邻字幕视为同一句</p>
          </Field>

          <div className="rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-xs">
            <span className="text-zinc-500">字幕区域：</span>
            {region ? (
              <span className="text-zinc-300">
                x{region.x.toFixed(2)} y{region.y.toFixed(2)} w{region.w.toFixed(2)} h{region.h.toFixed(2)}
              </span>
            ) : (
              <span className="text-amber-500">请在预览图上拖拽框选</span>
            )}
          </div>

          {/* 操作按钮 */}
          {runState === "running" ? (
            <button onClick={cancel}
              className="flex items-center justify-center gap-2 rounded-lg bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-500 transition-colors">
              <Square size={14} /> 取消
            </button>
          ) : (
            <button onClick={start} disabled={!canStart}
              className="flex items-center justify-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-40 transition-colors">
              <Play size={14} /> 开始识别
            </button>
          )}
        </div>

        {/* ── 右：依赖 + 预览 + 进度 + 结果 ── */}
        <div className="flex flex-1 flex-col gap-4 overflow-y-auto p-5">
          {/* Python 依赖检测 / 一键安装 */}
          <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4">
            <div className="mb-3 flex items-center gap-2 text-sm text-zinc-300">
              <Package size={14} className="text-blue-400" /> Python 依赖
              <button onClick={checkDeps} disabled={depsLoading || installing}
                title="重新检测"
                className="ml-auto flex items-center gap-1 rounded px-2 py-1 text-xs text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100 disabled:opacity-50">
                <RefreshCw size={12} className={depsLoading ? "animate-spin" : ""} /> 重新检测
              </button>
            </div>

            {!deps?.python && (
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2 text-xs text-red-400">
                  <AlertCircle size={13} /> 未检测到 Python 3，请先安装并加入 PATH，或手动指定。
                </div>
                <button onClick={pickPython}
                  className="flex w-fit items-center gap-1.5 rounded-lg border border-zinc-700 px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-800">
                  <Upload size={12} /> 选择 Python 解释器
                </button>
              </div>
            )}

            {deps?.python && (
              <>
                {/* 解释器信息 + 版本/兼容性 */}
                <div className="mb-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                  <span className="text-zinc-500">解释器：</span>
                  <span className="font-mono text-zinc-400" title={deps.python}>
                    {deps.python_version ?? deps.python}
                  </span>
                  {deps.configured && <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-zinc-500">手动指定</span>}
                  <button onClick={pickPython}
                    className="text-blue-400 hover:underline">更改</button>
                  {deps.configured && (
                    <button onClick={resetPython} className="text-zinc-500 hover:underline">恢复自动</button>
                  )}
                </div>

                {pythonError && (
                  <div className="mb-2 flex items-center gap-2 text-xs text-red-400">
                    <AlertCircle size={12} /> {pythonError}
                  </div>
                )}

                {/* 版本不兼容警告 */}
                {!deps.compatible && (
                  <div className="mb-3 flex flex-col gap-1.5 rounded-lg border border-amber-900/50 bg-amber-950/30 p-3 text-xs text-amber-400">
                    <div className="flex items-start gap-2">
                      <AlertCircle size={13} className="mt-0.5 shrink-0" />
                      <span>
                        当前 Python 版本过新，OCR 依赖（paddle / onnxruntime）暂无对应安装包（需 <b>3.8 ~ 3.13</b>）。
                        请安装一个 3.12 / 3.13，再点上方「更改」指向它的 python.exe。
                      </span>
                    </div>
                  </div>
                )}

                <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 sm:grid-cols-3">
                  {deps.deps.map((d) => (
                    <div key={d.module} className="flex items-center gap-1.5 text-xs">
                      {d.installed
                        ? <CheckCircle2 size={12} className="shrink-0 text-green-400" />
                        : <X size={12} className="shrink-0 text-red-400" />}
                      <span className={d.installed ? "text-zinc-400" : "text-zinc-300"}>{d.package}</span>
                    </div>
                  ))}
                </div>

                {deps.all_ok ? (
                  <div className="mt-3 flex items-center gap-2 text-xs text-green-400">
                    <CheckCircle2 size={13} /> 依赖已就绪，可以开始识别。
                  </div>
                ) : (
                  <div className="mt-3 flex flex-col gap-2">
                    <label className="flex items-center gap-2 text-xs text-zinc-400">
                      <input type="checkbox" checked={useMirror}
                        onChange={(e) => setUseMirror(e.target.checked)}
                        disabled={installing} className="accent-blue-500" />
                      使用清华镜像加速（国内推荐）
                    </label>
                    <button onClick={installDeps} disabled={installing || !deps.compatible}
                      title={!deps.compatible ? "请先切换到兼容的 Python 版本" : ""}
                      className="flex w-fit items-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors">
                      {installing
                        ? <><Loader2 size={14} className="animate-spin" /> 安装中…</>
                        : <><Download size={14} /> 一键安装依赖</>}
                    </button>
                    <p className="text-xs text-zinc-600">
                      {engine === "rapidocr"
                        ? "RapidOCR 较轻量（含模型约几十 MB），装好即可用。"
                        : "PaddleOCR 首次会下载 paddlepaddle 与模型（数百 MB），耗时较长，请保持联网。"}
                    </p>
                  </div>
                )}

                {installLogs.length > 0 && (
                  <div ref={installLogRef} className="mt-3 max-h-44 overflow-auto rounded-lg bg-zinc-950 p-3 font-mono text-xs leading-relaxed text-zinc-500">
                    {installLogs.map((l, i) => <div key={i} className="whitespace-pre-wrap break-all">{l}</div>)}
                  </div>
                )}
              </>
            )}
          </div>

          {/* 预览图 + 框选 */}
          <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-3">
            <div className="mb-2 flex items-center gap-2 text-sm text-zinc-300">
              <Crop size={14} className="text-blue-400" /> 字幕区域框选
            </div>
            {previewSrc ? (
              <div className="relative inline-block select-none">
                <img ref={imgRef} src={previewSrc} alt="预览"
                  draggable={false}
                  className="block max-h-[50vh] w-auto rounded-lg"
                  onPointerDown={onPointerDown}
                  onPointerMove={onPointerMove}
                  onPointerUp={onPointerUp} />
                {rect && (
                  <div className="pointer-events-none absolute border-2 border-blue-400 bg-blue-400/20"
                    style={{
                      left: `${rect.left * 100}%`, top: `${rect.top * 100}%`,
                      width: `${rect.width * 100}%`, height: `${rect.height * 100}%`,
                    }} />
                )}
              </div>
            ) : (
              <div className="flex h-48 items-center justify-center text-sm text-zinc-600">
                {extracting ? <><Loader2 size={16} className="mr-2 animate-spin" /> 抽帧中…</>
                  : "选择视频后在此框选字幕区域"}
              </div>
            )}
          </div>

          {/* 进度 */}
          {runState !== "idle" && (
            <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4">
              <div className="mb-2 flex items-center gap-2 text-sm">
                {runState === "running" && <><Loader2 size={14} className="animate-spin text-blue-400" /> <span className="text-zinc-300">识别中…</span></>}
                {runState === "done" && <><CheckCircle2 size={14} className="text-green-400" /> <span className="text-zinc-300">完成</span></>}
                {runState === "error" && <><AlertCircle size={14} className="text-red-400" /> <span className="text-zinc-300">出错</span></>}
                {runState === "cancelled" && <><X size={14} className="text-zinc-400" /> <span className="text-zinc-300">已取消</span></>}
                {progress.total > 0 && (
                  <span className="ml-auto text-xs tabular-nums text-zinc-500">
                    {progress.current} / {progress.total}
                  </span>
                )}
              </div>
              <div className="h-2 overflow-hidden rounded-full bg-zinc-800">
                <div className="h-full rounded-full bg-blue-500 transition-all"
                  style={{ width: `${runState === "done" ? 100 : pct}%` }} />
              </div>
              {errorMsg && <p className="mt-2 text-xs text-red-400">{errorMsg}</p>}

              {outputPath && (
                <div className="mt-3 flex items-center gap-2 text-xs">
                  <CheckCircle2 size={12} className="shrink-0 text-green-400" />
                  <span className="min-w-0 flex-1 truncate text-zinc-400" title={outputPath}>{outputPath}</span>
                  <button onClick={() => openPath(dirOf(outputPath))}
                    className="flex items-center gap-1 rounded px-2 py-1 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100">
                    <FolderOpen size={12} /> 打开目录
                  </button>
                </div>
              )}
            </div>
          )}

          {/* SRT 预览 */}
          {srtText && (
            <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4">
              <div className="mb-2 flex items-center gap-2 text-sm text-zinc-300">
                <FileText size={14} className="text-blue-400" /> SRT 预览
              </div>
              <pre className="max-h-72 overflow-auto whitespace-pre-wrap rounded-lg bg-zinc-950 p-3 text-xs leading-relaxed text-zinc-300">
                {srtText}
              </pre>
            </div>
          )}

          {/* 日志 */}
          {logs.length > 0 && (
            <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4">
              <div className="mb-2 text-sm text-zinc-300">运行日志</div>
              <div ref={logBoxRef} className="max-h-40 overflow-auto rounded-lg bg-zinc-950 p-3 font-mono text-xs leading-relaxed text-zinc-500">
                {logs.map((l, i) => <div key={i}>{l}</div>)}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-xs font-medium text-zinc-400">{label}</label>
      {children}
    </div>
  );
}
