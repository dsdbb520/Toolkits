"""OCR 引擎封装，支持 PaddleOCR 与 RapidOCR，可切换。

把 OCR 隔离在这一层：换引擎只改这里，main 流程不动。
- PaddleOCR：识别强，但 paddlepaddle 体积大、x86 上 oneDNN 偶有崩溃。
- RapidOCR：基于 ONNX Runtime，轻量、跨平台一致、CPU 更快，推荐。
"""
import sys
from typing import List, Tuple

import numpy as np


class BaseEngine:
    def recognize(self, img_bgr: np.ndarray) -> Tuple[str, float]:
        """识别一张图，返回 (拼接文本, 平均置信度)。识别不到返回 ("", 0.0)。"""
        raise NotImplementedError


# ─── PaddleOCR ────────────────────────────────────────────────

class PaddleOcrEngine(BaseEngine):
    def __init__(self, lang: str = "ch"):
        self.lang = lang
        self._ocr = None

    def _ensure(self):
        if self._ocr is not None:
            return
        from paddleocr import PaddleOCR  # 延迟导入，避免无关命令也加载大模型

        # PaddleOCR 2.x / 3.x 构造参数差异很大，从「带优化参数」到「最简」依次尝试，
        # 捕获所有异常（3.x 对未知参数抛 ValueError，2.x 抛 TypeError）。
        attempts = [
            # 3.x：关掉文档方向分类 / 去扭曲 / 文本行方向三个额外模型，字幕条用不上，大幅提速。
            # enable_mkldnn=False：绕开 paddle 在某些机器上 oneDNN/PIR 推理崩溃。
            dict(
                lang=self.lang,
                use_doc_orientation_classify=False,
                use_doc_unwarping=False,
                use_textline_orientation=False,
                enable_mkldnn=False,
            ),
            dict(lang=self.lang, use_angle_cls=False, show_log=False),  # 2.x
            dict(lang=self.lang),  # 最简兜底
        ]
        last_err = None
        for kwargs in attempts:
            try:
                self._ocr = PaddleOCR(**kwargs)
                print(f"PaddleOCR 初始化方式: {list(kwargs.keys())}", file=sys.stderr, flush=True)
                return
            except Exception as e:
                last_err = e
        raise RuntimeError(f"PaddleOCR 初始化失败: {last_err}")

    def recognize(self, img_bgr: np.ndarray) -> Tuple[str, float]:
        self._ensure()
        result = None
        for method in ("predict", "ocr"):  # 3.x 用 predict，2.x 用 ocr
            fn = getattr(self._ocr, method, None)
            if fn is None:
                continue
            try:
                result = fn(img_bgr)
                break
            except Exception:
                result = None
        if result is None:
            return "", 0.0

        texts, scores = self._parse(result)
        if not texts:
            return "", 0.0
        text = "".join(texts).strip()
        conf = float(sum(scores) / len(scores)) if scores else 0.0
        return text, conf

    @staticmethod
    def _parse(result) -> Tuple[List[str], List[float]]:
        texts: List[str] = []
        scores: List[float] = []
        if not result:
            return texts, scores

        # 形式 A（2.x）: [[ [box, (text, score)], ... ]]
        try:
            first = result[0]
            if isinstance(first, list):
                for line in first:
                    if (
                        isinstance(line, (list, tuple))
                        and len(line) >= 2
                        and isinstance(line[1], (list, tuple))
                        and len(line[1]) >= 2
                    ):
                        texts.append(str(line[1][0]))
                        scores.append(float(line[1][1]))
                if texts:
                    return texts, scores
        except Exception:
            pass

        # 形式 B（3.x predict）: [ OCRResult(dict): {'rec_texts': [...], 'rec_scores': [...]} ]
        try:
            for item in result:
                if isinstance(item, dict):
                    rt = item.get("rec_texts") or []
                    rs = item.get("rec_scores") or []
                    for i, t in enumerate(rt):
                        texts.append(str(t))
                        scores.append(float(rs[i]) if i < len(rs) else 0.0)
        except Exception:
            pass

        return texts, scores


# ─── RapidOCR ─────────────────────────────────────────────────

class RapidOcrEngine(BaseEngine):
    def __init__(self, lang: str = "ch"):
        self.lang = lang  # RapidOCR 内置中英模型，简繁都走同一套
        self._ocr = None

    def _ensure(self):
        if self._ocr is not None:
            return
        from rapidocr_onnxruntime import RapidOCR

        self._ocr = RapidOCR()
        print("RapidOCR 初始化完成", file=sys.stderr, flush=True)

    def recognize(self, img_bgr: np.ndarray) -> Tuple[str, float]:
        self._ensure()
        try:
            result, _ = self._ocr(img_bgr)
        except Exception:
            return "", 0.0
        if not result:
            return "", 0.0
        # result: [ [box, text, score], ... ]
        texts = [str(item[1]) for item in result if len(item) >= 2]
        scores = [float(item[2]) for item in result if len(item) >= 3]
        text = "".join(texts).strip()
        conf = float(sum(scores) / len(scores)) if scores else 0.0
        return text, conf


# ─── 工厂 ─────────────────────────────────────────────────────

def create_engine(name: str, lang: str = "ch") -> BaseEngine:
    if name == "rapidocr":
        return RapidOcrEngine(lang)
    return PaddleOcrEngine(lang)
