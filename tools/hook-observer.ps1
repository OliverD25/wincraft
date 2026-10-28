# Watches the keyboard with a low-level hook of its own and writes what it
# sees to a file, so a key that WinCraft's hook never received can be told
# from a key that no hook received. It only looks: it never keeps a key
# from Windows or from WinCraft.
#
# Run it, do the test (Win+1, let go, Win+2), and wait: it stops by itself
# after -Seconds (60 by default) and prints the file's path.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\hook-observer.ps1
#
# One line per event, for the Windows keys, the digits and WinCraft's mask
# key (0xE8): time, key, down/up, the hook flags (0x10 = injected by a
# program), the extra info, and whether a Windows key is held.
param([int]$Seconds = 60)

$source = @'
using System;
using System.IO;
using System.Runtime.InteropServices;

public static class HookObserver
{
    delegate IntPtr HookProc(int code, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr SetWindowsHookEx(int id, HookProc proc, IntPtr module, uint thread);
    [DllImport("user32.dll")]
    static extern bool UnhookWindowsHookEx(IntPtr hook);
    [DllImport("user32.dll")]
    static extern IntPtr CallNextHookEx(IntPtr hook, int code, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")]
    static extern short GetAsyncKeyState(int vk);
    [DllImport("kernel32.dll")]
    static extern IntPtr GetModuleHandle(string name);
    [DllImport("user32.dll")]
    static extern bool PeekMessage(out MSG msg, IntPtr hwnd, uint min, uint max, uint remove);
    [DllImport("user32.dll")]
    static extern bool TranslateMessage(ref MSG msg);
    [DllImport("user32.dll")]
    static extern IntPtr DispatchMessage(ref MSG msg);
    [DllImport("user32.dll")]
    static extern uint MsgWaitForMultipleObjects(uint count, IntPtr[] handles, bool all, uint ms, uint mask);

    [StructLayout(LayoutKind.Sequential)]
    struct MSG { public IntPtr hwnd; public uint message; public UIntPtr wParam; public IntPtr lParam; public uint time; public int x; public int y; }

    [StructLayout(LayoutKind.Sequential)]
    struct KBDLLHOOKSTRUCT { public uint vkCode; public uint scanCode; public uint flags; public uint time; public UIntPtr extra; }

    static HookProc keepAlive;
    static IntPtr hook;
    static StreamWriter log;

    public static int Run(string path, int seconds)
    {
        log = new StreamWriter(path, true);
        log.AutoFlush = true;
        keepAlive = Proc;
        hook = SetWindowsHookEx(13, keepAlive, GetModuleHandle(null), 0);
        log.WriteLine(Stamp() + " observer hook " + (hook == IntPtr.Zero ? "FAILED, error " + Marshal.GetLastWin32Error() : "in"));
        if (hook == IntPtr.Zero) { log.Close(); return 1; }
        DateTime end = DateTime.Now.AddSeconds(seconds);
        IntPtr[] none = new IntPtr[0];
        while (DateTime.Now < end)
        {
            MsgWaitForMultipleObjects(0, none, false, 100, 0x4FF);
            MSG msg;
            while (PeekMessage(out msg, IntPtr.Zero, 0, 0, 1))
            {
                TranslateMessage(ref msg);
                DispatchMessage(ref msg);
            }
        }
        UnhookWindowsHookEx(hook);
        log.WriteLine(Stamp() + " observer hook out");
        log.Close();
        return 0;
    }

    static string Stamp() { return DateTime.Now.ToString("HH:mm:ss.fff"); }

    static IntPtr Proc(int code, IntPtr wParam, IntPtr lParam)
    {
        if (code >= 0)
        {
            KBDLLHOOKSTRUCT k = (KBDLLHOOKSTRUCT)Marshal.PtrToStructure(lParam, typeof(KBDLLHOOKSTRUCT));
            uint vk = k.vkCode;
            bool interesting = vk == 0x5B || vk == 0x5C || (vk >= 0x30 && vk <= 0x39) || vk == 0xE8;
            if (interesting)
            {
                int msg = (int)wParam;
                bool down = msg == 0x100 || msg == 0x104;
                bool win = (GetAsyncKeyState(0x5B) & 0x8000) != 0 || (GetAsyncKeyState(0x5C) & 0x8000) != 0;
                log.WriteLine(string.Format("{0} key 0x{1:x2} {2} flags 0x{3:x} extra 0x{4:x} win-held {5}",
                    Stamp(), vk, down ? "down" : "up", k.flags, (ulong)k.extra, win));
            }
        }
        else
        {
            log.WriteLine(Stamp() + " hook called with code " + code);
        }
        return CallNextHookEx(hook, code, wParam, lParam);
    }
}
'@

Add-Type -TypeDefinition $source -Language CSharp
$dir = Join-Path $env:LOCALAPPDATA "WinCraft"
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
$path = Join-Path $dir ("hook-observer-" + (Get-Date -Format "yyyyMMdd-HHmmss") + ".txt")
Write-Host "Observer on for $Seconds seconds. Do the test now: Win+1, let go, Win+2."
$code = [HookObserver]::Run($path, $Seconds)
Write-Host "Observer off. The file: $path"
exit $code
