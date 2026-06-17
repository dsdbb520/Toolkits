"""OCR 文本清洗：去噪、压空格，保留中文标点。"""
import re

# 明显的 OCR 噪声/装饰字符（竖线、方块、箭头等），用转义避免隐藏字符
_NOISE_CHARS = "".join([
    "|",  # |
    "｜",  # ｜
    "■□",  # ■ □
    "●○",  # ● ○
    "◇◆",  # ◇ ◆
    "※",  # ※
    "→←↑↓",  # → ← ↑ ↓
])
_NOISE_RE = re.compile("[" + re.escape(_NOISE_CHARS) + "]")
# 多个空白压成一个
_SPACE_RE = re.compile(r"\s+")
# CJK 统一表意文字 + 常见中文标点范围，用于判断是否含有效内容
_CJK_RE = re.compile(r"[一-鿿　-〿＀-￯]")
# 两个 CJK 之间的空格（多为误识别）
_CJK_GAP_RE = re.compile(r"(?<=[一-鿿])\s+(?=[一-鿿])")


def normalize(text: str) -> str:
    """清洗单条 OCR 文本。返回清洗后的字符串（可能为空）。"""
    if not text:
        return ""
    text = _NOISE_RE.sub("", text)
    text = _SPACE_RE.sub(" ", text).strip()
    # 中文字符之间的空格删除（保留英文单词间空格）
    text = _CJK_GAP_RE.sub("", text)
    return text.strip()


def is_meaningful(text: str, min_len: int = 1) -> bool:
    """判断文本是否值得保留：非空、含 CJK 或足够长。"""
    if not text or len(text) < min_len:
        return False
    if _CJK_RE.search(text):
        return True
    return len(text.strip()) >= 2
