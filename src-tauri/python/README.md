# 硬字幕 OCR sidecar

由 Rust 后端（`src-tauri/src/subtitle_ocr.rs`）以
`python -u main.py --config <cfg.json>` 调用。

## 安装依赖

```bash
cd src-tauri/python
pip install -r requirements.txt
```

> 首次运行 PaddleOCR 会自动下载识别模型（数百 MB，存到用户目录的 `.paddleocr/`）。
> 需要联网。之后离线可用。

## 输入（cfg.json）

```json
{
  "video_path": "D:/xxx.mp4",
  "output_path": "D:/xxx.srt",
  "subtitle_region": { "x": 0.1, "y": 0.8, "w": 0.8, "h": 0.15 },
  "sample_fps": 5,
  "language": "ch",
  "similarity_threshold": 0.90,
  "min_duration": 0.4,
  "max_gap": 0.4,
  "min_confidence": 0.6
}
```

- `subtitle_region`：相对坐标，均为 0~1。
- `language`：`ch`=简体/通用，`chinese_cht`=繁体。

## 输出（stdout，每行一条 JSON）

```
{"type":"progress","current":120,"total":1000,"message":"OCR 识别中"}
{"type":"done","output_path":"D:/xxx.srt","message":"完成，共 42 条字幕"}
{"type":"error","message":"..."}
```

普通日志写到 stderr。

## 模块

| 文件 | 职责 |
|------|------|
| `main.py` | 解析参数、调度整体流程、输出进度 |
| `video_reader.py` | 读取视频信息、按 sample_fps 抽帧、裁剪 ROI |
| `image_preprocess.py` | 字幕区域预处理（放大/降噪/对比度） |
| `ocr_engine.py` | 封装 PaddleOCR（兼容 2.x/3.x），可替换 |
| `text_normalizer.py` | 清洗 OCR 文本 |
| `subtitle_aligner.py` | 相似度合并 + 投票选最可信文本 + 对轴 |
| `srt_writer.py` | 写出 SRT |
| `models.py` | 数据结构 |
