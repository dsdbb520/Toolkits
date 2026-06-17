"""写出 SRT 文件。"""
from typing import List

from models import SubtitleSegment


def _fmt_time(seconds: float) -> str:
    if seconds < 0:
        seconds = 0
    ms = int(round(seconds * 1000))
    h, ms = divmod(ms, 3600_000)
    m, ms = divmod(ms, 60_000)
    s, ms = divmod(ms, 1000)
    return f"{h:02d}:{m:02d}:{s:02d},{ms:03d}"


def write_srt(segments: List[SubtitleSegment], output_path: str) -> int:
    """写 SRT，返回写入的字幕条数。"""
    lines = []
    for i, seg in enumerate(segments, start=1):
        lines.append(str(i))
        lines.append(f"{_fmt_time(seg.start)} --> {_fmt_time(seg.end)}")
        lines.append(seg.text)
        lines.append("")  # 条目间空行
    with open(output_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    return len(segments)
