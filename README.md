# SeraphToolkits

一个基于 Tauri 2 + React 的桌面工具箱，把日常用得上的小工具整合到一起，避免到处找软件。

## 功能

| 工具 | 平台 | 说明 |
|------|------|------|
| 备忘录 | Win / Mac | 富文本笔记（标题 / 颜色 / 高亮 / 列表 / 行距调整 / 两行互跳锚点），跨设备云同步、多用户、冲突解决 |
| B站下载 | Win / Mac | 下载 B 站视频，支持多清晰度、历史记录 |
| 媒体编辑 | Win / Mac | 视频裁剪、提取音频、格式转换，依赖本地 ffmpeg |
| 图片编辑 | Win / Mac | 裁剪、旋转、压缩、格式转换，纯本地处理 |
| Steam 区域价格 | Win / Mac | 搜索游戏，对比各区售价与锁区情况，可一键加入心愿单 |
| 心愿单 · 降价追踪 | Win / Mac | 粘贴 Steam/PS/NS 商店链接或手动添加（含实体卡带）追踪价格，记录历史曲线 / 史低 / 目标价；每日同步取价、降价红点提醒，跨设备共享 |
| Steam 账号切换 | **仅 Windows** | 快速切换 Steam 登录账号，依赖 Windows 注册表 |
| 截图标注 | — | 开发中 |

## 技术栈

- **框架**：Tauri 2
- **前端**：React 19 + TypeScript + Vite + Tailwind CSS v4
- **后端**：Rust，SQLite 本地存储
- **同步服务端**：Rust Axum，可自部署，JWT 鉴权

## 构建

### 依赖

- [Rust](https://rustup.rs/)
- Node.js + [pnpm](https://pnpm.io/)
- ffmpeg（媒体编辑 / B站下载功能需要，`brew install ffmpeg` 或 `scoop install ffmpeg`）

### 本地运行

```bash
pnpm install
pnpm tauri dev
```

### 打包

```bash
pnpm tauri build
```

Windows 输出 `.msi` / `.exe`，macOS 输出 `.dmg` / `.app`，需在对应系统上分别构建。

## 同步服务端

备忘录与心愿单的跨设备同步共用同一个 `sync-server/`（同一账户），需自行部署，支持多用户注册登录。

```bash
cd sync-server
cargo build --release
./target/release/sync-server
```

默认端口 `14689`，支持通过环境变量配置。客户端在备忘录设置页填入服务地址后注册/登录即可使用；心愿单会复用该登录态，每天首次打开自动同步并刷新价格。

> 价格由客户端获取（可走代理），服务端只负责存储、合并与价格历史，因此无需服务器能访问 Steam/PS/任天堂。

## License

MIT
