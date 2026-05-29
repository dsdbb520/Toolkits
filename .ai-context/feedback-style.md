---
name: feedback-style
description: 用户协作偏好与反馈习惯
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 6ca0cd19-c7b7-475b-b7ad-a0f28e9fbfc1
---

# 协作偏好

## commit 风格
不在 commit 里加 `Co-Authored-By: Claude ...` 署名行。
**Why:** 用户明确要求，不想 AI 出现在提交历史里。
**How to apply:** 每次 git commit 直接写内容，不附加 Co-Authored-By。

## 响应风格
- 简洁直接，不需要过多铺垫
- 报错时直接给修复方案，不需要先解释原因再给方案
- 中文交流

## 开发节奏
- 每完成一个功能阶段后 commit + push 到 GitHub
- 遇到问题描述症状，不一定能描述根因，需要主动诊断
- 功能优先验证可用性，细节 bug 按需修复

## 已验证的好做法
- 启动 Tauri dev 时用后台任务，不阻塞对话
- 每次启动前先 kill 占用 1420 端口的进程
- 用 `pnpm exec vite build` 快速验证前端是否有编译错误，不用每次都等 Tauri 全量编译
