import { useNavigate } from "react-router-dom";
import { ArrowLeft, Download } from "lucide-react";

export default function BiliDownload() {
  const navigate = useNavigate();
  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-zinc-800 px-4 py-3">
        <button
          onClick={() => navigate("/toolbox")}
          className="flex items-center gap-1.5 text-sm text-zinc-400 hover:text-zinc-100 transition-colors"
        >
          <ArrowLeft size={15} />
          工具箱
        </button>
        <span className="text-zinc-600">/</span>
        <span className="text-sm text-zinc-100">B站下载</span>
      </header>
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-500">
        <Download size={40} />
        <p className="text-sm">B站下载 — 开发中</p>
      </div>
    </div>
  );
}
