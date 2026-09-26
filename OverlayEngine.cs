using Microsoft.Win32;

namespace DesktopOverlay;

/// <summary>
/// Держит оверлей включённым: окно на WorkerW, watchdog, периодическую
/// перерисовку и значок в трее. Умеет включаться и выключаться, чтобы
/// окно утилиты могло применять и откатывать обои.
/// </summary>
internal sealed class OverlayEngine : IDisposable
{
    private Image? _image;
    private NotifyIcon? _tray;
    private System.Windows.Forms.Timer? _watchdog;
    private System.Windows.Forms.Timer? _repaint;
    private OverlayWindow? _window;

    /// <summary>Оверлей сейчас показан на рабочем столе.</summary>
    internal bool IsApplied { get; private set; }

    /// <summary>Пользователь выбрал в трее «Показать окно утилиты».</summary>
    internal Action? ShowRequested { get; set; }

    /// <summary>Пользователь выбрал в трее «Выход».</summary>
    internal Action? ExitRequested { get; set; }

    /// <summary>Пользователь выбрал в трее «Откатить обои».</summary>
    internal Action? RevertRequested { get; set; }

    /// <summary>Вызывается, когда оверлей упал и не смог восстановиться.</summary>
    internal Action<string>? StatusChanged { get; set; }

    internal bool Apply(Image image, bool tray)
    {
        Remove();

        _image = image;
        IsApplied = true;

        if (tray)
            _tray = BuildTray();

        SystemEvents.DisplaySettingsChanged += OnDisplaySettingsChanged;

        _watchdog = new System.Windows.Forms.Timer { Interval = 2000 };
        _watchdog.Tick += (_, _) => EnsureOverlay();
        _watchdog.Start();

        // Оболочка периодически перерисовывает свои обои поверх рабочего стола и
        // затирает наше окно, не присылая WM_PAINT. Поэтому перерисовываем картинку
        // сами раз в секунду: картинка кэшируется и переносится одним
        // DrawImageUnscaled, так что это почти бесплатно. Без этого картинка
        // мигает и пропадает.
        _repaint = new System.Windows.Forms.Timer { Interval = 1000 };
        _repaint.Tick += (_, _) => _window?.ForceRepaint();
        _repaint.Start();

        EnsureOverlay();
        return IsHealthy();
    }

    /// <summary>Снимает оверлей: рабочий стол возвращается к обычным обоям.</summary>
    internal void Remove()
    {
        IsApplied = false;

        SystemEvents.DisplaySettingsChanged -= OnDisplaySettingsChanged;

        if (_watchdog is not null) { _watchdog.Stop(); _watchdog.Dispose(); _watchdog = null; }
        if (_repaint is not null) { _repaint.Stop(); _repaint.Dispose(); _repaint = null; }

        // Сначала заливаем окно настоящими системными обоями: после DestroyWindow
        // оболочка не перерисовывает занятую нами область, и на экране остаются
        // залипшие пиксели картинки. Пока окно живо, безопаснее нарисовать в нём то,
        // что должно быть под ним.
        if (_window is { } alive && alive.IsAlive && alive.ShowSystemWallpaper())
        {
            Thread.Sleep(150);
            Application.DoEvents();
        }

        DestroyWindow();
        DestroyTray();

        Native.RedrawDesktop();

        _image?.Dispose();
        _image = null;
    }

    internal bool IsHealthy() => _window is { } w && w.IsStillAttached();

    private NotifyIcon BuildTray()
    {
        var icon = Program.LoadAppIcon();
        var menu = new ContextMenuStrip();

        var show = new ToolStripMenuItem("Показать окно утилиты");
        show.Font = new Font(menu.Font, FontStyle.Bold);
        show.Click += (_, _) => ShowRequested?.Invoke();

        var reattach = new ToolStripMenuItem("Переприкрепить обои");
        reattach.Click += (_, _) => EnsureOverlay(force: true);

        var revert = new ToolStripMenuItem("Откатить обои");
        revert.Click += (_, _) =>
        {
            if (RevertRequested is not null)
                RevertRequested();
            else
                Remove();
        };

        var exit = new ToolStripMenuItem("Выход");
        exit.Click += (_, _) => ExitRequested?.Invoke();

        menu.Items.AddRange(new ToolStripItem[] { show, reattach, revert, new ToolStripSeparator(), exit });

        return new NotifyIcon
        {
            Icon = icon,
            Text = Program.AppName,
            Visible = true,
            ContextMenuStrip = menu,
        };
    }

    private void DestroyTray()
    {
        if (_tray is null)
            return;

        _tray.Visible = false;
        _tray.ContextMenuStrip?.Dispose();
        _tray.Dispose();
        _tray = null;
    }

    private void DestroyWindow()
    {
        try
        {
            _window?.Dispose();
        }
        catch (Exception ex)
        {
            Program.Log("ошибка при закрытии окна оверлея: " + ex.Message);
        }
        _window = null;
    }

    /// <summary>
    /// Точка сердца: если окно пропало (перезапуск Explorer) или отцепилось —
    /// пересоздаём и прикрепляем заново.
    /// </summary>
    private void EnsureOverlay(bool force = false)
    {
        if (!IsApplied || _image is null)
            return;

        if (!force && _window is { } w && w.IsStillAttached())
        {
            if (NeedsRelayout(w))
                w.LayoutToPrimaryMonitor();
            return;
        }

        Recreate(reason: force
            ? "принудительно"
            : _window is null ? "первичный запуск" : "окно потеряно (перезапуск Explorer?)");
    }

    private static bool NeedsRelayout(OverlayWindow w)
    {
        if (!Native.GetWindowRect(w.Hwnd, out var r))
            return false;

        var screen = Screen.PrimaryScreen!.Bounds;
        return r.Width != screen.Width || r.Height != screen.Height ||
               r.Left != screen.Left || r.Top != screen.Top;
    }

    private void Recreate(string reason)
    {
        if (_image is null)
            return;

        Program.Log("пересоздание оверлея: " + reason);
        DestroyWindow();

        var window = new OverlayWindow(_image);
        if (window.Attach())
        {
            _window = window;
            Program.Log("прикреплено: hwnd=" + window.Hwnd + " host=" + window.Host);
        }
        else
        {
            window.Dispose();
            Program.Log("не удалось прикрепиться");
            StatusChanged?.Invoke("Не удалось встроить окно в рабочий стол");
        }
    }

    private void OnDisplaySettingsChanged(object? sender, EventArgs e)
    {
        _window?.LayoutToPrimaryMonitor();
        _window?.ForceRepaint();
    }

    public void Dispose()
    {
        Remove();
        GC.SuppressFinalize(this);
    }
}
