"""生成 whbus 的 App 图标源图。

设计：品牌蓝圆角方块 + 白色公交侧影 + 一条橙色"正在靠近"的位置点。
刻意保持几何极简——小尺寸下仍要能辨认，所以只用 3 个视觉元素。
"""

from PIL import Image, ImageDraw

S = 1024
BG = (37, 99, 235)        # --brand 浅色主题值
FG = (255, 255, 255)
ACCENT = (251, 146, 60)   # --accent

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# --- 圆角方块底 ---
d.rounded_rectangle([0, 0, S - 1, S - 1], radius=int(S * 0.22), fill=BG)

# --- 公交侧影（圆角矩形车身 + 车窗带 + 两个车轮）---
bx0, by0, bx1, by1 = int(S * 0.22), int(S * 0.26), int(S * 0.78), int(S * 0.70)
d.rounded_rectangle([bx0, by0, bx1, by1], radius=int(S * 0.06), fill=FG)

# 车窗带（用底色挖空，形成负空间）
wx0, wy0, wx1, wy1 = int(S * 0.28), int(S * 0.33), int(S * 0.72), int(S * 0.45)
d.rounded_rectangle([wx0, wy0, wx1, wy1], radius=int(S * 0.02), fill=BG)

# 两个车轮
r = int(S * 0.055)
cy = int(S * 0.70)
for cx in (int(S * 0.35), int(S * 0.65)):
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=FG)

# --- 橙色位置点：暗示"实时靠近" ---
pr = int(S * 0.075)
pcx, pcy = int(S * 0.755), int(S * 0.755)
# 外圈光晕
d.ellipse(
    [pcx - int(pr * 1.75), pcy - int(pr * 1.75), pcx + int(pr * 1.75), pcy + int(pr * 1.75)],
    fill=(255, 255, 255, 70),
)
d.ellipse([pcx - pr, pcy - pr, pcx + pr, pcy + pr], fill=ACCENT, outline=FG, width=int(S * 0.012))

img.save("src-tauri/icons/source.png")
print("wrote src-tauri/icons/source.png", img.size)

# 同时导出一份 32px 预览，便于肉眼确认小尺寸可读性
img.resize((32, 32), Image.LANCZOS).resize((256, 256), Image.NEAREST).save(
    "src-tauri/icons/_preview_32.png"
)
print("wrote 32px preview")
