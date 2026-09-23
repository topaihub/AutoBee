#!/usr/bin/env bash
# 把便携 MSVC 注入当前 shell（Git Bash 版，与 scripts/cargo-msvc.bat 等价）。
#
# 用法： source scripts/msvc-env.sh && cargo test -p route-core
#
# 两个坑，都是实测出来的：
# 1. Git 的 coreutils 自带 link.exe，抢在 MSVC 链接器前面。
#    所以必须把 MSVC 的 bin 目录放到 PATH 最前面 —— 探测 cl.exe 而不是
#    link.exe，因为 coreutils 也有 link.exe。
# 2. Git Bash 对 PATH 做 MSYS 路径转换，但对 LIB/INCLUDE 不做。
#    所以 PATH 用 POSIX 风格（/d/...），LIB 与 INCLUDE 用 Windows 风格
#    （D:\...;D:\...）。混用任何一种都会得到
#    「cannot open input file 'kernel32.lib'」。

VCTOOLS="14.44.35207"
SDK="10.0.26100.0"
MSVC_ROOT="${PORTABLE_MSVC_ROOT:-/d/Applications/msvc-portable/msvc}"
MSVC_ROOT_WIN='D:\Applications\msvc-portable\msvc'

if [ ! -f "$MSVC_ROOT/VC/Tools/MSVC/$VCTOOLS/bin/Hostx64/x64/cl.exe" ]; then
    echo "ERROR: 便携 MSVC 不在 $MSVC_ROOT（缺 cl.exe）。" >&2
    echo "可用 PORTABLE_MSVC_ROOT 指到含 setup_x64.bat 的目录。" >&2
    return 1 2>/dev/null || exit 1
fi

export PATH="$MSVC_ROOT/VC/Tools/MSVC/$VCTOOLS/bin/Hostx64/x64:$MSVC_ROOT/Windows Kits/10/bin/$SDK/x64:$PATH"

export INCLUDE="$MSVC_ROOT_WIN\\VC\\Tools\\MSVC\\$VCTOOLS\\include;$MSVC_ROOT_WIN\\Windows Kits\\10\\Include\\$SDK\\ucrt;$MSVC_ROOT_WIN\\Windows Kits\\10\\Include\\$SDK\\shared;$MSVC_ROOT_WIN\\Windows Kits\\10\\Include\\$SDK\\um"
export LIB="$MSVC_ROOT_WIN\\VC\\Tools\\MSVC\\$VCTOOLS\\lib\\x64;$MSVC_ROOT_WIN\\Windows Kits\\10\\Lib\\$SDK\\ucrt\\x64;$MSVC_ROOT_WIN\\Windows Kits\\10\\Lib\\$SDK\\um\\x64"

# 验证一下。失败的话宁可现在报，也不要等编译到一半才发现。
if ! command -v cl.exe >/dev/null 2>&1; then
    echo "ERROR: 注入后仍找不到 cl.exe，工具链不完整。" >&2
    return 1 2>/dev/null || exit 1
fi
echo "MSVC 已注入（$VCTOOLS / SDK $SDK）"
