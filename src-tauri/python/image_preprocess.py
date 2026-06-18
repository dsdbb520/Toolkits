"""字幕区域图像预处理：放大 + 灰度 + 提升对比度，帮助 OCR 识别小字幕。"""
import cv2
import numpy as np


def preprocess(roi_bgr: np.ndarray, upscale: float = 2.0) -> np.ndarray:
    """对裁剪出的字幕区域做基础预处理。

    保持轻量：放大 + 去噪 + 自适应对比度。返回 BGR 图（PaddleOCR 接受 BGR/RGB）。
    """
    if roi_bgr is None or roi_bgr.size == 0:
        return roi_bgr

    img = roi_bgr
    # 放大小字幕（双三次插值）
    if upscale and upscale > 1.0:
        img = cv2.resize(
            img, None, fx=upscale, fy=upscale, interpolation=cv2.INTER_CUBIC
        )

    # CLAHE 对比度增强（在亮度通道上做，保留彩色字幕信息）
    # 注：原先的 bilateralFilter 很慢且收益有限，已去掉
    lab = cv2.cvtColor(img, cv2.COLOR_BGR2LAB)
    l, a, b = cv2.split(lab)
    clahe = cv2.createCLAHE(clipLimit=2.0, tileGridSize=(8, 8))
    l = clahe.apply(l)
    img = cv2.cvtColor(cv2.merge((l, a, b)), cv2.COLOR_LAB2BGR)

    return img


def roi_signature(roi_bgr: np.ndarray):
    """生成 ROI 的小尺寸灰度签名，用于廉价地判断相邻采样帧字幕是否变化。"""
    if roi_bgr is None or roi_bgr.size == 0:
        return None
    g = cv2.cvtColor(roi_bgr, cv2.COLOR_BGR2GRAY)
    return cv2.resize(g, (128, 32), interpolation=cv2.INTER_AREA).astype(np.int16)


def signature_diff(a, b) -> float:
    """两个签名的平均绝对差（0~255）。值越小越相似。"""
    if a is None or b is None:
        return 1e9
    return float(np.abs(a - b).mean())


def signature_changed(
    a,
    b,
    mean_threshold: float = 1.8,
    top_threshold: float = 10.0,
    ratio_threshold: float = 0.004,
) -> bool:
    """判断两个 ROI 签名是否发生了值得 OCR 的变化。

    短字幕经常只占字幕 ROI 的很小一块区域，单看平均差值会被背景面积稀释。
    这里同时看整体均值、变化最明显的一小撮像素、以及明显变化像素占比。
    """
    if a is None or b is None:
        return True

    diff = np.abs(a - b).astype(np.float32)
    mean = float(diff.mean())
    if mean >= mean_threshold:
        return True

    flat = diff.reshape(-1)
    top_n = max(1, min(64, flat.size))
    top_start = flat.size - top_n
    top_mean = float(np.partition(flat, top_start)[top_start:].mean())
    if top_mean >= top_threshold:
        return True

    changed_ratio = float((flat >= 18.0).mean())
    return changed_ratio >= ratio_threshold
