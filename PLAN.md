# Toolkits 开发计划

Tauri（React + TypeScript 前端 + Rust 后端）工具箱，集成多个常用工具，同时支持 Windows 和 Mac（优先 Windows）。

## 技术栈

| 层级 | 技术 |
|------|------|
| 框架 | Tauri 2.x |
| 前端 | React 19 + TypeScript + Vite 7 |
| UI 组件 | Tailwind CSS v4 + lucide-react + TipTap |
| 后端（客户端） | Rust + rusqlite (SQLite) |
| 后端（同步服务端） | Rust Axum，部署于 Linux NAS |
| 包管理 | pnpm |

## 工具列表

| # | 工具 | 平台 | 核心方案 |
|---|------|------|---------|
| 1 | 备忘录（跨设备同步） | Win + Mac | 本地 SQLite + 自建 Axum 同步服务端 |
| 2 | B站视频下载 | Win + Mac | 内嵌 `yt-dlp` 二进制 |
| 3 | Steam 账号快速切换 | Win + Mac | 读写 `loginusers.vdf` |
| 4 | 截图工具 | Win + Mac | `screenshots` crate / `screencapture` |
| 5 | 简单图片编辑 | Win + Mac | 前端 Canvas + Rust `image` crate |

---

## 阶段计划

### 阶段 0：项目脚手架 ✅
- [x] `create-tauri-app` 初始化项目（React + TypeScript + Vite）
- [x] 配置 Tailwind CSS v4 + lucide-react + clsx + tailwind-merge
- [x] 搭主窗口布局：左侧图标栏 → 工具箱卡片网格 → 具体工具页
- [x] 配置前端路由（React Router）
- [x] 验证 `pnpm tauri dev` 能正常启动，窗口 1280×800

---

### 阶段 1：备忘录 ✅（本地功能完成，同步已完成）

**本地存储**
- [x] Rust rusqlite：笔记 CRUD，软删除，settings 表
- [x] TipTap 富文本编辑器：加粗/斜体/下划线/标题/列表/文字颜色
- [x] 笔记/备忘录双模式（is_note）：笔记默认只读，需点编辑按钮解锁
- [x] 修复工具栏状态同步（useRef + forceRender）
- [x] 修复切换笔记/导航离开时的数据丢失问题（flush on switch/unmount）

**同步服务端（`sync-server/`）**
- [x] Rust Axum 独立二进制，部署于飞牛 NAS（Linux Debian）
- [x] 环境变量配置：`SYNC_USERNAME` / `SYNC_PASSWORD` / `JWT_SECRET` / `PORT` / `DB_PATH`
- [x] `POST /login` → JWT Token（有效期 10 年）
- [x] `POST /sync` → 双向增量同步，last-write-wins，含软删除传播
- [x] frp TCP 代理暴露公网端口
- [x] reqwest 客户端接受自签名证书（`danger_accept_invalid_certs`）

**待规划：同步服务端多用户支持**
- [ ] 服务端 notes 表加 `user_id` 字段，按用户隔离数据
- [ ] JWT 携带 `user_id`，注册/管理接口
- [ ] 一个端口支持多个独立账户

---

### 阶段 2：B站视频下载

- [ ] 将 `yt-dlp` 可执行文件打包进 Tauri `resources/`（Windows + Mac 各一份）
- [ ] Rust：启动子进程、解析进度输出、通过 Tauri 事件推送到前端
- [ ] 前端：URL 输入框、清晰度/音频选项、实时进度条、下载历史

---

### 阶段 3：Steam 账号快速切换

- [ ] Rust 解析 `loginusers.vdf`
  - Windows 路径：`C:\Program Files (x86)\Steam\config\loginusers.vdf`
  - Mac 路径：`~/Library/Application Support/Steam/config/loginusers.vdf`
- [ ] 切换逻辑：修改 `MostRecent` 字段，可选自动重启 Steam
- [ ] 前端：账号卡片列表（含头像缓存）、一键切换、添加备注名

---

### 阶段 4：截图工具

- [ ] Windows：`screenshots` crate 实现全屏 / 区域截图
- [ ] Mac：调用系统 `screencapture` 命令
- [ ] 前端：截图预览、基础标注（矩形框、箭头、文字）
- [ ] 快速保存到剪贴板或指定目录
- [ ] 全局快捷键呼出截图

---

### 阶段 5：图片编辑

- [ ] 前端 Canvas：裁剪、旋转、翻转、缩放
- [ ] 调整面板：亮度、对比度、饱和度滑块
- [ ] Rust `image` crate：格式转换（PNG/JPG/WebP）、批量压缩
- [ ] 拖拽 / 粘贴打开图片

---

### 收尾

- [ ] 统一错误提示与 Toast 通知风格
- [ ] 全局快捷键管理（截图、唤醒主窗口）
- [ ] 自动更新（Tauri updater）
- [ ] 打包测试：Windows `.msi` / `.exe`，Mac `.dmg`
- [ ] 图标设计与替换

---

### 待规划：快捷访问（桌面/抽屉模式）

> 类似手机抽屉与桌面的关系：工具箱是抽屉（所有工具），一级菜单是桌面（常用工具）

- [ ] 用户可将任意工具「固定」到左侧图标栏一级入口，直接点击跳转
- [ ] 一级入口支持拖拽排序
- [ ] 固定状态持久化到本地配置

---

## 进度记录

| 日期 | 完成内容 |
|------|---------|
| 2026-05-30 | 环境搭建，Tauri 项目初始化，pnpm 依赖安装 |
| 2026-05-30 | 阶段 0 完成：Tailwind + 路由 + 导航布局 |
| 2026-05-30 | 阶段 1 完成：备忘录本地 CRUD + TipTap 编辑器 + 同步服务端 + 多项 bug 修复 |
