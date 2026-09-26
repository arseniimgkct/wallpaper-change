using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace DesktopOverlay;

/// <summary>
/// Окно-оверлей поверх рабочего стола. Создаётся сразу как дочернее окно (WS_CHILD)
/// хоста WorkerW, а не «показывается, а потом перевклеивается» — так надёжнее.
/// Благодаря тому, что родитель — не top-level окно, панель задач всегда остаётся поверх.
/// </summary>
internal sealed class OverlayWindow : IDisposable
{
    private const string ClassName = "DesktopOverlaySurface";

    // Делегат обязан жить всё время работы окна, иначе GC соберёт callback.
    private static readonly WndProcDelegate WndProc = OnWindowProc;
    private static OverlayWindow? _current;

    private readonly Image _image;
    private readonly bool _debugFill;
    private Bitmap? _scaled;
    private Image? _scaledSource;
    private Image? _systemWallpaper;
    private bool _showSystemWallpaper;

    /// <summary>
    /// WS_EX_TRANSPARENT нужен для проскока кликов, но вместе с ним окно
    /// исключается из композиции DWM на некоторых сборках — картинка перестаёт
    /// отображаться. Поэтому его можно отключить (OVERLAY_NO_EX_TRANSPARENT=1),
    /// а проскок кликов обеспечить через WM_NCHITTEST -> HTTRANSPARENT.
    /// </summary>
    private readonly int _exStyle;

    private IntPtr _host;

    internal OverlayWindow(Image image)
    {
        _image = image;
        _debugFill = Environment.GetEnvironmentVariable("OVERLAY_DEBUG_FILL") == "1";
        _exStyle = Native.WS_EX_NOACTIVATE | Native.WS_EX_TOOLWINDOW;

        if (Environment.GetEnvironmentVariable("OVERLAY_NO_EX_TRANSPARENT") != "1")
            _exStyle |= Native.WS_EX_TRANSPARENT;

        EnsureClassRegistered();
    }

    internal IntPtr Hwnd { get; private set; }

    internal IntPtr Host => _host;

    internal bool IsAlive => Hwnd != IntPtr.Zero && Native.IsWindow(Hwnd);

    internal bool IsStillAttached() =>
        IsAlive &&
        _host != IntPtr.Zero &&
        Native.IsWindow(_host) &&
        Native.GetParent(Hwnd) == _host;

    /// <summary>
    /// Создаёт окно и вклеивает его в WorkerW, который лежит НИЖЕ слоя иконок.
    /// Перерисовывать иконки вручную больше не нужно: слой с ними выше нас и прозрачен,
    /// поэтому оболочка рисует их поверх нашего окна штатным образом.
    /// </summary>
    internal bool Attach()
    {
        var host = Native.FindWallpaperWorkerW();
        if (host == IntPtr.Zero || !Native.IsWindow(host))
            return false;

        var screen = Screen.PrimaryScreen!.Bounds;
        var origin = ScreenToClient(host, screen.Left, screen.Top);

        // Окно создаётся сразу дочерним и сразу уводится в самый низ Z-order,
        // чтобы оказаться под слоем иконок (SHELLDLL_DefView).
        Hwnd = Native.CreateWindowEx(
            _exStyle,
            ClassName,
            "DesktopOverlay",
            Native.WS_CHILD | Native.WS_VISIBLE,
            origin.X,
            origin.Y,
            screen.Width,
            screen.Height,
            host,
            IntPtr.Zero,
            Native.GetModuleHandle(null),
            IntPtr.Zero);

        if (Hwnd == IntPtr.Zero)
        {
            Program.Log("CreateWindowEx вернул 0, Win32 error = " + Marshal.GetLastWin32Error());
            return false;
        }

        _host = host;
        _current = this;
        LayoutToPrimaryMonitor();
        ForceRepaint();
        return true;
    }

    /// <summary>Растягивает окно на весь основной монитор с поправкой на смещение клиента хоста.</summary>
    internal void LayoutToPrimaryMonitor()
    {
        if (!IsAlive || _host == IntPtr.Zero || !Native.IsWindow(_host))
            return;

        var screen = Screen.PrimaryScreen!.Bounds;
        var origin = ScreenToClient(_host, screen.Left, screen.Top);

        Native.SetWindowPos(
            Hwnd,
            Native.HWND_BOTTOM,
            origin.X,
            origin.Y,
            screen.Width,
            screen.Height,
            Native.SWP_NOACTIVATE | Native.SWP_SHOWWINDOW);

        if (Native.GetWindowRect(Hwnd, out var wr))
            Program.Log($"размещено: client=({origin.X},{origin.Y}) {screen.Width}x{screen.Height}, экран=({wr.Left},{wr.Top}) {wr.Width}x{wr.Height}");
    }

    internal void ForceRepaint()
    {
        if (!IsAlive)
            return;

        // Инвалидируем весь прямоугольник: без этого оболочка может прислать
        // WM_PAINT с частичной областью, и часть окна останется непокрашенной.
        Native.InvalidateRect(Hwnd, IntPtr.Zero, true);
        Native.UpdateWindow(Hwnd);
    }

    public void Dispose()
    {
        if (ReferenceEquals(_current, this))
            _current = null;

        if (IsAlive)
            Native.DestroyWindow(Hwnd);

        _scaled?.Dispose();
        _scaled = null;
        _scaledSource = null;
        _systemWallpaper?.Dispose();
        _systemWallpaper = null;
        Hwnd = IntPtr.Zero;
        _host = IntPtr.Zero;
    }

    private static Native.POINT ScreenToClient(IntPtr host, int screenX, int screenY)
    {
        var p = new Native.POINT { X = screenX, Y = screenY };
        Native.MapWindowPoints(IntPtr.Zero, host, ref p, 1);
        return p;
    }

    private static void EnsureClassRegistered()
    {
        var wc = new Native.WNDCLASSEX
        {
            cbSize = (uint)Marshal.SizeOf<Native.WNDCLASSEX>(),
            style = Native.CS_HREDRAW | Native.CS_VREDRAW,
            lpfnWndProc = WndProc,
            hInstance = Native.GetModuleHandle(null),
            // hbrBackground = 0 — фон не стираем, иначе при перерисовке будет мерцание.
            hbrBackground = IntPtr.Zero,
            lpszClassName = ClassName,
        };
        Native.RegisterClassEx(ref wc);
    }

    private static IntPtr OnWindowProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        var self = _current;

        switch (msg)
        {
            case Native.WM_NCHITTEST:
                // Пропускаем клики к иконкам рабочего стола: окно не должно перехватывать ввод.
                return new IntPtr(-1); // HTTRANSPARENT

            case Native.WM_PAINT:
                self?.HandlePaint(hWnd);
                return IntPtr.Zero;

            case Native.WM_ERASEBKGND:
                // Фон полностью перекрывается картинкой — сообщаем, что стёрли сами.
                return new IntPtr(1);

            case Native.WM_DISPLAYCHANGE:
                self?.LayoutToPrimaryMonitor();
                self?.ForceRepaint();
                return IntPtr.Zero;

            case Native.WM_CLOSE:
                self?.Dispose();
                return IntPtr.Zero;

            case Native.WM_NCDESTROY:
                if (ReferenceEquals(_current, self))
                    _current = null;
                break;
        }

        return Native.DefWindowProc(hWnd, msg, wParam, lParam);
    }

    /// <summary>
    /// Рисуем строго внутри WM_PAINT через BeginPaint: DWM композитит только то,
    /// что нарисовано в контексте WM_PAINT. Рисование через GetDC после EndPaint
    /// не отображается вовсе — проверено.
    ///
    /// Область обновления при этом может прийти частичной, поэтому перед отрисовкой
    /// окно полностью инвалидируется (см. ForceRepaint) — тогда rcPaint всегда
    /// равен всему клиенту и непокрашенных полос не остаётся.
    ///
    /// Отдельная важная деталь: оболочка периодически перерисовывает поверх рабочего
    /// стола свои обои, и наше окно при этом стирается, хотя WM_PAINT нам не приходит.
    /// На экране картинка «мигает и пропадает». Лечится периодическим перерисовом
    /// из таймера (см. Program) — именно так ведут себя видео-обои, где кадры идут
    /// непрерывно. Картинка у нас кэшируется и переносится одним BitBlt, так что
    /// такая перерисовка почти ничего не стоит.
    /// </summary>
    private void HandlePaint(IntPtr hWnd)
    {
        var ps = new Native.PAINTSTRUCT();
        var hdc = Native.BeginPaint(hWnd, ref ps);
        if (hdc == IntPtr.Zero)
        {
            LogPaint("BeginPaint вернул 0");
            return;
        }

        try
        {
            Native.GetClientRect(hWnd, out var rect);

            using var g = Graphics.FromHdc(hdc);

            if (_debugFill)
            {
                g.Clear(Color.Magenta);
                LogPaint($"DEBUG magenta, client={rect.Width}x{rect.Height}");
                return;
            }

            var source = _showSystemWallpaper && _systemWallpaper is not null ? _systemWallpaper : _image;
            DrawCover(g, rect.Width, rect.Height, source);
            LogPaint($"paint client={rect.Width}x{rect.Height} rcPaint={ps.rcPaint.Left},{ps.rcPaint.Top} {ps.rcPaint.Width}x{ps.rcPaint.Height}");
        }
        catch (Exception ex)
        {
            Program.Log("ошибка отрисовки: " + ex.Message);
        }
        finally
        {
            Native.EndPaint(hWnd, ref ps);
        }
    }

    private int _paintLogCount;

    private void LogPaint(string message)
    {
        if (Interlocked.Increment(ref _paintLogCount) > 8)
            return;
        Program.Log($"[paint #{_paintLogCount}] {message}");
    }

    /// <summary>
    /// Масштабированная картинка кэшируется и при каждой перерисовке переносится
    /// одним DrawImageUnscaled — иначе перерисовка раз в секунду была бы дорогой.
    /// </summary>
    private void DrawCover(Graphics g, int width, int height, Image source)
    {
        if (width <= 0 || height <= 0)
            return;

        if (_scaled is null || _scaled.Width != width || _scaled.Height != height || _scaledSource != source)
        {
            _scaled?.Dispose();

            // Режим «cover»: заполняем окно целиком, обрезая лишнее, пропорции сохраняются.
            double scale = Math.Max((double)width / source.Width, (double)height / source.Height);
            int w = (int)Math.Round(source.Width * scale);
            int h = (int)Math.Round(source.Height * scale);

            _scaled = new Bitmap(width, height, PixelFormat.Format32bppPArgb);
            using var sg = Graphics.FromImage(_scaled);
            sg.InterpolationMode = InterpolationMode.HighQualityBicubic;
            sg.PixelOffsetMode = PixelOffsetMode.HighQuality;
            sg.CompositingQuality = CompositingQuality.HighQuality;
            sg.DrawImage(source,
                new Rectangle((width - w) / 2, (height - h) / 2, w, h),
                0, 0, source.Width, source.Height,
                GraphicsUnit.Pixel);

            _scaledSource = source;
        }

        g.CompositingMode = CompositingMode.SourceCopy;
        g.DrawImageUnscaled(_scaled, 0, 0);
    }

    /// <summary>
    /// Заливает окно настоящими системными обоями перед его уничтожением.
    ///
    /// Зачем: после DestroyWindow оболочка не перерисовывает область, которую
    /// занимало наше окно, и на экране остаются залипшие пиксели картинки — вплоть
    /// до перезапуска проводника. RedrawWindow по Progman/WorkerW/ListView не помогает.
    /// Раз окно ещё живо, проще нарисовать в нём то, что должно быть под ним.
    /// </summary>
    internal bool ShowSystemWallpaper()
    {
        if (!IsAlive)
            return false;

        if (_systemWallpaper is null)
            _systemWallpaper = LoadSystemWallpaper();

        if (_systemWallpaper is null)
        {
            Program.Log("системные обои не прочитаны, окно будет просто закрыто");
            return false;
        }

        _showSystemWallpaper = true;
        ForceRepaint();
        return true;
    }

    /// <summary>
    /// Кэшированная копия текущих системных обоев. Сначала пробуем TranscodedWallpaper
    /// (его пишет оболочка), затем путь из доменной политики.
    /// </summary>
    private static Image? LoadSystemWallpaper()
    {
        var candidates = new List<string>
        {
            Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                "Microsoft", "Windows", "Themes", "TranscodedWallpaper"),
        };

        try
        {
            using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Policies\System", writable: false);
            if (key?.GetValue("Wallpaper") is string policy && File.Exists(policy))
                candidates.Add(policy);
        }
        catch
        {
            // политика может быть недоступна — не критично
        }

        foreach (var path in candidates)
        {
            try
            {
                // TranscachedWallpaper бывает залочен оболочкой — открываем на чтение с разделением.
                using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
                using var img = Image.FromStream(stream);
                return new Bitmap(img);
            }
            catch (Exception ex)
            {
                Program.Log("не прочитаны системные обои " + path + ": " + ex.Message);
            }
        }

        return null;
    }
}
