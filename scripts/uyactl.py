"""Drive one running `uya play --control` (the game on the port's own runtime; port/uya/src/control.rs) instead of rebooting the
game for each check.

    python scripts/uyactl.py [-c "CMD; CMD; ..."] [-f FILE] [--profile DIR] [--exe EXE] [--iso ISO] [-v] [-- PLAY ARGS...]

Starts the game with --control (and --fast pacing: fields run only when a command asks, as fast as the game's code goes), then
sends each command and waits for its answer (`ctl: ok ...` or `ctl: error ...`), printing it with the seconds it took. Commands
come from -c (separated by ';'), then from -f (one a line, '#' comments), then, with neither, from the keyboard. A `quit` is sent
at the end; a list (-c, -f) stops at its first error, and the exit code is 1 then. Commands: run N, press BUTTONS N, stick X Y N [BUTTONS], pointer X Y [click], warp LEVEL, until pause|menu|level L|field F [max N],
ram ADDR LEN, write ADDR HEX, shot FILE, quit (control.rs says what each does).

The profile defaults to captures/ctl/profile (made fresh, never the player's own, but for its copy of the player's texture packs,
made the first time); -v prints the game's own log lines too, and without it a game that ends on its own (a panic, an error:
exit 1) has its last lines printed with the command it ended on.
The game runs under the PC's heavy-job rules (scripts/heavy-slot.ps1, docs/WORKFLOW.md "Memory and processes"): it holds one
of the run slots (the `uya-runN-slot` mutexes of the logon session; --exclusive, for a run whose numbers are timings, every build slot
and every run slot; how many there are comes from the PC, as in heavy-slot.ps1) for its whole length, starts only with that
script's free RAM, and is stopped if its private memory passes
--limit-mb (3072), as watch-run.ps1 does for the other runs.
Example, from the boot to Veldin's QUICK SELECT in one session:
    python scripts/uyactl.py -c "until level 1; until field 2000; press 800 4; until pause; ...; shot captures/ctl/q.png" -- --start 1
"""

import argparse
import collections
import ctypes
import ctypes.wintypes as wt
import os
import queue
import shutil
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
K32 =ctypes.WinDLL('kernel32', use_last_error=True)
K32.CreateMutexW.restype = wt.HANDLE
K32.CreateMutexW.argtypes = [ctypes.c_void_p, wt.BOOL, wt.LPCWSTR]
K32.WaitForMultipleObjects.restype = wt.DWORD
K32.WaitForMultipleObjects.argtypes = [wt.DWORD, ctypes.POINTER(wt.HANDLE), wt.BOOL, wt.DWORD]
K32.ReleaseMutex.argtypes = [wt.HANDLE]
K32.CloseHandle.argtypes = [wt.HANDLE]
K32.OpenProcess.restype = wt.HANDLE
K32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]


class MemoryStatus(ctypes.Structure):
    _fields_ = [('dwLength', wt.DWORD), ('dwMemoryLoad', wt.DWORD), ('ullTotalPhys', ctypes.c_ulonglong), ('ullAvailPhys', ctypes.c_ulonglong),
                ('ullTotalPageFile', ctypes.c_ulonglong), ('ullAvailPageFile', ctypes.c_ulonglong), ('ullTotalVirtual', ctypes.c_ulonglong),
                ('ullAvailVirtual', ctypes.c_ulonglong), ('ullAvailExtendedVirtual', ctypes.c_ulonglong)]


class ProcessMemory(ctypes.Structure):
    _fields_ = [('cb', wt.DWORD), ('PageFaultCount', wt.DWORD), ('PeakWorkingSetSize', ctypes.c_size_t), ('WorkingSetSize', ctypes.c_size_t),
                ('QuotaPeakPagedPoolUsage', ctypes.c_size_t), ('QuotaPagedPoolUsage', ctypes.c_size_t), ('QuotaPeakNonPagedPoolUsage', ctypes.c_size_t),
                ('QuotaNonPagedPoolUsage', ctypes.c_size_t), ('PagefileUsage', ctypes.c_size_t), ('PeakPagefileUsage', ctypes.c_size_t), ('PrivateUsage', ctypes.c_size_t)]


def memory():
    m = MemoryStatus()
    m.dwLength = ctypes.sizeof(m)
    if not K32.GlobalMemoryStatusEx(ctypes.byref(m)):
        raise OSError(ctypes.get_last_error(), 'GlobalMemoryStatusEx')
    return m


def free_gb():
    return memory().ullAvailPhys / 2 ** 30


# heavy-slot.ps1's numbers, worked out the same way from the PC: two build slots only with 28 GB of RAM and 12 logical CPUs,
# three run slots with 28 GB, two with 14 GB, else one; a job waits for a fifth of the RAM free, at most 6 GB.
TOTAL_GB = memory().ullTotalPhys / 2 ** 30
BUILD_SLOTS = 2 if TOTAL_GB >= 28 and os.cpu_count() >= 12 else 1
RUN_SLOTS = 3 if TOTAL_GB >= 28 else 2 if TOTAL_GB >= 14 else 1
FREE_GB = min(6, int(TOTAL_GB // 5))


def private_mb(handle):
    m = ProcessMemory()
    m.cb = ctypes.sizeof(m)
    if not K32.K32GetProcessMemoryInfo(handle, ctypes.byref(m), m.cb):
        raise OSError(ctypes.get_last_error(), 'GetProcessMemoryInfo')
    return m.PrivateUsage / 2 ** 20


def wait_any(names, what):
    """Takes one of the named mutexes (heavy-slot.ps1's Wait-Any): an abandoned one is taken over. Returns its handle."""
    hs = []
    for n in names:
        h = K32.CreateMutexW(None, False, 'Local\\uya-%s-slot' % n)
        if not h:
            raise OSError(ctypes.get_last_error(), 'CreateMutexW %s' % n)
        hs.append(h)
    arr = (wt.HANDLE * len(hs))(*hs)
    t0, said = time.time(), -1
    while True:
        r = K32.WaitForMultipleObjects(len(hs), arr, False, 30000)
        if r < len(hs) or 0x80 <= r < 0x80 + len(hs):
            k = r if r < len(hs) else r - 0x80
            for j, h in enumerate(hs):
                if j != k:
                    K32.CloseHandle(h)
            return hs[k]
        if r != 0x102:
            raise OSError(ctypes.get_last_error(), 'WaitForMultipleObjects')
        m = int((time.time() - t0) / 60)
        if said < 0 or m - said >= 5:
            said = m
            print('heavy slot: %s waits for a %s slot (%d min so far)' % (what, names[0].rstrip('0123456789'), m), flush=True)


def enter_slot(what, exclusive, minimum_free_gb=0):
    """heavy-slot.ps1's Enter-HeavySlot for a run: one run slot, or (exclusive) every build slot then every run slot, each in
    order; then the PC's free-RAM threshold (or the explicit measured-workload threshold). Returns the handles held."""
    t0 = time.time()
    if exclusive:
        held = [wait_any(['build%d' % k], what) for k in range(1, BUILD_SLOTS + 1)] + [wait_any(['run%d' % k], what) for k in range(1, RUN_SLOTS + 1)]
    else:
        held = [wait_any(['run%d' % k for k in range(1, RUN_SLOTS + 1)], what)]
    threshold = minimum_free_gb or FREE_GB
    while free_gb() < threshold:
        print('heavy slot: %s waits for %.1f GB of free RAM' % (what, threshold), flush=True)
        time.sleep(30)
    print('heavy slot: %s starts after %.0f s (run%s)' % (what, time.time() - t0, ', exclusive' if exclusive else ''), flush=True)
    return held


def exit_slot(held):
    for h in reversed(held):
        K32.ReleaseMutex(h)
        K32.CloseHandle(h)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('-c', '--commands', default='')
    ap.add_argument('-f', '--file')
    ap.add_argument('--profile', default=os.path.join(ROOT, 'captures', 'ctl', 'profile'))
    ap.add_argument('--keep-profile', action='store_true', help='keep the profile folder as it is (default: made fresh)')
    ap.add_argument('--exe', default=os.path.join(ROOT, 'port', 'target', 'release', 'uya.exe'))
    ap.add_argument('--iso', default=os.path.join(ROOT, 'disc', 'SCUS_973.53.iso'))
    ap.add_argument('-v', '--verbose', action='store_true')
    ap.add_argument('--exclusive', action='store_true', help='hold the build slot and every run slot (a run whose numbers are timings)')
    ap.add_argument('--limit-mb', type=float, default=3072, help='stop the game above this private memory (default 3072)')
    ap.add_argument('--minimum-free-gb', type=float, default=0, help='launch threshold for a known measured workload (0 keeps the PC default); memory cap still applies')
    ap.add_argument('play', nargs='*', help='more `uya play` arguments (after --)')
    a = ap.parse_args()
    if not 0 <= a.minimum_free_gb <= 128:
        ap.error('--minimum-free-gb must be between 0 and 128')

    # A fresh profile keeps its copy of the player's texture packs (the game always draws them, from the profile's
    # texture_replacements: 1.3 GB, copied the first time only).
    packs = 'texture_replacements'
    os.makedirs(a.profile, exist_ok=True)
    if not a.keep_profile:
        for e in os.listdir(a.profile):
            if e == packs:
                continue
            path = os.path.join(a.profile, e)
            if os.path.isdir(path):
                shutil.rmtree(path)
            else:
                os.remove(path)
    player = os.path.join(os.environ['USERPROFILE'], 'Games', 'Ratchet and Clank 3')
    atlas = os.path.join(player, 'prompts.bin')
    if os.path.exists(atlas) and not os.path.exists(os.path.join(a.profile, 'prompts.bin')):
        shutil.copy(atlas, a.profile)
    if not os.path.exists(os.path.join(a.profile, packs)):
        print(f'uyactl: copying the texture packs into {a.profile} (once)', flush=True)
        shutil.copytree(os.path.join(player, packs), os.path.join(a.profile, packs))

    commands = [c.strip() for c in a.commands.split(';') if c.strip()]
    if a.file:
        with open(a.file, encoding='utf-8') as f:
            commands += [line.strip() for line in f if line.strip() and not line.strip().startswith('#')]
    interactive = not commands

    argv = [a.exe, 'play', a.iso, '--control', '--profile', a.profile] + a.play
    held = enter_slot('uyactl ' + ' '.join(a.play), a.exclusive, a.minimum_free_gb)
    try:
        good = run(a, argv, commands, interactive)
    finally:
        exit_slot(held)
    sys.exit(0 if good else 1)


def run(a, argv, commands, interactive):
    t0 = time.time()
    p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1, encoding='utf-8', errors='replace')
    # The private-memory watch (watch-run.ps1's): sampled every second, the game stopped above the limit.
    peak, stopped = [0.0], [False]
    handle = K32.OpenProcess(0x1000, False, p.pid)

    def watch():
        while p.poll() is None:
            try:
                mb = private_mb(handle)
            except OSError:
                break
            peak[0] = max(peak[0], mb)
            if mb > a.limit_mb:
                stopped[0] = True
                print('uyactl: the game passed %.0f MB of private memory (%.0f MB): stopped' % (a.limit_mb, mb), flush=True)
                p.kill()
                break
            time.sleep(1)

    threading.Thread(target=watch, daemon=True).start()
    answers = queue.Queue()
    # The game's own last lines (its log, and on stderr why it stopped: a panic, an error), printed when the game ends without being
    # told to, so that a run that ends with exit 1 always says why, -v or not.
    tail = collections.deque(maxlen=12)

    def read():
        for line in p.stdout:
            line = line.rstrip('\n')
            if line.startswith('ctl: '):
                answers.put(line[5:])
            elif a.verbose:
                print('   ' + line, flush=True)
            else:
                tail.append(line)
        answers.put(None)

    threading.Thread(target=read, daemon=True).start()

    def send(cmd):
        """Returns 'ok', 'error', 'ended' on unexpected EOF, or 'quit' on an acknowledged quit."""
        t = time.time()
        try:
            p.stdin.write(cmd + '\n')
            p.stdin.flush()
        except BrokenPipeError:
            # The reader still drains the game's final diagnostic and queues EOF.
            pass
        ans = answers.get()
        if ans is None:
            print('%-40s -> the game ended; its last lines:' % cmd, flush=True)
            for line in tail:
                print('   ' + line, flush=True)
            return 'ended'
        print('%6.1f s  %-40s -> %s (%.1f s)' % (time.time() - t0, cmd, ans, time.time() - t), flush=True)
        return 'quit' if ans.startswith('ok quit') else 'error' if ans.startswith('error') else 'ok'

    state = 'ok'
    if interactive:
        for line in sys.stdin:
            if line.strip():
                state = send(line.strip())
                if state in ('quit', 'ended'):
                    break
    else:
        # A list stops at its first error: the commands after it were written for the state it did not reach (a click meant for a
        # pause screen would be Cross in the level, a jump).
        for cmd in commands:
            state = send(cmd)
            if state != 'ok':
                break
        if state == 'error':
            print('uyactl: the list stopped at its first error; the rest were not sent', flush=True)
    if state not in ('ended', 'quit'):
        quit_state = send('quit')
        if state == 'ok':
            state = quit_state
    try:
        p.wait(timeout=30)
    except subprocess.TimeoutExpired:
        p.kill()
        p.wait()
    K32.CloseHandle(handle)
    print('%6.1f s  the game ended (exit %s, peak private %.0f MB%s)' % (time.time() - t0, p.returncode, peak[0], ', stopped at the memory limit' if stopped[0] else ''))
    return state == 'quit' and p.returncode == 0 and not stopped[0]


if __name__ == '__main__':
    main()
