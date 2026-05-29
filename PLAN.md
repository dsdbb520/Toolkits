# Toolkits 开发计划

Tauri（React + TypeScript 前端 + Rust 后端）工具箱，集成多个常用工具，同时支持 Windows 和 Mac（优先 Windows）。

## 技术栈

| 层级 | 技术 |
|------|------|
| 框架 | Tauri 2.x |
| 前端 | React 18 + TypeScript + Vite |
| UI 组件 | shadcn/ui + Tailwind CSS |
| 后端 | Rust |
| 本地存储 | SQLite（via `rusqlite`） |
| 包管理 | pnpm |

## 工具列表

| # | 工具 | 平台 | 核心方案 |
|---|------|------|---------|
| 1 | 备忘录（跨设备同步） | Win + Mac | 本地 SQLite + 自建服务端同步 |
| 2 | B站视频下载 | Win + Mac | 内嵌 `yt-dlp` 二进制 |
| 3 | Steam 账号快速切换 | Win + Mac | 读写 `loginusers.vdf` |
| 4 | 截图工具 | Win + Mac | `screenshots` crate / `screencapture` |
| 5 | 简单图片编辑 | Win + Mac | 前端 Canvas + Rust `image` crate |

---

## 阶段计划

### 阶段 0：项目脚手架 ✅
- [x] `create-tauri-app` 初始化项目（React + TypeScript + Vite）
- [x] pnpm 依赖安装完成
- [x] 配置 Tailwind CSS v4 + clsx + tailwind-merge + lucide-react
- [x] 搭主窗口布局：左侧图标栏（一级）+ 二级面板 + 内容区
- [x] 配置前端路由（React Router）
- [x] 验证 `pnpm tauri dev` 能正常启动

---

### 阶段 1：备忘录（优先，跑通整体架构）

**后端（服务器）**
- [ ] 在自有服务器部署轻量 REST API（Rust Axum 或 Node.js）
- [ ] 接口：增删改查笔记、账号认证（JWT）

**客户端**
- [ ] 本地 SQLite 存储笔记
- [ ] 与服务器增量同步（冲突策略：最后写入获胜）
- [ ] 前端：Markdown 编辑器（`@uiw/react-md-editor`）、笔记列表、搜索

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

- [ ] 用户可将任意工具「固定」到左侧图标栏一级入口，直接点击跳转（无需展开工具箱）
- [ ] 一级入口支持拖拽排序
- [ ] 固定状态持久化到本地配置

---

## 进度记录

| 日期 | 完成内容 |
|------|---------|
| 2026-05-30 | 环境搭建完成，Tauri 项目初始化，pnpm 依赖安装完成 |
| 2026-05-30 | 阶段 0 完成：Tailwind + 路由 + 两级导航布局（图标栏 + 工具箱面板） |
