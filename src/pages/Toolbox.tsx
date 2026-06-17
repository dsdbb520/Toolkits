import { useNavigate } from "react-router-dom";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  FileText,
  Download,
  Gamepad2,
  Camera,
  ImageIcon,
  Film,
  DollarSign,
  Heart,
  Captions,
} from "lucide-react";

const tools = [
  {
    to: "/toolbox/notes",
    icon: FileText,
    label: "备忘录",
    desc: "跨设备同步的 Markdown 笔记",
  },
  {
    to: "/toolbox/bili",
    icon: Download,
    label: "B站下载",
    desc: "下载 B站视频，支持多清晰度",
  },
  {
    to: "/toolbox/media",
    icon: Film,
    label: "媒体编辑",
    desc: "裁剪、提取音频、格式转换",
  },
  {
    to: "/toolbox/subtitle-ocr",
    icon: Captions,
    label: "硬字幕 OCR 对轴",
    desc: "OCR 扫描硬字幕，自动生成 SRT 外挂字幕",
  },
  {
    to: "/toolbox/steam",
    icon: Gamepad2,
    label: "Steam 切换",
    desc: "快速切换 Steam 账号",
  },
  {
    to: "/toolbox/steam-price",
    icon: DollarSign,
    label: "Steam 区域价格",
    desc: "对比各区游戏价格与锁区情况",
  },
  {
    to: "/toolbox/wishlist",
    icon: Heart,
    label: "心愿单 · 降价追踪",
    desc: "粘贴 Steam/PS/NS 链接，追踪价格与历史、降价提醒",
  },
  {
    to: "/toolbox/screenshot",
    icon: Camera,
    label: "截图",
    desc: "截图并快速标注",
  },
  {
    to: "/toolbox/image",
    icon: ImageIcon,
    label: "图片编辑",
    desc: "裁剪、调色、格式转换",
  },
];

export default function Toolbox() {
  const navigate = useNavigate();
  const [drops, setDrops] = useState(0); // 心愿单未读降价数（红点）

  // 进入工具箱时：① 每天首次触发一次云同步（重新取价 + 跨设备）② 读取未读降价数显示红点
  useEffect(() => {
    let alive = true;
    (async () => {
      const refreshDot = async () => {
        try {
          const n = await invoke<number>("wishlist_unseen_count");
          if (alive) setDrops(n);
        } catch { /* ignore */ }
      };
      await refreshDot();
      // 每天首次：本机 localStorage 记录日期，已登录同步账户才触发
      try {
        const today = new Date().toDateString();
        if (localStorage.getItem("wishlistSyncDate") !== today) {
          const conf = await invoke<{ token: string }>("get_sync_settings");
          if (conf?.token) {
            await invoke("wishlist_sync");
            localStorage.setItem("wishlistSyncDate", today);
            await refreshDot();
          }
        }
      } catch { /* 未登录/同步失败：忽略，价格仍可在页面内手动刷新 */ }
    })();
    return () => { alive = false; };
  }, []);

  return (
    <div className="flex h-full flex-col p-10">
      <h1 className="mb-10 text-2xl font-semibold text-zinc-100">工具箱</h1>
      <div className="grid grid-cols-2 gap-6 sm:grid-cols-3 lg:grid-cols-4">
        {tools.map(({ to, icon: Icon, label, desc }) => {
          const badge = to === "/toolbox/wishlist" && drops > 0 ? drops : 0;
          return (
            <button
              key={to}
              onClick={() => navigate(to)}
              className="relative flex flex-col gap-5 rounded-xl border border-zinc-800 bg-zinc-900 p-7 text-left transition-colors hover:border-zinc-600 hover:bg-zinc-800"
            >
              {badge > 0 && (
                <span className="absolute right-3 top-3 flex h-5 min-w-5 items-center justify-center rounded-full bg-red-500 px-1.5 text-xs font-medium text-white">
                  {badge}
                </span>
              )}
              <div className="flex h-13 w-13 items-center justify-center rounded-xl bg-zinc-800 p-3">
                <Icon size={24} className="text-blue-400" />
              </div>
              <div>
                <p className="text-base font-medium text-zinc-100">{label}</p>
                <p className="mt-1.5 text-sm leading-relaxed text-zinc-500">{desc}</p>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
