#!/usr/bin/env python3
"""Process counters for bench.mjs: reads lines of PIDs on stdin, answers each line with one JSON line
{"<pid>": [cpu_ns, rss_bytes, wakeups, instructions, peak_footprint_bytes] | null}.
`ps -o time` counts in 10 ms steps — too coarse for an idle server (≈ 10-100 ms CPU per minute). macOS:
proc_pid_rusage(RUSAGE_INFO_V4): user + system time (mach units → ns), resident size, interrupt + package-idle
wakeups, retired instructions (independent of the clock speed the power mode sets), lifetime peak physical
footprint. Linux: /proc/<pid>/stat (clock ticks) + VmRSS + VmHWM; wakeups and instructions null."""
import ctypes, json, os, sys

if sys.platform == "darwin":
    V4 = ["user", "system", "pkg_idle_wkups", "interrupt_wkups", "pageins", "wired", "resident", "footprint", "proc_start", "proc_exit",
          "child_user", "child_system", "child_pkg_idle_wkups", "child_interrupt_wkups", "child_pageins", "child_elapsed", "diskio_read", "diskio_written",
          "qos_default", "qos_maintenance", "qos_background", "qos_utility", "qos_legacy", "qos_user_initiated", "qos_user_interactive",
          "billed_system", "serviced_system", "logical_writes", "lifetime_max_footprint", "instructions", "cycles", "billed_energy",
          "serviced_energy", "interval_max_footprint", "runnable"]

    class RU(ctypes.Structure):  # struct rusage_info_v4 (sys/resource.h)
        _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(f, ctypes.c_uint64) for f in V4]

    class TB(ctypes.Structure):
        _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]
    lib = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
    tb = TB(); lib.mach_timebase_info(ctypes.byref(tb))

    def sample(pid):
        ru = RU()
        if lib.proc_pid_rusage(pid, 4, ctypes.byref(ru)) != 0:
            return None
        return [(ru.user + ru.system) * tb.numer // tb.denom, ru.resident, ru.interrupt_wkups + ru.pkg_idle_wkups, ru.instructions, ru.lifetime_max_footprint]
else:
    HZ = os.sysconf("SC_CLK_TCK")

    def sample(pid):
        try:
            f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
            st = {l.split(":")[0]: int(l.split()[1]) * 1024 for l in open(f"/proc/{pid}/status") if l.startswith(("VmRSS:", "VmHWM:"))}
        except OSError:
            return None
        return [(int(f[11]) + int(f[12])) * 1_000_000_000 // HZ, st.get("VmRSS"), None, None, st.get("VmHWM")]

for line in sys.stdin:
    print(json.dumps({p: sample(int(p)) for p in line.split()}), flush=True)
