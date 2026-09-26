using System.Runtime.InteropServices;
using System.Text;

namespace DesktopOverlay;

internal delegate IntPtr WndProcDelegate(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

internal static class Native
{
    internal const int GWL_STYLE = -16;

    internal const int WS_CHILD = 0x40000000;
    internal const int WS_POPUP = unchecked((int)0x80000000);
    internal const int WS_VISIBLE = 0x10000000;

    internal const int WS_EX_TRANSPARENT = 0x00000020;
    internal const int WS_EX_TOOLWINDOW = 0x00000080;
    internal const int WS_EX_NOACTIVATE = 0x08000000;

    internal static readonly IntPtr HWND_BOTTOM = new(1);

    internal const uint SWP_NOACTIVATE = 0x0010;
    internal const uint SWP_SHOWWINDOW = 0x0040;

    internal const uint WM_DESTROY = 0x0002;
    internal const uint WM_NCHITTEST = 0x0084;
    internal const uint WM_PAINT = 0x000F;
    internal const uint WM_ERASEBKGND = 0x0014;
    internal const uint WM_CLOSE = 0x0010;
    internal const uint WM_DISPLAYCHANGE = 0x007E;
    internal const uint WM_NCDESTROY = 0x0082;

    internal const int CS_HREDRAW = 0x0002;
    internal const int CS_VREDRAW = 0x0001;

    internal const int RDW_INVALIDATE = 0x0001;
    internal const int RDW_UPDATENOW = 0x0100;
    internal const int RDW_ERASE = 0x0004;
    internal const int RDW_ALLCHILDREN = 0x0080;
    internal const int RDW_FRAME = 0x0400;

    /// <summary>Не документированное сообщение оболочки: расщепляет Progman на два WorkerW.</summary>
    internal const uint MSG_SPLIT_WORKERW = 0x052C;

    [StructLayout(LayoutKind.Sequential)]
    internal struct RECT
    {
        internal int Left, Top, Right, Bottom;
        internal int Width => Right - Left;
        internal int Height => Bottom - Top;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct POINT
    {
        internal int X, Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct PAINTSTRUCT
    {
        internal IntPtr hdc;
        [MarshalAs(UnmanagedType.Bool)] internal bool fErase;
        internal RECT rcPaint;
        [MarshalAs(UnmanagedType.Bool)] internal bool fRestore;
        [MarshalAs(UnmanagedType.Bool)] internal bool fIncUpdate;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)] internal byte[] rgbReserved;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    internal struct WNDCLASSEX
    {
        internal uint cbSize;
        internal uint style;
        [MarshalAs(UnmanagedType.FunctionPtr)] internal WndProcDelegate lpfnWndProc;
        internal int cbClsExtra;
        internal int cbWndExtra;
        internal IntPtr hInstance;
        internal IntPtr hIcon;
        internal IntPtr hCursor;
        internal IntPtr hbrBackground;
        [MarshalAs(UnmanagedType.LPWStr)] internal string? lpszMenuName;
        [MarshalAs(UnmanagedType.LPWStr)] internal string lpszClassName;
        internal IntPtr hIconSm;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern IntPtr FindWindow(string? lpClassName, string? lpWindowName);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern IntPtr FindWindowEx(IntPtr hwndParent, IntPtr hwndChildAfter, string? lpszClass, string? lpszWindow);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

    internal delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true)]
    internal static extern IntPtr SendMessageTimeout(
        IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam,
        uint fuFlags, uint uTimeout, out IntPtr lpdwResult);

    [DllImport("user32.dll", SetLastError = true)]
    internal static extern IntPtr SetParent(IntPtr hWndChild, IntPtr hWndNewParent);

    [DllImport("user32.dll", SetLastError = true)]
    internal static extern IntPtr GetParent(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool IsWindow(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool SetWindowPos(
        IntPtr hWnd, IntPtr hWndInsertAfter, int X, int Y, int cx, int cy, uint uFlags);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool ClientToScreen(IntPtr hWnd, ref POINT lpPoint);

    /// <summary>
    /// Пересчитывает координаты между окнами. При hWndFrom = NULL точки трактуются
    /// как экранные. Надёжнее ручного «screen минус ClientToScreen», особенно когда
    /// у хоста есть неклиентская область.
    /// </summary>
    [DllImport("user32.dll", SetLastError = true)]
    internal static extern int MapWindowPoints(IntPtr hWndFrom, IntPtr hWndTo, ref POINT lpPoint, uint cPoints);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern ushort RegisterClassEx(ref WNDCLASSEX lpwcx);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern IntPtr CreateWindowEx(
        int dwExStyle, string? lpClassName, string? lpWindowName, uint dwStyle,
        int x, int y, int nWidth, int nHeight, IntPtr hWndParent, IntPtr hMenu,
        IntPtr hInstance, IntPtr lpParam);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool DestroyWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    internal static extern IntPtr DefWindowProc(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    internal static extern IntPtr BeginPaint(IntPtr hWnd, ref PAINTSTRUCT lpPaint);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool EndPaint(IntPtr hWnd, ref PAINTSTRUCT lpPaint);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool RedrawWindow(IntPtr hWnd, IntPtr lprc, IntPtr hrgn, uint flags);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool UpdateWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    internal static extern IntPtr GetDC(IntPtr hWnd);

    [DllImport("user32.dll")]
    internal static extern int ReleaseDC(IntPtr hWnd, IntPtr hDC);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool InvalidateRect(IntPtr hWnd, IntPtr lpRect, [MarshalAs(UnmanagedType.Bool)] bool bErase);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool ValidateRect(IntPtr hWnd, IntPtr lpRect);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern IntPtr GetModuleHandle(string? lpModuleName);

    /// <summary>
    /// Ищет окно WorkerW, в которое надо вклеить оверлей.
    ///
    /// Важный нюанс, на котором спотыкаются почти все самодельные реализации:
    ///целиться нужно НЕ в тот WorkerW, где живут иконки (SHELLDLL_DefView), а в
    /// соседний, расположенный НИЖЕ него по Z-order. Именно так делают Lively,
    /// Wallpaper Engine и WallPop. WorkerW с иконками прозрачен, поэтому наш контент
    /// виден сквозь него, а иконки остаются сверху и не требуют перерисовки.
    ///
    /// Если сделать родителем WorkerW с самими иконками, список SysListView32 окажется
    /// над нами и начнёт затирать картинку своим фоном.
    /// </summary>
    internal static IntPtr FindWallpaperWorkerW()
    {
        IntPtr progman = FindWindow("Progman", null);
        if (progman == IntPtr.Zero)
            progman = FindTopLevelByClass("Progman");

        // Просим оболочку создать второй WorkerW, если его ещё нет.
        if (progman != IntPtr.Zero)
            SendMessageTimeout(progman, MSG_SPLIT_WORKERW, IntPtr.Zero, IntPtr.Zero, 0, 1000, out _);

        // EnumWindows идёт сверху вниз по Z-order — это тот порядок, который нужен.
        var workers = new List<IntPtr>();
        EnumWindows((hwnd, _) =>
        {
            if (GetClassNameOf(hwnd) == "WorkerW")
                workers.Add(hwnd);
            return true;
        }, IntPtr.Zero);

        int iconsIndex = workers.FindIndex(w =>
            FindWindowEx(w, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero);

        if (iconsIndex < 0)
            return IntPtr.Zero;

        // Следующий WorkerW в списке = тот, что ниже слоя иконок.
        if (iconsIndex + 1 < workers.Count)
            return workers[iconsIndex + 1];

        // Запасной вариант: WorkerW под слоем иконок не нашлось — берём Progman,
        // наш слой ляжет в самый низ его дочерних окон, под SHELLDLL_DefView.
        return progman;
    }

    /// <summary>
    /// Принудительно перерисовывает весь рабочий стол.
    ///
    /// Нужна после снятия оверлея: оболочка не считает нужным трогать область, которую
    /// занимало наше окно, и на экране остаются залипшие пиксели картинки — вплоть до
    /// перезапуска проводника. RedrawWindow с RDW_ERASE по Progman и всем WorkerW
    /// заставляет оболочку заново нарисовать системные обои.
    /// </summary>
    internal static void RedrawDesktop()
    {
        uint flags = RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_FRAME;

        if (FindTopLevelByClass("Progman") is { } progman && progman != IntPtr.Zero)
            RedrawWindow(progman, IntPtr.Zero, IntPtr.Zero, flags);

        EnumWindows((hwnd, _) =>
        {
            if (GetClassNameOf(hwnd) == "WorkerW")
                RedrawWindow(hwnd, IntPtr.Zero, IntPtr.Zero, flags);
            return true;
        }, IntPtr.Zero);
    }

    internal static IntPtr FindTopLevelByClass(string className)
    {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hwnd, _) =>
        {
            if (GetClassNameOf(hwnd) != className)
                return true;
            found = hwnd;
            return false;
        }, IntPtr.Zero);
        return found;
    }

    internal static string GetClassNameOf(IntPtr hwnd)
    {
        var sb = new StringBuilder(128);
        return GetClassName(hwnd, sb, sb.Capacity) > 0 ? sb.ToString() : string.Empty;
    }

    [DllImport("dwmapi.dll", PreserveSig = true)]
    internal static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int attrValue, int attrSize);

    internal static void EnableDarkModeForWindow(IntPtr hwnd)
    {
        if (hwnd == IntPtr.Zero)
            return;

        int trueValue = 1;
        // 20 = DWMWA_USE_IMMERSIVE_DARK_MODE in modern Windows 10/11
        DwmSetWindowAttribute(hwnd, 20, ref trueValue, sizeof(int));
        // 19 = DWMWA_USE_IMMERSIVE_DARK_MODE in earlier Windows 10 builds
        DwmSetWindowAttribute(hwnd, 19, ref trueValue, sizeof(int));
    }
}
