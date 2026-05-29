import { useEffect, useRef, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { useEditor, EditorContent } from "@tiptap/react";
import { StarterKit } from "@tiptap/starter-kit";
import { Underline } from "@tiptap/extension-underline";
import { TextStyle } from "@tiptap/extension-text-style";
import { Color } from "@tiptap/extension-color";
import { Highlight } from "@tiptap/extension-highlight";
import {
  ArrowLeft, Plus, Trash2, Search, FileText,
  Bold, Italic, Underline as UnderlineIcon,
  Heading1, Heading2, Heading3, List, ListOrdered,
} from "lucide-react";
import { cn } from "@/lib/utils";

interface Note {
  id: string;
  title: string;
  content: string;
  created_at: number;
  updated_at: number;
}

const PRESET_COLORS = [
  { label: "默认", value: "" },
  { label: "红", value: "#f87171" },
  { label: "橙", value: "#fb923c" },
  { label: "黄", value: "#facc15" },
  { label: "绿", value: "#4ade80" },
  { label: "蓝", value: "#60a5fa" },
  { label: "紫", value: "#c084fc" },
  { label: "粉", value: "#f472b6" },
];

function formatDate(ms: number) {
  const diff = Date.now() - ms;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return new Date(ms).toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
}

function ToolbarBtn({
  active, onClick, title, children,
}: {
  active?: boolean; onClick: () => void; title: string; children: React.ReactNode;
}) {
  return (
    <button
      onMouseDown={(e) => { e.preventDefault(); onClick(); }}
      title={title}
      className={cn(
        "flex h-7 w-7 items-center justify-center rounded transition-colors",
        active ? "bg-zinc-600 text-white" : "text-zinc-400 hover:bg-zinc-700 hover:text-zinc-100"
      )}
    >
      {children}
    </button>
  );
}

export default function Notes() {
  const navigate = useNavigate();
  const [notes, setNotes] = useState<Note[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [search, setSearch] = useState("");
  const [showColorPicker, setShowColorPicker] = useState(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({ heading: { levels: [1, 2, 3] } }),
      Underline,
      TextStyle,
      Color,
      Highlight.configure({ multicolor: true }),
    ],
    content: "",
    editorProps: {
      attributes: {
        class: "prose prose-invert prose-base max-w-none h-full outline-none px-8 py-6 leading-relaxed",
      },
    },
    onUpdate: ({ editor }) => {
      if (!selectedId) return;
      const html = editor.getHTML();
      if (saveTimer.current) clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        invoke<Note>("update_note", { id: selectedId, title, content: html }).then((updated) => {
          setNotes((prev) =>
            prev.map((n) => (n.id === updated.id ? updated : n))
              .sort((a, b) => b.updated_at - a.updated_at)
          );
        });
      }, 600);
    },
  });

  useEffect(() => {
    invoke<Note[]>("get_notes").then((data) => {
      setNotes(data);
      if (data.length > 0) selectNote(data[0]);
    });
  }, []);

  function selectNote(note: Note) {
    setSelectedId(note.id);
    setTitle(note.title);
    editor?.commands.setContent(note.content || "");
  }

  const saveTitle = useCallback(
    (id: string, t: string) => {
      const html = editor?.getHTML() ?? "";
      invoke<Note>("update_note", { id, title: t, content: html }).then((updated) => {
        setNotes((prev) =>
          prev.map((n) => (n.id === updated.id ? updated : n))
            .sort((a, b) => b.updated_at - a.updated_at)
        );
      });
    },
    [editor]
  );

  function handleTitleChange(val: string) {
    setTitle(val);
    if (!selectedId) return;
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => saveTitle(selectedId, val), 600);
  }

  async function handleCreate() {
    const note = await invoke<Note>("create_note");
    setNotes((prev) => [note, ...prev]);
    selectNote(note);
    setTimeout(() => editor?.commands.focus(), 50);
  }

  async function handleDelete() {
    if (!selectedId) return;
    await invoke("delete_note", { id: selectedId });
    const remaining = notes.filter((n) => n.id !== selectedId);
    setNotes(remaining);
    if (remaining.length > 0) selectNote(remaining[0]);
    else { setSelectedId(null); setTitle(""); editor?.commands.setContent(""); }
  }

  const filtered = notes.filter(
    (n) => n.title.includes(search) || n.content.includes(search)
  );

  return (
    <div className="flex h-full flex-col" onClick={() => setShowColorPicker(false)}>
      {/* Header */}
      <header className="flex items-center justify-between border-b border-zinc-800 px-6 py-4">
        <div className="flex items-center gap-3">
          <button
            onClick={() => navigate("/toolbox")}
            className="flex items-center gap-1.5 text-sm text-zinc-400 hover:text-zinc-100 transition-colors"
          >
            <ArrowLeft size={15} />
            工具箱
          </button>
          <span className="text-zinc-600">/</span>
          <span className="text-sm text-zinc-100">备忘录</span>
        </div>
        <button
          onClick={handleCreate}
          className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-500 transition-colors"
        >
          <Plus size={14} />
          新建
        </button>
      </header>

      <div className="flex flex-1 overflow-hidden">
        {/* Note list */}
        <aside className="flex w-60 flex-col border-r border-zinc-800 bg-zinc-900/40">
          <div className="px-4 pt-4 pb-3">
            <div className="flex items-center gap-2 rounded-lg bg-zinc-800 px-4 py-2.5">
              <Search size={14} className="text-zinc-500 shrink-0" />
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="搜索笔记..."
                className="flex-1 bg-transparent text-sm text-zinc-100 placeholder-zinc-500 outline-none"
              />
            </div>
          </div>
          <div className="flex-1 overflow-y-auto">
            {filtered.length === 0 ? (
              <p className="px-4 pt-8 text-center text-sm text-zinc-600">
                {search ? "无匹配笔记" : "暂无笔记"}
              </p>
            ) : (
              filtered.map((note) => (
                <button
                  key={note.id}
                  onClick={() => selectNote(note)}
                  className={cn(
                    "w-full px-5 py-4 text-left transition-colors",
                    note.id === selectedId
                      ? "bg-zinc-800"
                      : "hover:bg-zinc-800/50"
                  )}
                >
                  <p className="truncate text-sm font-medium text-zinc-100">
                    {note.title || "无标题"}
                  </p>
                  <p className="mt-1 truncate text-xs text-zinc-500">
                    {formatDate(note.updated_at)}
                  </p>
                </button>
              ))
            )}
          </div>
        </aside>

        {/* Editor */}
        {selectedId ? (
          <div className="flex flex-1 flex-col overflow-hidden">
            {/* Title */}
            <div className="border-b border-zinc-800 px-8 py-5">
              <input
                value={title}
                onChange={(e) => handleTitleChange(e.target.value)}
                placeholder="无标题"
                className="w-full bg-transparent text-xl font-semibold text-zinc-100 outline-none placeholder-zinc-600"
              />
            </div>

            {/* Toolbar */}
            <div
              className="flex items-center gap-0.5 border-b border-zinc-800 px-6 py-2.5"
              onClick={(e) => e.stopPropagation()}
            >
              <ToolbarBtn active={editor?.isActive("bold")} onClick={() => editor?.chain().focus().toggleBold().run()} title="加粗 (Ctrl+B)">
                <Bold size={14} />
              </ToolbarBtn>
              <ToolbarBtn active={editor?.isActive("italic")} onClick={() => editor?.chain().focus().toggleItalic().run()} title="斜体 (Ctrl+I)">
                <Italic size={14} />
              </ToolbarBtn>
              <ToolbarBtn active={editor?.isActive("underline")} onClick={() => editor?.chain().focus().toggleUnderline().run()} title="下划线 (Ctrl+U)">
                <UnderlineIcon size={14} />
              </ToolbarBtn>

              <div className="mx-1.5 h-4 w-px bg-zinc-700" />

              <ToolbarBtn active={editor?.isActive("heading", { level: 1 })} onClick={() => editor?.chain().focus().toggleHeading({ level: 1 }).run()} title="大标题">
                <Heading1 size={14} />
              </ToolbarBtn>
              <ToolbarBtn active={editor?.isActive("heading", { level: 2 })} onClick={() => editor?.chain().focus().toggleHeading({ level: 2 }).run()} title="中标题">
                <Heading2 size={14} />
              </ToolbarBtn>
              <ToolbarBtn active={editor?.isActive("heading", { level: 3 })} onClick={() => editor?.chain().focus().toggleHeading({ level: 3 }).run()} title="小标题">
                <Heading3 size={14} />
              </ToolbarBtn>

              <div className="mx-1.5 h-4 w-px bg-zinc-700" />

              <ToolbarBtn active={editor?.isActive("bulletList")} onClick={() => editor?.chain().focus().toggleBulletList().run()} title="无序列表">
                <List size={14} />
              </ToolbarBtn>
              <ToolbarBtn active={editor?.isActive("orderedList")} onClick={() => editor?.chain().focus().toggleOrderedList().run()} title="有序列表">
                <ListOrdered size={14} />
              </ToolbarBtn>

              <div className="mx-1.5 h-4 w-px bg-zinc-700" />

              {/* Color picker */}
              <div className="relative">
                <button
                  onMouseDown={(e) => { e.preventDefault(); setShowColorPicker((v) => !v); }}
                  title="文字颜色"
                  className="flex h-7 items-center gap-1 rounded px-2 text-xs text-zinc-400 hover:bg-zinc-700 hover:text-zinc-100 transition-colors"
                >
                  <span className="font-bold" style={{ color: editor?.getAttributes("textStyle").color || "#e4e4e7" }}>A</span>
                  <span>颜色</span>
                </button>
                {showColorPicker && (
                  <div className="absolute left-0 top-full z-50 mt-1 flex gap-1 rounded-lg border border-zinc-700 bg-zinc-900 p-2 shadow-xl">
                    {PRESET_COLORS.map(({ label, value }) => (
                      <button
                        key={label}
                        onMouseDown={(e) => {
                          e.preventDefault();
                          if (value) editor?.chain().focus().setColor(value).run();
                          else editor?.chain().focus().unsetColor().run();
                          setShowColorPicker(false);
                        }}
                        title={label}
                        className="h-5 w-5 rounded-full border border-zinc-600 transition-transform hover:scale-110"
                        style={{ background: value || "#e4e4e7" }}
                      />
                    ))}
                  </div>
                )}
              </div>

              <div className="flex-1" />

              <button
                onClick={handleDelete}
                title="删除笔记"
                className="rounded p-1.5 text-zinc-500 hover:bg-red-900/40 hover:text-red-400 transition-colors"
              >
                <Trash2 size={14} />
              </button>
            </div>

            {/* TipTap editor */}
            <div className="flex-1 overflow-y-auto">
              <EditorContent editor={editor} className="h-full" />
            </div>
          </div>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-600">
            <FileText size={40} />
            <p className="text-sm">点击「新建」创建第一篇笔记</p>
          </div>
        )}
      </div>
    </div>
  );
}
