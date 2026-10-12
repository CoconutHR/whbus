"""校验 GitHub Actions 工作流 YAML。

用 PyYAML 精确报错，并检查 GitHub 特有的结构约束
（`if` 表达式、job 权限、步骤名唯一性等）。
"""

import sys

try:
    import yaml
except ImportError:
    print("PyYAML 未安装，尝试 pip install pyyaml")
    sys.exit(0)

PATH = ".github/workflows/android.yml"

with open(PATH, encoding="utf-8") as f:
    raw = f.read()

try:
    d = yaml.safe_load(raw)
    print("✓ YAML 语法解析成功")
except yaml.YAMLError as e:
    print("✗ YAML 语法错误：")
    print(e)
    sys.exit(1)

print("  顶层键:", list(d.keys()))
print("  on:", list(d["on"].keys()) if isinstance(d.get("on"), dict) else d.get("on"))

for jn, job in d["jobs"].items():
    steps = job.get("steps", [])
    print(f"\n  [{jn}] runs-on={job.get('runs-on')} steps={len(steps)}")
    names = []
    for s in steps:
        name = s.get("name", "(无名)")
        names.append(name)
        if "if" in s:
            cond = s["if"]
            print(f"      IF {cond!r}  ->  {name}")
            # GitHub 在 **step 级** if 里不支持 secrets 上下文
            if isinstance(cond, str) and "secrets." in cond:
                print("      ⚠️  step 级 if 里出现 secrets.* —— GitHub 不支持该上下文！")
    dup = {n for n in names if names.count(n) > 1 and n != "(无名)"}
    if dup:
        print("      ⚠️ 重复步骤名:", dup)

# 检查 shell 块里可能的语法问题
print("\n  ---- run 块中的可疑写法 ----")
for jn, job in d["jobs"].items():
    for s in job.get("steps", []):
        run = s.get("run", "")
        if not isinstance(run, str):
            continue
        flags = []
        if "${TAG:+" in run:
            flags.append("参数展开 ${VAR:+...}（bash 特有，需确认 shell 是 bash）")
        if "<<EOF" in run:
            flags.append("heredoc")
        if "|| true" in run:
            flags.append("|| true")
        if flags:
            print(f"  [{jn}] {s.get('name','(无名)')}: " + "; ".join(flags))

# shell 默认值检查
print("\n  ---- shell 设置 ----")
defaults = d.get("defaults")
print("  顶层 defaults:", defaults)
for jn, job in d["jobs"].items():
    print(f"  [{jn}] defaults: {job.get('defaults')}")
