#!/usr/bin/env bash
# 让上游 workspace 的 craft-anything-platform 指向本项目的 OHOS fork。
#
# 上游 crate 用 `craft-anything-platform = { workspace = true }`，解析成上游 workspace 内的
# path 依赖。Cargo 的 [patch] 和 .cargo/config.toml 的 paths override 都无法覆盖同一
# workspace 里的 path 依赖（已用 cargo metadata 验证），因此构建前必须改写上游
# Cargo.toml 的这一行。fork 在桌面目标下行为与上游完全一致，只给 OHOS 加外部进程桥。
#
# 可重复执行；若上游已指向 fork 则什么都不做。
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UPSTREAM="$ROOT/../_upstream/Cargo.toml"
[ -f "$UPSTREAM" ] || { echo "upstream workspace not found: $UPSTREAM" >&2; exit 1; }

python - "$UPSTREAM" <<'PY'
from pathlib import Path
import sys

p = Path(sys.argv[1])
s = p.read_text(encoding="utf-8")
old = 'craft-anything-platform = { path = "crates/platform" }'
new = 'craft-anything-platform = { path = "../craftanything-hmos/vendor/platform-ohos" }'
if new in s:
    print("exec bridge: upstream already points at the OHOS fork")
elif old in s:
    p.write_text(s.replace(old, new, 1), encoding="utf-8")
    print("exec bridge: upstream platform dependency redirected to OHOS fork")
else:
    print("exec bridge: unexpected upstream Cargo.toml layout, left unchanged", file=sys.stderr)
    sys.exit(1)
PY
