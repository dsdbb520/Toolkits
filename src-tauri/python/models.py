"""数据结构定义。"""
from dataclasses import dataclass


@dataclass
class OcrFrameResult:
    """单个采样时间点的 OCR 结果。"""
    time: float          # 时间戳（秒）
    text: str            # 清洗后的文本（可能为空）
    confidence: float    # 平均置信度 0~1


@dataclass
class SubtitleSegment:
    """合并后的一条字幕。"""
    start: float
    end: float
    text: str
    confidence: float
