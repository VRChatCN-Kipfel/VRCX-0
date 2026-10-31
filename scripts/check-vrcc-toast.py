"""读取 Windows 通知中心数据库，确认 VRCX-0 的 toast 是否真的投递成功。

用法：
    python check-vrcc-toast.py              # 列出最近的 VRCX-0 通知
    python check-vrcc-toast.py --watch      # 持续盯着，出现新通知就打印

为什么需要这个：`DesktopNotifier::show()` 返回 Ok 只代表调用没报错，
不代表 Windows 真的接收了。真正的证据在系统的 wpndatabase.db 里。

注意：数据库被系统独占，必须先复制再读，不能直接打开。
"""
import os
import re
import shutil
import sqlite3
import sys
import tempfile
import time
from datetime import datetime, timedelta

# 本项目在通知中心注册的 AUMID（来自 tauri.conf.json 的 identifier）
AUMID = "com.vrcx-0.app"
DB = os.path.join(
    os.environ["LOCALAPPDATA"], r"Microsoft\Windows\Notifications\wpndatabase.db"
)


def filetime_to_dt(value):
    """Windows FILETIME → datetime；非法值返回 None。"""
    try:
        return datetime(1601, 1, 1) + timedelta(microseconds=int(value) / 10)
    except Exception:
        return None


def read_rows(limit=20):
    """把系统通知库复制到临时目录后读取（原库被系统锁着）。"""
    if not os.path.exists(DB):
        raise SystemExit(f"找不到通知数据库: {DB}")

    tmp = os.path.join(tempfile.gettempdir(), "dsh-wpn-read.db")
    shutil.copy2(DB, tmp)
    con = sqlite3.connect(tmp)
    try:
        hit = list(
            con.execute(
                "select RecordId from NotificationHandler where PrimaryId = ?", (AUMID,)
            )
        )
        if not hit:
            return None  # 应用还没注册到通知中心
        handler_id = hit[0][0]
        rows = list(
            con.execute(
                "select Id, ArrivalTime, Payload from Notification "
                "where HandlerId = ? order by ArrivalTime desc limit ?",
                (handler_id, limit),
            )
        )
    finally:
        con.close()
        try:
            os.remove(tmp)
        except OSError:
            pass

    out = []
    for notif_id, arrival, payload in rows:
        raw = (
            payload.decode("utf-8", "ignore")
            if isinstance(payload, bytes)
            else str(payload)
        )
        texts = [t.strip() for t in re.findall(r"<text[^>]*>(.*?)</text>", raw, re.S)]
        out.append(
            {
                "id": notif_id,
                "at": filetime_to_dt(arrival),
                "title": texts[0] if texts else "",
                "body": texts[1] if len(texts) > 1 else "",
            }
        )
    return out


def describe(row):
    at = row["at"].strftime("%Y-%m-%d %H:%M:%S") if row["at"] else "?"
    title = row["title"][:40]
    body = row["body"][:40]
    return f"  {at}  {title!r}" + (f"  body={body!r}" if body else "")


def main():
    watch = "--watch" in sys.argv
    rows = read_rows()
    if rows is None:
        print(f"[X] {AUMID} 尚未注册到通知中心（应用从未发过通知）")
        return 1
    if not rows:
        print(f"[!] {AUMID} 已注册，但通知库为空")
        return 1

    print(f"=== {AUMID} 在 Windows 通知中心的记录（{len(rows)} 条）===")
    for row in rows:
        print(describe(row))

    vrcc_rows = [r for r in rows if "星遇" in r["title"] or "VRCC" in r["title"].upper()]
    print()
    if vrcc_rows:
        print(f"[OK] 找到 {len(vrcc_rows)} 条星遇同频通知 —— toast 投递确认")
        for row in vrcc_rows:
            print(describe(row))
        return 0

    print("[i] 以上没有星遇同频的提醒。")
    print("   常见原因：应用开着的这段时间没来新提醒（启动前来的不补弹）。")
    print("   排查：看 diagnostic-log.txt 里 `vrcc: background poll round finished` 的 fresh 值。")

    if not watch:
        return 1

    # 持续盯着：出现新记录就打印，便于实时确认投递。
    print("\n持续监视中（Ctrl+C 退出）…")
    seen = {r["id"] for r in rows}
    try:
        while True:
            time.sleep(10)
            current = read_rows() or []
            for row in current:
                if row["id"] not in seen:
                    seen.add(row["id"])
                    print(f"[新通知] {describe(row)}")
    except KeyboardInterrupt:
        print("\n已停止。")
    return 0


if __name__ == "__main__":
    # Windows 控制台默认是 GBK，直接 print emoji 会 UnicodeEncodeError。
    # 重配成 UTF-8 并允许无法编码的字符降级，避免脚本因为日志符号而崩。
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass
    sys.exit(main())
