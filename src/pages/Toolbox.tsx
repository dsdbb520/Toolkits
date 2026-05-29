import { useNavigate } from "react-router-dom";
import {
  FileText,
  Download,
  Gamepad2,
  Camera,
  ImageIcon,
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
    to: "/toolbox/steam",
    icon: Gamepad2,
    label: "Steam 切换",
    desc: "快速切换 Steam 账号",
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

  return (
    <div className="flex h-full flex-col p-6">
      <h1 className="mb-6 text-xl font-semibold text-zinc-100">工具箱</h1>
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
        {tools.map(({ to, icon: Icon, label, desc }) => (
          <button
            key={to}
            onClick={() => navigate(to)}
            className="flex flex-col gap-3 rounded-xl border border-zinc-800 bg-zinc-900 p-5 text-left transition-colors hover:border-zinc-600 hover:bg-zinc-800"
          >
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-zinc-800">
              <Icon size={20} className="text-blue-400" />
            </div>
            <div>
              <p className="text-sm font-medium text-zinc-100">{label}</p>
              <p className="mt-0.5 text-xs text-zinc-500">{desc}</p>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
