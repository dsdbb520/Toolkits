"""硬字幕 OCR 对轴 sidecar 入口。

被 Rust 后端以  python -u main.py --config <cfg.json>  调用。
通过 stdout 输出 JSON 进度消息（每行一条）：
  {"type":"progress","current":N,"total":M,"message":"..."}
  {"type":"done","output_path":"xxx.srt","message":"..."}
  {"type":"error","message":"..."}
普通日志走 stderr（Rust 会转成 log 事件）。
"""
import argparse
import json
import sys
import traceback

# 强制 stdout/stderr 用 UTF-8，避免 Windows 中文系统默认 GBK 导致 Rust 端读取乱码
try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass


def emit(obj: dict):
    """向 stdout 输出一条 JSON 消息并立即刷新。"""
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def log(msg: str):
    sys.stderr.write(str(msg) + "\n")
    sys.stderr.flush()


def run(config: dict):
    # 延迟导入，import 失败也能作为 error 反馈
    from models import OcrFrameResult
    from video_reader import VideoReader
    from image_preprocess import preprocess, roi_signature, signature_diff
    from ocr_engine import create_engine
    from text_normalizer import normalize
    from subtitle_aligner import align
    from srt_writer import write_srt

    video_path = config["video_path"]
    output_path = config["output_path"]
    region = config["subtitle_region"]
    # sample_fps 现在是「扫描帧率」：每秒读多少帧做廉价的变化检测（抓住短字幕），
    # 真正的 OCR 只在字幕区域发生变化时才跑。所以扫描率可以开高而不至于太慢。
    sample_fps = float(config.get("sample_fps", 4.0))
    language = config.get("language", "ch")
    engine_name = config.get("engine", "rapidocr")
    similarity_threshold = float(config.get("similarity_threshold", 0.90))
    min_duration = float(config.get("min_duration", 0.4))
    max_gap = float(config.get("max_gap", 0.4))
    min_confidence = float(config.get("min_confidence", 0.6))

    log("sidecar v2: 扫描/OCR 解耦 + 诊断日志 已启用")
    log(f"打开视频: {video_path}（引擎: {engine_name}, 扫描 {sample_fps} fps）")
    reader = VideoReader(video_path)
    total = reader.sample_count(sample_fps)
    sample_interval = 1.0 / max(0.1, sample_fps)
    emit({"type": "progress", "current": 0, "total": total, "message": "初始化 OCR 引擎"})

    engine = create_engine(engine_name, lang=language)

    frames = []
    processed = 0
    ocr_calls = 0
    emit_every = max(1, total // 200) if total else 10  # 控制消息频率

    # 帧变化检测：ROI 与上一扫描点几乎相同 → 字幕没变，直接复用结果、跳过 OCR。
    # 字幕通常停留 1~3 秒，这能省掉绝大部分重复识别。CHANGE_THRESH 越小越敏感。
    CHANGE_THRESH = 6.0
    prev_sig = None
    prev_text, prev_conf = "", 0.0

    for ts, frame in reader.iter_samples(sample_fps):
        roi = VideoReader.crop_region(frame, region)
        sig = roi_signature(roi)

        if prev_sig is not None and signature_diff(sig, prev_sig) < CHANGE_THRESH:
            # 与上一帧几乎一致：复用，不跑 OCR
            text, conf = prev_text, prev_conf
        else:
            proc = preprocess(roi)
            text, conf = engine.recognize(proc)
            text = normalize(text)
            ocr_calls += 1
            prev_text, prev_conf = text, conf

        prev_sig = sig
        frames.append(OcrFrameResult(time=ts, text=text, confidence=conf))

        processed += 1
        if processed % emit_every == 0 or processed == total:
            emit({
                "type": "progress",
                "current": processed,
                "total": total,
                "message": f"OCR 识别中（实际识别 {ocr_calls} 次）",
            })

    reader.release()
    log(f"采样 {processed} 帧，实际 OCR {ocr_calls} 次（跳过 {processed - ocr_calls} 帧重复字幕）")

    # 诊断：看字幕在哪一步丢的
    non_empty = sum(1 for f in frames if f.text)
    changes = 0
    prev = None
    for f in frames:
        if f.text and f.text != prev:
            changes += 1
        prev = f.text
    log(f"[诊断] 非空文本帧 {non_empty}/{processed}，文本变化 ~{changes} 次")

    emit({"type": "progress", "current": total, "total": total, "message": "合并字幕与对轴"})

    segments = align(
        frames,
        similarity_threshold=similarity_threshold,
        min_duration=min_duration,
        max_gap=max_gap,
        min_confidence=min_confidence,
        sample_interval=sample_interval,
        log=log,
    )

    count = write_srt(segments, output_path)
    log(f"共生成 {count} 条字幕")
    emit({
        "type": "done",
        "output_path": output_path,
        "message": f"完成，共 {count} 条字幕",
    })


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True, help="JSON 配置文件路径")
    args = parser.parse_args()

    try:
        with open(args.config, "r", encoding="utf-8") as f:
            config = json.load(f)
    except Exception as e:
        emit({"type": "error", "message": f"读取配置失败: {e}"})
        sys.exit(1)

    try:
        run(config)
    except ModuleNotFoundError as e:
        emit({
            "type": "error",
            "message": f"缺少 Python 依赖: {e}. 请在 sidecar 目录执行 pip install -r requirements.txt",
        })
        sys.exit(1)
    except Exception as e:
        log(traceback.format_exc())
        emit({"type": "error", "message": f"OCR 任务失败: {e}"})
        sys.exit(1)


if __name__ == "__main__":
    main()
