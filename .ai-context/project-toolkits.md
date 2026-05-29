---
name: project-toolkits
description: Tauri 工具箱项目整体情况、技术栈、当前进度、关键架构决策
metadata: 
  node_type: memory
  type: project
  originSessionId: 6ca0cd19-c7b7-475b-b7ad-a0f28e9fbfc1
---

# Toolkits 项目

GitHub: https://github.com/dsdbb520/Toolkits.git
本地路径: F:\Rust\Toolkits

## 技术栈
- 框架: Tauri 2.x
- 前端: React 19 + TypeScript + Vite 7 + Tailwind CSS v4
- UI: lucide-react + TipTap 3.x（富文本编辑器）
- 路由: React Router 7
- 前端工具: clsx + tailwind-merge
- 客户端后端: Rust + rusqlite (SQLite bundled)
- 同步服务端: Rust Axum（独立二进制，`sync-server/` 目录）
- 包管理: pnpm 11

## 导航结构
左侧 48px 图标栏（一级） → 点击「工具箱」→ 内容区显示工具卡片网格 → 点卡片进入具体工具（顶部有返回面包屑）

## 已完成功能

### 阶段 0：脚手架 ✅
- 窗口 1280×800，最小 960×600
- Tailwind v4，全局字体 15px，行高 1.7

### 阶段 1：备忘录 ✅
客户端：
- SQLite 本地存储（AppData/notes.db）
- TipTap 富文本：加粗/斜体/下划线/H1-H3/列表/文字颜色（8色预设）
- is_note 双模式：笔记=默认只读+需点编辑按钮；备忘录=直接可编辑
- 笔记列表搜索、自动保存（debounce 600ms）

同步服务端（sync-server/）：
- 部署于用户飞牛NAS（Linux Debian），路径 /vol1/1000/sync-server
- 端口 14689，frp TCP 代理暴露公网
- JWT 认证（10年有效期），环境变量配置
- 双向增量同步，last-write-wins，含软删除跨设备传播
- 客户端 reqwest 允许自签名证书

## 已知待开发
- 备忘录同步多用户支持（notes 加 user_id，单端口多账户）
- 快捷访问（工具固定到一级图标栏）
- 阶段 2-5：B站下载、Steam切换、截图、图片编辑

## 重要 Bug 修复记录（避免重蹈）
1. TipTap 3.x 全是具名导出（`import { StarterKit } from ...`，不是 default）
2. Tailwind v4 不能用无 @layer 的 `* { padding: 0 }`——会覆盖所有 utility class
3. useEditor onUpdate 闭包捕获过期 state → 改用 useRef 追踪 selectedId/title/content/isEditing
4. setContent 触发 onUpdate 误保存 → 传第二参数 false
5. 切换笔记/卸载时数据丢失 → selectNote 前 flush 未保存内容，useEffect cleanup 也 flush
6. 工具栏状态不实时 → onTransaction + onSelectionUpdate 触发 forceRender
7. pnpm 11 的 esbuild build 权限 → pnpm-workspace.yaml 加 `allowBuilds: { esbuild: true }`

**Why:** 这些 bug 较隐蔽，下次开发时避免重复引入。
**How to apply:** 每次涉及 TipTap/编辑器状态/Tailwind CSS 时先对照这份清单。
