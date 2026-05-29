import { useEffect, useRef, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { useEditor, EditorContent } from "@tiptap/react";
import { StarterKit } from "@tiptap/starter-kit";
import { Underline } from "@tiptap/extension-underline";
import { TextStyle } from "@tiptap/extension-text-style";
import { Color } from "@tiptap/extension-color";
import { Highlight } from "@tiptap/extension-highlight";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  ArrowLeft, Plus, Trash2, Search, FileText,
  Bold, Italic, Underline as UnderlineIcon,
  Heading1, Heading2, Heading3, List, ListOrdered,
  BookOpen, FileEdit, Pencil, RefreshCw, Settings2, X,
} from "lucide-react";
import { cn } from "@/lib/utils";

interface Note {
  id: string;
  title: string;
  content: string;
  is_note: boolean;
  deleted: boolean;
  created_at: number;
  updated_at: number;
}

interface SyncSettings {
  server_url: string;
  token: string;
  last_sync_at: number;
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

function ToolbarBtn({ active, onClick, title, children }: {
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
  const [isEditing, setIsEditing] = useState(false);
  const [showSyncPanel, setShowSyncPanel] = useState(false);
  const [syncSettings, setSyncSettings] = useState<SyncSettings>({ server_url: "", token: "", last_sync_at: 0 });
  const [syncStatus, setSyncStatus] = useState("");
  const [syncing, setSyncing] = useState(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 用 ref 追踪最新值，避免 useEditor 回调中的闭包过期问题
  const selectedIdRef = useRef<string | null>(null);
  const titleRef = useRef("");
  const isEditingRef = useRef(false);
  const contentRef = useRef("");

  const selected = notes.find((n) => n.id === selectedId) ?? null;

  // 每次编辑器事务/选区变化时强制重渲染，确保工具栏状态实时同步
  const [, forceRender] = useState(0);

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
    onTransaction: () => forceRender((n) => n + 1),
    onSelectionUpdate: () => forceRender((n) => n + 1),
    onUpdate: ({ editor }) => {
      if (!selectedIdRef.current || !isEditingRef.current) return;
      const html = editor.getHTML();
      contentRef.current = html; // 始终追踪最新内容
      const id = selectedIdRef.current;
      const t = titleRef.current;
      if (saveTimer.current) clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        invoke<Note>("update_note", { id, title: t, content: html }).then(updateNoteInList);
      }, 600);
    },
  });

  useEffect(() => {
    invoke<Note[]>("get_notes").then((data) => {
      setNotes(data);
      if (data.length > 0) selectNote(data[0]);
    });
    invoke<SyncSettings>("get_sync_settings").then(setSyncSettings);

    // 组件卸载时立即执行未完成的保存，防止导航离开时丢失数据
    return () => {
      if (saveTimer.current) {
        clearTimeout(saveTimer.current);
        saveTimer.current = null;
      }
      if (selectedIdRef.current && isEditingRef.current) {
        invoke("update_note", {
          id: selectedIdRef.current,
          title: titleRef.current,
          content: contentRef.current,
        }).catch(() => {});
      }
    };
  }, []);

  // 切换笔记时，is_note 的默认为只读
  useEffect(() => {
    if (selected) setIsEditing(!selected.is_note);
  }, [selectedId]);

  // 同步编辑器可编辑状态
  useEffect(() => {
    if (!editor) return;
    isEditingRef.current = isEditing;
    editor.setEditable(isEditing);
    if (isEditing) setTimeout(() => editor.commands.focus(), 50);
  }, [isEditing, editor]);

  function updateNoteInList(updated: Note) {
    setNotes((prev) =>
      prev.map((n) => (n.id === updated.id ? updated : n))
        .sort((a, b) => b.updated_at - a.updated_at)
    );
  }

  function selectNote(note: Note) {
    // 切换前先把当前笔记的未保存内容立即写入
    if (saveTimer.current) {
      clearTimeout(saveTimer.current);
      saveTimer.current = null;
      if (selectedIdRef.current && isEditingRef.current) {
        invoke("update_note", {
          id: selectedIdRef.current,
          title: titleRef.current,
          content: contentRef.current,
        }).catch(() => {});
      }
    }
    selectedIdRef.current = note.id;
    titleRef.current = note.title;
    contentRef.current = note.content || "";
    isEditingRef.current = !note.is_note;
    setSelectedId(note.id);
    setTitle(note.title);
    setIsEditing(!note.is_note);
    // false = 不触发 onUpdate，避免加载内容时误保存
    editor?.commands.setContent(note.content || "", false);
  }

  const saveTitle = useCallback(
    (id: string, t: string) => {
      invoke<Note>("update_note", { id, title: t, content: contentRef.current }).then(updateNoteInList);
    },
    []
  );

  function handleTitleChange(val: string) {
    setTitle(val);
    titleRef.current = val;
    if (!selectedId || !isEditing) return;
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => saveTitle(selectedId, val), 600);
  }

  async function handleCreate() {
    const note = await invoke<Note>("create_note");
    setNotes((prev) => [note, ...prev]);
    selectNote(note);
  }

  async function handleDelete() {
    if (!selectedId) return;
    await invoke("delete_note", { id: selectedId });
    const remaining = notes.filter((n) => n.id !== selectedId);
    setNotes(remaining);
    if (remaining.length > 0) selectNote(remaining[0]);
    else { setSelectedId(null); setTitle(""); editor?.commands.setContent(""); }
  }

  async function handleToggleType() {
    if (!selectedId) return;
    const updated = await invoke<Note>("toggle_note_type", { id: selectedId });
    updateNoteInList(updated);
    setIsEditing(!updated.is_note);
  }

  async function handleSync() {
    setSyncing(true);
    setSyncStatus("同步中...");
    try {
      const count = await invoke<number>("sync_notes");
      setSyncStatus(`同步完成，更新了 ${count} 条笔记`);
      const data = await invoke<Note[]>("get_notes");
      setNotes(data);
      const settings = await invoke<SyncSettings>("get_sync_settings");
      setSyncSettings(settings);
    } catch (e) {
      setSyncStatus(`同步失败：${e}`);
    } finally {
      setSyncing(false);
    }
  }

  async function handleSaveSyncSettings() {
    await invoke("save_sync_settings", {
      serverUrl: syncSettings.server_url,
      token: syncSettings.token,
    });
    setSyncStatus("设置已保存");
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
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowSyncPanel((v) => !v)}
            title="同步设置"
            className={cn(
              "rounded-lg p-2 transition-colors",
              showSyncPanel ? "bg-zinc-700 text-white" : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
            )}
          >
            <Settings2 size={16} />
          </button>
          <button
            onClick={handleCreate}
            className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-500 transition-colors"
          >
            <Plus size={14} />
            新建
          </button>
        </div>
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
                    note.id === selectedId ? "bg-zinc-800" : "hover:bg-zinc-800/50"
                  )}
                >
                  <div className="flex items-center gap-2">
                    {note.is_note
                      ? <BookOpen size={12} className="shrink-0 text-blue-400" />
                      : <FileEdit size={12} className="shrink-0 text-zinc-500" />
                    }
                    <p className="truncate text-sm font-medium text-zinc-100">
                      {note.title || "无标题"}
                    </p>
                  </div>
                  <p className="mt-1 truncate text-xs text-zinc-500 pl-4">
                    {formatDate(note.updated_at)}
                  </p>
                </button>
              ))
            )}
          </div>
        </aside>

        {/* Editor / Reader */}
        {selectedId ? (
          <div className="flex flex-1 flex-col overflow-hidden">
            {/* Title */}
            <div className="border-b border-zinc-800 px-8 py-5">
              <input
                value={title}
                readOnly={!isEditing}
                onChange={(e) => handleTitleChange(e.target.value)}
                placeholder="无标题"
                className={cn(
                  "w-full bg-transparent text-xl font-semibold text-zinc-100 outline-none placeholder-zinc-600",
                  !isEditing && "cursor-default"
                )}
              />
            </div>

            {/* Toolbar（编辑模式）/ 操作栏（只读模式）*/}
            <div
              className="flex items-center gap-0.5 border-b border-zinc-800 px-6 py-2.5"
              onClick={(e) => e.stopPropagation()}
            >
              {isEditing ? (
                <>
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
                  {/* 颜色 */}
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
                </>
              ) : (
                /* 只读模式操作栏 */
                <div className="flex items-center gap-2">
                  <span className="flex items-center gap-1.5 text-xs text-blue-400">
                    <BookOpen size={13} />
                    笔记（只读）
                  </span>
                </div>
              )}

              <div className="flex-1" />

              {/* 右侧按钮 */}
              <div className="flex items-center gap-1">
                {selected?.is_note && !isEditing && (
                  <button
                    onClick={() => setIsEditing(true)}
                    className="flex items-center gap-1.5 rounded-lg bg-zinc-700 px-3 py-1.5 text-xs font-medium text-zinc-100 hover:bg-zinc-600 transition-colors"
                  >
                    <Pencil size={12} />
                    编辑
                  </button>
                )}
                {isEditing && selected?.is_note && (
                  <button
                    onClick={() => setIsEditing(false)}
                    className="flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-xs text-zinc-400 hover:text-zinc-100 transition-colors"
                  >
                    <X size={12} />
                    完成
                  </button>
                )}
                <button
                  onClick={handleToggleType}
                  title={selected?.is_note ? "切换为备忘录" : "切换为笔记"}
                  className="flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-xs text-zinc-500 hover:bg-zinc-700 hover:text-zinc-100 transition-colors"
                >
                  {selected?.is_note ? <FileEdit size={13} /> : <BookOpen size={13} />}
                  {selected?.is_note ? "转为备忘录" : "转为笔记"}
                </button>
                <div className="mx-0.5 h-4 w-px bg-zinc-700" />
                <button
                  onClick={handleDelete}
                  title="删除"
                  className="rounded p-1.5 text-zinc-500 hover:bg-red-900/40 hover:text-red-400 transition-colors"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>

            {/* 内容区：只读用 prose 渲染，编辑用 TipTap */}
            <div className="flex-1 overflow-y-auto">
              {isEditing ? (
                <EditorContent editor={editor} className="h-full" />
              ) : (
                <div className="prose prose-invert prose-base max-w-none px-8 py-6 leading-relaxed">
                  <div dangerouslySetInnerHTML={{ __html: selected?.content || "<p class='text-zinc-600'>空白笔记</p>" }} />
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-zinc-600">
            <FileText size={40} />
            <p className="text-sm">点击「新建」创建第一篇笔记</p>
          </div>
        )}

        {/* 同步设置面板 */}
        {showSyncPanel && (
          <div className="flex w-72 flex-col border-l border-zinc-800 bg-zinc-900/60 p-5 gap-4">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium text-zinc-100">同步设置</span>
              <button onClick={() => setShowSyncPanel(false)} className="text-zinc-500 hover:text-zinc-300">
                <X size={14} />
              </button>
            </div>

            <div className="flex flex-col gap-2">
              <label className="text-xs text-zinc-400">服务器地址</label>
              <input
                value={syncSettings.server_url}
                onChange={(e) => setSyncSettings((s) => ({ ...s, server_url: e.target.value }))}
                placeholder="http://your-server:3000"
                className="rounded-lg bg-zinc-800 px-3 py-2 text-sm text-zinc-100 outline-none placeholder-zinc-600 focus:ring-1 focus:ring-zinc-600"
              />
            </div>

            <div className="flex flex-col gap-2">
              <label className="text-xs text-zinc-400">访问令牌（Token）</label>
              <input
                value={syncSettings.token}
                onChange={(e) => setSyncSettings((s) => ({ ...s, token: e.target.value }))}
                placeholder="从服务端 /login 获取"
                type="password"
                className="rounded-lg bg-zinc-800 px-3 py-2 text-sm text-zinc-100 outline-none placeholder-zinc-600 focus:ring-1 focus:ring-zinc-600"
              />
            </div>

            <button
              onClick={handleSaveSyncSettings}
              className="rounded-lg bg-zinc-700 px-3 py-2 text-sm text-zinc-100 hover:bg-zinc-600 transition-colors"
            >
              保存设置
            </button>

            <button
              onClick={handleSync}
              disabled={syncing}
              className="flex items-center justify-center gap-2 rounded-lg bg-blue-600 px-3 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50 transition-colors"
            >
              <RefreshCw size={14} className={syncing ? "animate-spin" : ""} />
              {syncing ? "同步中..." : "立即同步"}
            </button>

            {syncSettings.last_sync_at > 0 && (
              <p className="text-xs text-zinc-500">
                上次同步：{formatDate(syncSettings.last_sync_at)}
              </p>
            )}
            {syncStatus && (
              <p className="text-xs text-zinc-400 rounded bg-zinc-800 px-3 py-2">{syncStatus}</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
