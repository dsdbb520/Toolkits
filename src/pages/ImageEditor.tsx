import { useEffect, useRef, useState, useCallback } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import {
  ArrowLeft, ImageIcon, Upload, X, Loader2, CheckCircle2,
  AlertCircle, FolderOpen, Crop, RotateCw, Maximize2, FileDown,
  RefreshCw, SlidersHorizontal, Scissors, ZoomIn, ZoomOut,
  Save, FilePen,
} from "lucide-react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";

interface ImageInfo { path: string; filename: string; width: number; height: number; format: string; size_bytes: number }
interface ImageOpResult { output_path: string; output_size_bytes: number; width: number; height: number }
type ProcessState = "idle" | "processing" | "done" | "error";

const FORMATS = ["jpg","jpeg","png","webp","bmp","tiff"] as const;
const COMPRESS_FORMATS = ["jpg","webp","png"] as const;
const clamp = (v:number,lo:number,hi:number)=>Math.max(lo,Math.min(v,hi));
function fmtSize(b:number){if(!b)return"—";if(b<1<<10)return`${b} B`;if(b<1<<20)return`${(b>>10).toFixed(0)} KB`;if(b<1<<30)return`${(b/(1<<20)).toFixed(1)} MB`;return`${(b/(1<<30)).toFixed(2)} GB`;}
function dirOf(p:string){const i=Math.max(p.lastIndexOf("/"),p.lastIndexOf("\\"));return i>0?p.substring(0,i):"."}

export default function ImageEditor(){
  const navigate=useNavigate();
  const location=useLocation();

  const [imgInfo,setImgInfo]=useState<ImageInfo|null>(null);
  const [imgSrc,setImgSrc]=useState("");
  const [loadingInfo,setLoadingInfo]=useState(false);
  const [infoError,setInfoError]=useState("");

  const canvasRef=useRef<HTMLCanvasElement>(null);
  const canvasWrapRef=useRef<HTMLDivElement>(null);
  const imageRef=useRef<HTMLImageElement|null>(null);
  const [displayZoom,setDisplayZoom]=useState(100); // 用户可调的缩放百分比
  const [displayScale,setDisplayScale]=useState(1);
  const [canvasW,setCanvasW]=useState(0);
  const [canvasH,setCanvasH]=useState(0);

  const [cropMode,setCropMode]=useState(false);
  const [cropRect,setCropRect]=useState({x:0,y:0,w:0,h:0});
  const dragState=useRef<{active:boolean;handle:"tl"|"tr"|"bl"|"br"|"move"|null;startX:number;startY:number;startRect:{x:number;y:number;w:number;h:number}}>({active:false,handle:null,startX:0,startY:0,startRect:{x:0,y:0,w:0,h:0}});
  const [cropRes,setCropRes]=useState<ImageOpResult|null>(null);
  const [cropState,setCropState]=useState<ProcessState>("idle");
  const [cropError,setCropError]=useState("");

  const [rotation,setRotation]=useState(0);
  const [rotateState,setRotateState]=useState<ProcessState>("idle");
  const [rotateRes,setRotateRes]=useState<ImageOpResult|null>(null);
  const [rotateError,setRotateError]=useState("");

  const [resizeW,setResizeW]=useState(0);
  const [resizeH,setResizeH]=useState(0);
  const [keepRatio,setKeepRatio]=useState(true);
  const [resizeState,setResizeState]=useState<ProcessState>("idle");
  const [resizeRes,setResizeRes]=useState<ImageOpResult|null>(null);
  const [resizeError,setResizeError]=useState("");

  const [quality,setQuality]=useState(80);
  const [compressFmt,setCompressFmt]=useState<typeof COMPRESS_FORMATS[number]>("jpg");
  const [compressState,setCompressState]=useState<ProcessState>("idle");
  const [compressRes,setCompressRes]=useState<ImageOpResult|null>(null);
  const [compressError,setCompressError]=useState("");

  const [convertFmt,setConvertFmt]=useState<typeof FORMATS[number]>("png");
  const [convertState,setConvertState]=useState<ProcessState>("idle");
  const [convertRes,setConvertRes]=useState<ImageOpResult|null>(null);
  const [convertError,setConvertError]=useState("");

  const resetAll=()=>{setCropMode(false);setCropRect({x:0,y:0,w:0,h:0});setCropRes(null);setCropState("idle");setCropError("");setRotation(0);setRotateRes(null);setRotateState("idle");setRotateError("");setResizeW(0);setResizeH(0);setResizeRes(null);setResizeState("idle");setResizeError("");setQuality(80);setCompressFmt("jpg");setCompressRes(null);setCompressState("idle");setCompressError("");setConvertFmt("png");setConvertRes(null);setConvertState("idle");setConvertError("");};

  const loadFile=async(path:string)=>{setLoadingInfo(true);setInfoError("");setImgInfo(null);setImgSrc("");resetAll();try{const info=await invoke<ImageInfo>("image_get_info",{path});setImgInfo(info);setImgSrc(convertFileSrc(path));setResizeW(info.width);setResizeH(info.height);setCropRect({x:0,y:0,w:info.width,h:info.height});}catch(e){setInfoError(String(e))}finally{setLoadingInfo(false)}};

  useEffect(()=>{const fp=(location.state as {filePath?:string}|null)?.filePath;if(fp)loadFile(fp)},[]);

  const handleOpenFile=async()=>{const p=await invoke<string|null>("image_open_file");if(p)loadFile(p)};

  // ─── Canvas sizing: zoom% × fit-to-container ───
  const computeCanvasSize=useCallback((zoom:number=displayZoom)=>{
    const img=imageRef.current;
    const wrap=canvasWrapRef.current;
    if(!img||!wrap)return;
    requestAnimationFrame(()=>{
      if(!wrap||!img)return;
      const availW=wrap.clientWidth-32;
      const fitScale=Math.min(availW/img.naturalWidth,1); // 适配容器宽度
      const userScale=zoom/100;
      const scale=fitScale*userScale;
      setDisplayScale(scale);
      setCanvasW(Math.round(img.naturalWidth*scale));
      setCanvasH(Math.round(img.naturalHeight*scale));
    });
  },[displayZoom]);

  // 加载图片后测量
  useEffect(()=>{
    if(!imgSrc)return;
    const img=new Image();
    img.onload=()=>{imageRef.current=img;setDisplayZoom(100);computeCanvasSize(100)};
    img.src=imgSrc;
  },[imgSrc,computeCanvasSize]);

  // ResizeObserver + zoom变化重新计算
  useEffect(()=>{
    const wrap=canvasWrapRef.current;
    if(!wrap)return;
    const ro=new ResizeObserver(()=>computeCanvasSize());
    ro.observe(wrap);
    return()=>ro.disconnect();
  },[computeCanvasSize]);

  useEffect(()=>{computeCanvasSize()},[displayZoom,computeCanvasSize]);

  // ─── Canvas drawing ───
  const drawCanvas=useCallback(()=>{
    const canvas=canvasRef.current;
    const img=imageRef.current;
    if(!canvas||!img)return;
    const ctx=canvas.getContext("2d")!;
    const cw=canvas.width,ch=canvas.height;
    ctx.clearRect(0,0,cw,ch);
    if(rotation!==0&&!cropMode){
      ctx.save();
      ctx.translate(cw/2,ch/2);
      ctx.rotate(rotation*Math.PI/180);
      ctx.drawImage(img,-cw/2,-ch/2,cw,ch);
      ctx.restore();
    }else{
      ctx.drawImage(img,0,0,cw,ch);
    }

    if(cropMode&&cropRect.w>0&&cropRect.h>0){
      const s=displayScale;
      const rx=Math.round(cropRect.x*s);
      const ry=Math.round(cropRect.y*s);
      const rw=Math.round(cropRect.w*s);
      const rh=Math.round(cropRect.h*s);

      ctx.fillStyle="rgba(0,0,0,0.55)";
      ctx.fillRect(0,0,cw,ry);
      ctx.fillRect(0,ry+rh,cw,ch-ry-rh);
      ctx.fillRect(0,ry,rx,rh);
      ctx.fillRect(rx+rw,ry,cw-rx-rw,rh);

      ctx.strokeStyle="#3b82f6";ctx.lineWidth=2;
      ctx.setLineDash([5,3]);ctx.strokeRect(rx,ry,rw,rh);ctx.setLineDash([]);

      ctx.fillStyle="#3b82f6";
      [[rx,ry],[rx+rw,ry],[rx,ry+rh],[rx+rw,ry+rh]].forEach(([hx,hy])=>ctx.fillRect(hx-4,hy-4,8,8));

      ctx.fillStyle="rgba(0,0,0,0.85)";
      const label=`${cropRect.w} × ${cropRect.h}`;
      ctx.font="12px monospace";const lm=ctx.measureText(label);
      const ly=ry-6>20?ry-6:ry+rh+22;
      ctx.fillRect(rx,ly-14,lm.width+10,18);
      ctx.fillStyle="#fff";ctx.fillText(label,rx+5,ly);
    }
  },[cropMode,cropRect,displayScale,rotation]);
  useEffect(()=>{drawCanvas()},[drawCanvas]);

  // ─── Crop drag ───
  const getHandle=(cx:number,cy:number)=>{
    const s=displayScale;
    const rx=Math.round(cropRect.x*s),ry=Math.round(cropRect.y*s),rw=Math.round(cropRect.w*s),rh=Math.round(cropRect.h*s);
    const hs=12;
    if(Math.abs(cx-rx)<=hs&&Math.abs(cy-ry)<=hs)return"tl";
    if(Math.abs(cx-(rx+rw))<=hs&&Math.abs(cy-ry)<=hs)return"tr";
    if(Math.abs(cx-rx)<=hs&&Math.abs(cy-(ry+rh))<=hs)return"bl";
    if(Math.abs(cx-(rx+rw))<=hs&&Math.abs(cy-(ry+rh))<=hs)return"br";
    if(cx>=rx&&cx<=rx+rw&&cy>=ry&&cy<=ry+rh)return"move";
    return null;
  };

  const handleCanvasPointerDown=(e:React.PointerEvent)=>{
    if(!cropMode||!imgInfo)return;
    const c=canvasRef.current;if(!c)return;
    const r=c.getBoundingClientRect();
    const cx=e.clientX-r.left,cy=e.clientY-r.top;
    const h=getHandle(cx,cy);
    if(h){e.currentTarget.setPointerCapture(e.pointerId);dragState.current={active:true,handle:h,startX:cx,startY:cy,startRect:{...cropRect}}}
  };

  const handleCanvasPointerMove=(e:React.PointerEvent)=>{
    if(!dragState.current.active||!imgInfo)return;
    const c=canvasRef.current;if(!c)return;
    const r=c.getBoundingClientRect();
    const cx=e.clientX-r.left,cy=e.clientY-r.top;
    const dx=Math.round((cx-dragState.current.startX)/displayScale);
    const dy=Math.round((cy-dragState.current.startY)/displayScale);
    const sr=dragState.current.startRect;
    const clX=(v:number)=>clamp(v,0,imgInfo.width);
    const clY=(v:number)=>clamp(v,0,imgInfo.height);
    let{x,y,w,h}=sr;
    switch(dragState.current.handle){
      case"move":x=clX(sr.x+dx);y=clY(sr.y+dy);if(x+w>imgInfo.width)x=imgInfo.width-w;if(y+h>imgInfo.height)y=imgInfo.height-h;setCropRect({x,y,w,h});break;
      case"tl":x=clX(sr.x+dx);y=clY(sr.y+dy);w=sr.x+sr.w-x;h=sr.y+sr.h-y;if(w<8){x=sr.x+sr.w-8;w=8}if(h<8){y=sr.y+sr.h-8;h=8}setCropRect({x,y,w,h});break;
      case"tr":w=Math.max(8,sr.w+dx);{const ny=clY(sr.y+dy);h=sr.y+sr.h-ny;y=h<8?sr.y+sr.h-8:ny;h=Math.max(8,h)}setCropRect({x:sr.x,y,w,h});break;
      case"bl":{const nx=clX(sr.x+dx);w=sr.x+sr.w-nx;x=w<8?sr.x+sr.w-8:nx;w=Math.max(8,w)}h=Math.max(8,sr.h+dy);setCropRect({x,y:sr.y,w,h});break;
      case"br":w=Math.max(8,sr.w+dx);h=Math.max(8,sr.h+dy);setCropRect({x:sr.x,y:sr.y,w,h});break;
    }
  };

  const handleCanvasPointerUp=()=>{dragState.current.active=false};

  const enterCropMode=()=>{
    if(!imgInfo)return;
    setCropRect({x:Math.round(imgInfo.width*.05),y:Math.round(imgInfo.height*.05),w:Math.round(imgInfo.width*.9),h:Math.round(imgInfo.height*.9)});
    setCropMode(true);setCropRes(null);setCropState("idle");setCropError("");
  };

  const handleCrop=async()=>{if(!imgInfo||cropRect.w<1||cropRect.h<1)return;setCropState("processing");try{const res=await invoke<ImageOpResult>("image_crop",{path:imgInfo.path,x:cropRect.x,y:cropRect.y,width:cropRect.w,height:cropRect.h});setCropRes(res);setCropState("done")}catch(e){setCropError(String(e));setCropState("error")}};
  const handleRotate=async()=>{if(!imgInfo)return;setRotateState("processing");try{const res=await invoke<ImageOpResult>("image_rotate",{path:imgInfo.path,degrees:rotation});setRotateRes(res);setRotateState("done")}catch(e){setRotateError(String(e));setRotateState("error")}};
  const handleResize=async()=>{if(!imgInfo||resizeW<1||resizeH<1)return;setResizeState("processing");try{const res=await invoke<ImageOpResult>("image_resize",{path:imgInfo.path,targetWidth:resizeW,targetHeight:resizeH,keepRatio});setResizeRes(res);setResizeState("done")}catch(e){setResizeError(String(e));setResizeState("error")}};
  const handleCompress=async()=>{if(!imgInfo)return;setCompressState("processing");try{const res=await invoke<ImageOpResult>("image_compress",{path:imgInfo.path,quality,format:compressFmt});setCompressRes(res);setCompressState("done")}catch(e){setCompressError(String(e));setCompressState("error")}};
  const handleConvert=async()=>{if(!imgInfo)return;setConvertState("processing");try{const res=await invoke<ImageOpResult>("image_convert",{path:imgInfo.path,outputFormat:convertFmt});setConvertRes(res);setConvertState("done")}catch(e){setConvertError(String(e));setConvertState("error")}};

  const zoomMin=10,zoomMax=300,zoomStep=10;

  return(
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-zinc-800 px-4 py-3">
        <button onClick={()=>navigate("/toolbox")} className="flex items-center gap-1.5 text-sm text-zinc-400 hover:text-zinc-100 transition-colors"><ArrowLeft size={15}/>工具箱</button>
        <span className="text-zinc-600">/</span>
        <span className="text-sm text-zinc-100">图片编辑</span>
      </header>

      <div className="flex flex-1 overflow-hidden">
        <div className="flex w-56 shrink-0 flex-col gap-4 overflow-y-auto border-r border-zinc-800 p-4">
          <button onClick={handleOpenFile} className="flex items-center justify-center gap-2 rounded-xl border border-dashed border-zinc-700 py-3 text-sm text-zinc-400 transition-colors hover:border-zinc-500 hover:text-zinc-100">
            <Upload size={15}/>{imgInfo?"更换图片":"打开图片"}
          </button>
          {loadingInfo&&<div className="flex items-center gap-2 text-xs text-zinc-400"><Loader2 size={12} className="animate-spin"/>读取中…</div>}
          {infoError&&<div className="flex items-start gap-2 rounded-lg border border-red-800 bg-red-950/40 px-3 py-2 text-xs text-red-400"><AlertCircle size={12} className="mt-0.5 shrink-0"/><span className="flex-1 break-all">{infoError}</span><button onClick={()=>setInfoError("")}><X size={11}/></button></div>}
          {imgInfo&&(
            <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-3 flex flex-col gap-2">
              <div className="flex items-start gap-2"><ImageIcon size={14} className="mt-0.5 shrink-0 text-blue-400"/><p className="text-xs font-medium text-zinc-100 break-all leading-snug">{imgInfo.filename}</p></div>
              <div className="flex flex-col gap-1">
                <InfoRow label="分辨率" value={`${imgInfo.width} × ${imgInfo.height}`}/>
                <InfoRow label="格式" value={imgInfo.format}/>
                <InfoRow label="大小" value={fmtSize(imgInfo.size_bytes)}/>
              </div>
            </div>
          )}
        </div>

        <div className="flex flex-1 flex-col overflow-y-auto">
          {!imgInfo&&!loadingInfo&&(
            <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-600">
              <ImageIcon size={40}/><p className="text-sm">打开图片文件开始编辑</p><p className="text-xs text-zinc-700">支持 JPG · PNG · WebP · BMP · TIFF · GIF</p>
            </div>
          )}

          {imgInfo&&(<>
            {/* ─── 缩放控制条 ─── */}
            <div className="mx-5 mt-4 flex items-center gap-3 rounded-xl border border-zinc-800 bg-zinc-900 px-4 py-2.5">
              <ZoomOut size={14} className="text-zinc-500"/>
              <input type="range" min={zoomMin} max={zoomMax} step={zoomStep} value={displayZoom} onChange={e=>setDisplayZoom(Number(e.target.value))} className="w-32 accent-blue-500"/>
              <ZoomIn size={16} className="text-zinc-500"/>
              <span className="text-xs font-mono text-zinc-300 tabular-nums w-10 text-center">{displayZoom}%</span>
              <button onClick={()=>setDisplayZoom(100)} className="text-xs text-zinc-500 hover:text-zinc-300 transition-colors">重置</button>
              <span className="text-xs text-zinc-500 ml-2">显示尺寸: {canvasW} × {canvasH}</span>
            </div>

            {/* ─── 裁剪工具栏 ─── */}
            <div className="mx-5 mt-3 flex items-center gap-3 rounded-xl border px-4 py-3"
              style={{ borderColor: cropMode?"#2563eb":"rgb(63,63,70)", backgroundColor: cropMode?"rgba(37,99,235,0.08)":"rgb(24,24,27)" }}>
              {!cropMode?(
                <>
                  <Scissors size={16} className="text-blue-400"/>
                  <span className="text-sm text-zinc-300 flex-1">裁剪图片 — 拖拽选区，按比例精确裁切</span>
                  <button onClick={enterCropMode} className="flex items-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-500 transition-colors"><Crop size={14}/>进入裁剪</button>
                </>
              ):(
                <>
                  <Crop size={16} className="text-blue-400"/>
                  <span className="text-sm text-zinc-400">选区</span>
                  <span className="text-sm font-mono text-zinc-100 tabular-nums">{cropRect.w} × {cropRect.h}</span>
                  <span className="text-xs text-zinc-500">（拖动四角调整）</span>
                  <div className="flex-1"/>
                  <button onClick={()=>setCropMode(false)} className="rounded-lg px-3 py-1.5 text-xs text-zinc-400 hover:text-zinc-100 transition-colors">取消</button>
                  <button onClick={handleCrop} disabled={cropState==="processing"} className="flex items-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors">
                    {cropState==="processing"?<Loader2 size={14} className="animate-spin"/>:<Crop size={14}/>}裁剪
                  </button>
                </>
              )}
            </div>

            {/* ─── Canvas (可滚动) ─── */}
            <div className="mx-5 mt-4 rounded-xl border border-zinc-800 bg-zinc-900 overflow-auto" style={{maxHeight:"70vh"}}>
              <div ref={canvasWrapRef} className="flex items-start justify-center bg-zinc-950 p-4 min-h-[200px]">
                <canvas
                  ref={canvasRef}
                  width={canvasW}
                  height={canvasH}
                  style={{width:canvasW,height:canvasH,touchAction:"none"}}
                  className={`rounded border border-zinc-800 ${cropMode?"cursor-crosshair":""}`}
                  onPointerDown={handleCanvasPointerDown}
                  onPointerMove={handleCanvasPointerMove}
                  onPointerUp={handleCanvasPointerUp}
                  onPointerLeave={handleCanvasPointerUp}
                />
              </div>
              <OpResult state={cropState} result={cropRes?.output_path??""} error={cropError} info={cropRes?`${cropRes.width}×${cropRes.height}`:""} originalPath={imgInfo?.path}/>
            </div>

            {/* ─── 其他编辑卡片 ─── */}
            <div className="px-5 py-5 flex flex-col gap-5">
              <OpCard icon={<RotateCw size={14}/>} title="旋转" desc="以图片中心旋转任意角度">
                <div className="flex flex-wrap items-center gap-4">
                  <input type="range" min={-180} max={180} value={rotation} onChange={e=>setRotation(Number(e.target.value))} className="w-40 accent-blue-500"/>
                  <span className="text-sm font-mono text-zinc-100 tabular-nums w-12 text-center">{rotation}°</span>
                  <button onClick={()=>setRotation(0)} className="text-xs text-zinc-500 hover:text-zinc-300">重置</button>
                  <button onClick={handleRotate} disabled={rotateState==="processing"||rotation===0} className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors ml-auto">
                    {rotateState==="processing"?<Loader2 size={13} className="animate-spin"/>:<RotateCw size={13}/>}旋转
                  </button>
                </div>
                <OpResult state={rotateState} result={rotateRes?.output_path??""} error={rotateError} info={rotateRes?`${rotateRes.width}×${rotateRes.height} — ${fmtSize(rotateRes.output_size_bytes)}`:""} originalPath={imgInfo?.path}/>
              </OpCard>

              <OpCard icon={<Maximize2 size={14}/>} title="调整分辨率" desc="等比例缩放或拉伸">
                <div className="flex flex-wrap items-center gap-3">
                  <input type="number" value={resizeW||""} onChange={e=>{const v=Number(e.target.value)||0;setResizeW(v);if(keepRatio&&imgInfo&&v>0)setResizeH(Math.round(v*imgInfo.height/imgInfo.width))}} placeholder="宽度" className="w-24 rounded border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500"/>
                  <span className="text-zinc-500">×</span>
                  <input type="number" value={resizeH||""} onChange={e=>{const v=Number(e.target.value)||0;setResizeH(v);if(keepRatio&&imgInfo&&v>0)setResizeW(Math.round(v*imgInfo.width/imgInfo.height))}} placeholder="高度" className="w-24 rounded border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500"/>
                  <label className="flex items-center gap-1.5 text-xs text-zinc-400 cursor-pointer select-none"><input type="checkbox" checked={keepRatio} onChange={e=>setKeepRatio(e.target.checked)} className="accent-blue-500"/>等比例</label>
                  <button onClick={handleResize} disabled={resizeState==="processing"||resizeW<1||resizeH<1} className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors ml-auto">
                    {resizeState==="processing"?<Loader2 size={13} className="animate-spin"/>:<Maximize2 size={13}/>}调整分辨率
                  </button>
                </div>
                <OpResult state={resizeState} result={resizeRes?.output_path??""} error={resizeError} info={resizeRes?`${resizeRes.width}×${resizeRes.height} — ${fmtSize(resizeRes.output_size_bytes)}`:""} originalPath={imgInfo?.path}/>
              </OpCard>

              <OpCard icon={<SlidersHorizontal size={14}/>} title="压缩体积" desc="调整画质或转换格式以减小文件大小">
                <div className="flex flex-wrap items-center gap-4">
                  <select value={compressFmt} onChange={e=>setCompressFmt(e.target.value as typeof COMPRESS_FORMATS[number])} className="rounded border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500">{COMPRESS_FORMATS.map(f=><option key={f} value={f}>{f.toUpperCase()}</option>)}</select>
                  <span className="text-xs text-zinc-500">质量</span>
                  <input type="range" min={1} max={100} value={quality} onChange={e=>setQuality(Number(e.target.value))} className="w-32 accent-blue-500"/>
                  <span className="text-sm font-mono text-zinc-100 tabular-nums w-8">{quality}</span>
                  <button onClick={handleCompress} disabled={compressState==="processing"} className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors ml-auto">
                    {compressState==="processing"?<Loader2 size={13} className="animate-spin"/>:<FileDown size={13}/>}压缩
                  </button>
                </div>
                <OpResult state={compressState} result={compressRes?.output_path??""} error={compressError} info={compressRes?`${fmtSize(compressRes.output_size_bytes)}${imgInfo&&compressRes.output_size_bytes<imgInfo.size_bytes?` — 减少${Math.round((1-compressRes.output_size_bytes/imgInfo.size_bytes)*100)}%`:""}`:""} originalPath={imgInfo?.path}/>
              </OpCard>

              <OpCard icon={<RefreshCw size={14}/>} title="格式转换" desc="将图片转换为其他格式">
                <div className="flex flex-wrap items-center gap-3">
                  <select value={convertFmt} onChange={e=>setConvertFmt(e.target.value as typeof FORMATS[number])} className="rounded border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 outline-none focus:border-blue-500">{FORMATS.map(f=><option key={f} value={f}>{f.toUpperCase()}</option>)}</select>
                  <button onClick={handleConvert} disabled={convertState==="processing"} className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors">
                    {convertState==="processing"?<Loader2 size={13} className="animate-spin"/>:<RefreshCw size={13}/>}转换
                  </button>
                </div>
                <OpResult state={convertState} result={convertRes?.output_path??""} error={convertError} info={convertRes?`${convertRes.width}×${convertRes.height} — ${fmtSize(convertRes.output_size_bytes)}`:""} originalPath={imgInfo?.path}/>
              </OpCard>
            </div>
          </>)}
        </div>
      </div>
    </div>
  );
}

function InfoRow({label,value}:{label:string;value:string}){return <div className="flex justify-between gap-2 text-xs"><span className="text-zinc-600">{label}</span><span className="text-zinc-400">{value}</span></div>}
function OpCard({icon,title,desc,children}:{icon:React.ReactNode;title:string;desc:string;children:React.ReactNode}){return <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-5 flex flex-col gap-4"><div className="flex items-center gap-2"><span className="text-blue-400">{icon}</span><span className="text-sm font-medium text-zinc-100">{title}</span><span className="text-xs text-zinc-500">{desc}</span></div>{children}</div>}
function OpResult({state,result,error,info,originalPath}:{state:ProcessState;result:string;error:string;info?:string;originalPath?:string}){
  const [savingAs,setSavingAs]=useState(false);
  const [overwriteDone,setOverwriteDone]=useState(false);
  const baseName=result.split(/[\\/]/).pop()??"output";
  const handleSaveAs=async()=>{setSavingAs(true);try{await invoke("file_save_as",{src:result,defaultName:baseName})}catch{/**/}setSavingAs(false)};
  const handleOverwrite=async()=>{if(!originalPath)return;try{await invoke("file_overwrite",{src:result,dst:originalPath});setOverwriteDone(true)}catch{/**/}};
  if(state==="idle")return null;
  if(state==="processing")return <div className="flex items-center gap-2 text-xs text-zinc-400 px-4 pb-3"><Loader2 size={12} className="animate-spin"/>处理中，请稍候…</div>;
  if(state==="error")return <div className="flex items-start gap-2 text-xs text-red-400 px-4 pb-3"><AlertCircle size={12} className="mt-0.5 shrink-0"/>{error}</div>;
  if(state==="done"&&result)return(
    <div className="flex flex-col gap-1 border-t border-zinc-800 px-4 py-3">
      <div className="flex items-center gap-2">
        <CheckCircle2 size={12} className="shrink-0 text-green-400"/>
        <span className="min-w-0 flex-1 truncate text-xs text-zinc-500" title={result}>{result}</span>
        {info&&<span className="shrink-0 text-xs text-zinc-600">{info}</span>}
      </div>
      <div className="flex items-center gap-1">
        <button onClick={handleSaveAs} disabled={savingAs} className="flex items-center gap-1 rounded px-2 py-1 text-xs text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100 disabled:opacity-50 transition-colors">
          {savingAs?<Loader2 size={11} className="animate-spin"/>:<Save size={11}/>}另存为…
        </button>
        {originalPath&&(
          <button onClick={handleOverwrite} disabled={overwriteDone} className={cn("flex items-center gap-1 rounded px-2 py-1 text-xs transition-colors",overwriteDone?"text-green-400":"text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100")}>
            {overwriteDone?<CheckCircle2 size={11}/>:<FilePen size={11}/>}{overwriteDone?"已覆盖":"覆盖原文件"}
          </button>
        )}
        <button onClick={()=>openPath(dirOf(result))} className="ml-auto flex items-center gap-1 rounded px-2 py-1 text-xs text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100 transition-colors">
          <FolderOpen size={11}/>打开目录
        </button>
      </div>
    </div>
  );
  return null;
}
