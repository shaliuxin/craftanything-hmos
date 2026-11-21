#!/usr/bin/env python3
"""把上游 craft-anything 的成品应用图标（1024x1024，圆角已烘焙）落到 AppScope 与 entry 两处。

图标来源：`_upstream/assets/app-icon/craft-anything-1024.png`
（乌鸦头木刻风格，蓝色圆角底 —— 上游自带的正式发布图标，无需再做品牌加工）。

用法：
    python scripts/make_icon.py                 # 落到 AppScope 与 entry 两处
    python scripts/make_icon.py out.png         # 落到指定路径
"""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

# 上游目录默认在仓库上一级；可用 CRAFTANYTHING_UPSTREAM 覆盖。
UPSTREAM = os.environ.get("CRAFTANYTHING_UPSTREAM", os.path.join(ROOT, "..", "_upstream"))
LOGO = os.path.join(UPSTREAM, "assets", "app-icon", "craft-anything-1024.png")
TARGETS = [
    os.path.join(ROOT, "AppScope", "resources", "base", "media", "app_icon.png"),
    os.path.join(ROOT, "entry", "src", "main", "resources", "base", "media", "app_icon.png"),
]


def main(argv: list) -> int:
    paths = [os.path.abspath(argv[0])] if argv else list(TARGETS)
    if not os.path.exists(LOGO):
        print(f"missing upstream icon: {LOGO}", file=sys.stderr)
        return 1
    for path in paths:
        shutil.copyfile(LOGO, path)
        print(f"wrote {path} ({os.path.getsize(path)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
