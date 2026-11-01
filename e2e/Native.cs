using System;
using System.Runtime.InteropServices;

public class E2E
{
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr h);

    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern void mouse_event(uint f, uint dx, uint dy, int d, IntPtr e);
}
