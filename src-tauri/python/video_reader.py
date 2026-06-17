"""视频读取：获取信息、按 sample_fps 抽样、裁剪字幕 ROI。"""
from typing import Iterator, Tuple

import cv2
import numpy as np


class VideoReader:
    def __init__(self, video_path: str):
        self.path = video_path
        self.cap = cv2.VideoCapture(video_path)
        if not self.cap.isOpened():
            raise RuntimeError(f"无法打开视频: {video_path}")
        self.fps = self.cap.get(cv2.CAP_PROP_FPS) or 0.0
        if self.fps <= 0:
            self.fps = 25.0  # 兜底
        self.frame_count = int(self.cap.get(cv2.CAP_PROP_FRAME_COUNT) or 0)
        self.width = int(self.cap.get(cv2.CAP_PROP_FRAME_WIDTH) or 0)
        self.height = int(self.cap.get(cv2.CAP_PROP_FRAME_HEIGHT) or 0)
        self.duration = self.frame_count / self.fps if self.frame_count else 0.0

    def sample_count(self, sample_fps: float) -> int:
        step = max(1, round(self.fps / max(0.1, sample_fps)))
        if self.frame_count <= 0:
            return 0
        return (self.frame_count + step - 1) // step

    def iter_samples(self, sample_fps: float) -> Iterator[Tuple[float, np.ndarray]]:
        """按 sample_fps 抽样，yield (时间戳秒, BGR 帧)。

        用 grab() 廉价跳帧，只在采样点 retrieve()，避免逐帧解码开销。
        """
        step = max(1, round(self.fps / max(0.1, sample_fps)))
        idx = 0
        while True:
            grabbed = self.cap.grab()
            if not grabbed:
                break
            if idx % step == 0:
                ok, frame = self.cap.retrieve()
                if ok and frame is not None:
                    ts = idx / self.fps
                    yield ts, frame
            idx += 1

    @staticmethod
    def crop_region(frame: np.ndarray, region: dict) -> np.ndarray:
        """按相对坐标 (x,y,w,h ∈ 0~1) 裁剪 ROI。"""
        h, w = frame.shape[:2]
        x0 = int(round(region.get("x", 0.0) * w))
        y0 = int(round(region.get("y", 0.0) * h))
        x1 = int(round((region.get("x", 0.0) + region.get("w", 1.0)) * w))
        y1 = int(round((region.get("y", 0.0) + region.get("h", 1.0)) * h))
        x0 = max(0, min(x0, w - 1))
        y0 = max(0, min(y0, h - 1))
        x1 = max(x0 + 1, min(x1, w))
        y1 = max(y0 + 1, min(y1, h))
        return frame[y0:y1, x0:x1]

    def release(self):
        if self.cap:
            self.cap.release()
