"""把逐采样点的 OCR 结果合并成带时间轴的字幕段。

策略：
  - 顺序扫描采样结果，用相似度把连续相同/高度相似的文本归为同一段；
  - 允许中间最多 max_gap 秒的 OCR 失败（空文本）不立即断段；
  - 每段在收尾时投票选出最可信文本（出现次数 > 平均置信度，过滤过短/低置信）。
"""
from typing import List, Optional

from rapidfuzz import fuzz

from models import OcrFrameResult, SubtitleSegment
from text_normalizer import is_meaningful


def _similar(a: str, b: str) -> float:
    if not a or not b:
        return 0.0
    return fuzz.ratio(a, b) / 100.0


class _Bucket:
    """正在累积的一段字幕。"""

    def __init__(self, start: float, sample_interval: float):
        self.start = start
        self.last_seen = start
        self.sample_interval = sample_interval
        # 候选文本 → [出现次数, 置信度累加]
        self.votes = {}

    def add(self, t: float, text: str, conf: float):
        self.last_seen = t
        if text not in self.votes:
            self.votes[text] = [0, 0.0]
        self.votes[text][0] += 1
        self.votes[text][1] += conf

    def best(self, min_confidence: float) -> Optional[SubtitleSegment]:
        if not self.votes:
            return None
        # 投票：先按出现次数，再按平均置信度；过滤明显过短
        def score(item):
            text, (count, conf_sum) = item
            avg = conf_sum / count if count else 0.0
            length_bonus = min(len(text), 20) * 0.001  # 轻微偏好更完整的文本
            return (count, avg + length_bonus)

        ranked = sorted(self.votes.items(), key=score, reverse=True)
        for text, (count, conf_sum) in ranked:
            avg = conf_sum / count if count else 0.0
            if avg < min_confidence:
                continue
            if not is_meaningful(text):
                continue
            # 结束时间补一个采样间隔，避免字幕过早消失
            end = self.last_seen + self.sample_interval
            return SubtitleSegment(self.start, end, text, avg)
        return None


def align(
    frames: List[OcrFrameResult],
    similarity_threshold: float,
    min_duration: float,
    max_gap: float,
    min_confidence: float,
    sample_interval: float,
    log=None,
) -> List[SubtitleSegment]:
    segments: List[SubtitleSegment] = []
    current: Optional[_Bucket] = None
    # 当前段的“代表文本”（用于相似度比较），取最近一次非空文本
    ref_text: str = ""
    gap_start: Optional[float] = None  # 进入空白的起点
    # 诊断计数
    stats = {"buckets": 0, "dropped_lowconf": 0, "dropped_short": 0, "merged": 0, "kept": 0}

    def flush():
        nonlocal current, ref_text, gap_start
        if current is not None:
            stats["buckets"] += 1
            seg = current.best(min_confidence)
            if seg is None:
                stats["dropped_lowconf"] += 1  # 投票后无文本过 min_confidence / 全是噪声
            elif (seg.end - seg.start) < min_duration:
                stats["dropped_short"] += 1
            else:
                # 与上一段若文本相同且时间相接，则合并
                if segments and _similar(segments[-1].text, seg.text) >= similarity_threshold \
                        and seg.start - segments[-1].end <= max_gap:
                    segments[-1].end = seg.end
                    stats["merged"] += 1
                else:
                    segments.append(seg)
                    stats["kept"] += 1
        current = None
        ref_text = ""
        gap_start = None

    for fr in frames:
        text = fr.text
        if not text:
            # OCR 失败/空白：判断是否超过 max_gap
            if current is not None:
                if gap_start is None:
                    gap_start = fr.time
                if fr.time - gap_start > max_gap:
                    flush()
            continue

        gap_start = None
        if current is None:
            current = _Bucket(fr.time, sample_interval)
            current.add(fr.time, text, fr.confidence)
            ref_text = text
        else:
            if _similar(ref_text, text) >= similarity_threshold:
                current.add(fr.time, text, fr.confidence)
                ref_text = text  # 跟随最新识别，缓解渐变
            else:
                # 文本变了 → 收上一段，开新段
                flush()
                current = _Bucket(fr.time, sample_interval)
                current.add(fr.time, text, fr.confidence)
                ref_text = text

    flush()
    if log:
        log(
            f"[诊断] 合并出 {stats['buckets']} 段 → 保留 {stats['kept']}，"
            f"合并入相邻 {stats['merged']}，丢弃(低置信/噪声) {stats['dropped_lowconf']}，"
            f"丢弃(太短) {stats['dropped_short']}；min_conf={min_confidence} min_dur={min_duration}"
        )
    return segments
