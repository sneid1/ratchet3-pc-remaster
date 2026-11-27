# Starts a program with the start information Explorer gives a double-clicked one: STARTF_HASSHELLDATA (0x400) in dwFlags and a
# monitor in hStdOutput, no output handles. stdin is a pipe (Explorer gives none): the lines of --stdin are written to it, which
# is how a hidden `uya play --control` run is told what to do. Waits for the program to end.
#
#   python shellstart.py [--stdin "line;line"] [--timeout S] -- PROGRAM ARGS...
import ctypes, ctypes.wintypes as w, subprocess, sys, time

k = ctypes.WinDLL("kernel32", use_last_error=True)
u = ctypes.WinDLL("user32", use_last_error=True)


class STARTUPINFOW(ctypes.Structure):
    _fields_ = [("cb", w.DWORD), ("lpReserved", w.LPWSTR), ("lpDesktop", w.LPWSTR), ("lpTitle", w.LPWSTR), ("dwX", w.DWORD), ("dwY", w.DWORD),
                ("dwXSize", w.DWORD), ("dwYSize", w.DWORD), ("dwXCountChars", w.DWORD), ("dwYCountChars", w.DWORD), ("dwFillAttribute", w.DWORD),
                ("dwFlags", w.DWORD), ("wShowWindow", w.WORD), ("cbReserved2", w.WORD), ("lpReserved2", ctypes.c_void_p),
                ("hStdInput", w.HANDLE), ("hStdOutput", w.HANDLE), ("hStdError", w.HANDLE)]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", w.HANDLE), ("hThread", w.HANDLE), ("dwProcessId", w.DWORD), ("dwThreadId", w.DWORD)]


class SECURITY_ATTRIBUTES(ctypes.Structure):
    _fields_ = [("nLength", w.DWORD), ("lpSecurityDescriptor", ctypes.c_void_p), ("bInheritHandle", w.BOOL)]


u.MonitorFromPoint.restype = w.HANDLE
u.MonitorFromPoint.argtypes = [w.POINT, w.DWORD]
k.CreateProcessW.argtypes = [w.LPCWSTR, w.LPWSTR, ctypes.c_void_p, ctypes.c_void_p, w.BOOL, w.DWORD, ctypes.c_void_p, w.LPCWSTR, ctypes.POINTER(STARTUPINFOW), ctypes.POINTER(PROCESS_INFORMATION)]
k.WaitForSingleObject.argtypes = [w.HANDLE, w.DWORD]
k.TerminateProcess.argtypes = [w.HANDLE, w.UINT]
k.GetExitCodeProcess.argtypes = [w.HANDLE, ctypes.POINTER(w.DWORD)]
k.WriteFile.argtypes = [w.HANDLE, ctypes.c_char_p, w.DWORD, ctypes.POINTER(w.DWORD), ctypes.c_void_p]
k.SetHandleInformation.argtypes = [w.HANDLE, w.DWORD, w.DWORD]
k.CloseHandle.argtypes = [w.HANDLE]

# Explorer has no console, so the program's AttachConsole(ATTACH_PARENT_PROCESS) fails there: this parent gives its own up.
k.FreeConsole()

args = sys.argv[1:]
lines, timeout = [], 120.0
while args[0] != "--":
    if args[0] == "--stdin":
        lines = [l.strip() for l in args[1].split(";") if l.strip()]
    elif args[0] == "--timeout":
        timeout = float(args[1])
    else:
        sys.exit(f"shellstart: {args[0]}?")
    args = args[2:]
program = args[1:]

sa = SECURITY_ATTRIBUTES(ctypes.sizeof(SECURITY_ATTRIBUTES), None, True)
rd, wr = w.HANDLE(), w.HANDLE()
assert k.CreatePipe(ctypes.byref(rd), ctypes.byref(wr), ctypes.byref(sa), 0)
assert k.SetHandleInformation(wr, 1, 0)  # the write end stays here

si = STARTUPINFOW()
si.cb = ctypes.sizeof(si)
si.dwFlags = 0x400
si.hStdOutput = u.MonitorFromPoint(w.POINT(0, 0), 1)
si.hStdInput = rd
pi = PROCESS_INFORMATION()
cmd = ctypes.create_unicode_buffer(subprocess.list2cmdline(program))
if not k.CreateProcessW(None, cmd, None, None, True, 0, None, None, ctypes.byref(si), ctypes.byref(pi)):
    raise ctypes.WinError(ctypes.get_last_error())
k.CloseHandle(rd)
text = "".join(l + "\n" for l in lines).encode()
n = w.DWORD()
if text:
    assert k.WriteFile(wr, text, len(text), ctypes.byref(n), None)
t0 = time.time()
if k.WaitForSingleObject(pi.hProcess, int(timeout * 1000)) != 0:
    k.TerminateProcess(pi.hProcess, 99)
    k.WaitForSingleObject(pi.hProcess, 5000)
    print(f"shellstart: stopped after {timeout:.0f} s")
k.CloseHandle(wr)
code = w.DWORD()
k.GetExitCodeProcess(pi.hProcess, ctypes.byref(code))
print(f"shellstart: flags 0x400, hStdOutput a monitor ({si.hStdOutput:#x}); pid {pi.dwProcessId} ended with {code.value} after {time.time() - t0:.1f} s")
