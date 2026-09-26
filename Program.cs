namespace DesktopOverlay;

internal static class Program
{
    internal const string AppName = "DesktopOverlay";
    private const string MutexName = @"Local\DesktopOverlay.SingleInstance";

    internal static string DataDir => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), AppName);

    /// <summary>Путь к сохранённой копии картинки — её использует автозапуск.</summary>
    internal static string? SavedWallpaperPath
    {
        get
        {
            try
            {
                foreach (var ext in new[] { ".jpg", ".jpeg", ".png", ".bmp", ".gif", ".tif", ".tiff" })
                {
                    var candidate = Path.Combine(DataDir, "wallpaper" + ext);
                    if (File.Exists(candidate))
                        return candidate;
                }
            }
            catch
            {
                // для автозапуска не критично
            }
            return null;
        }
    }

    internal static void Log(string message)
    {
        try
        {
            Directory.CreateDirectory(DataDir);
            File.AppendAllText(
                Path.Combine(DataDir, "overlay.log"),
                $"{DateTime.Now:yyyy-MM-dd HH:mm:ss}  {message}{Environment.NewLine}");
        }
        catch
        {
            // логирование не должно ронять приложение
        }
    }

    private static Icon? _appIcon;

    /// <summary>Значок приложения, встроенный в exe.</summary>
    internal static Icon LoadAppIcon()
    {
        if (_appIcon is not null)
            return _appIcon;

        try
        {
            var exe = Environment.ProcessPath;
            if (!string.IsNullOrEmpty(exe))
                _appIcon = Icon.ExtractAssociatedIcon(exe);
        }
        catch
        {
            // не критично
        }

        _appIcon ??= SystemIcons.Application;
        return _appIcon;
    }

    [STAThread]
    private static void Main(string[] args)
    {
        using var mutex = new Mutex(true, MutexName, out bool isFirst);
        if (!isFirst)
        {
            Log("приложение уже запущено, второй экземпляр закрыт");
            return;
        }

        Application.SetHighDpiMode(HighDpiMode.PerMonitorV2);
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);

        Application.ThreadException += (_, e) => Log("исключение в UI: " + e.Exception);
        AppDomain.CurrentDomain.UnhandledException += (_, e) => Log("фатальное: " + e.ExceptionObject);

        // Режим автозапуска: окно не показываем, сразу поднимаем оверлей и живём в трее.
        if (args.Length >= 2 && args[0] == "--apply")
        {
            var saved = Program.LoadStableCopy(args[1]);
            if (saved is null)
                return;

            Log($"автозапуск, картинка: {args[1]}");
            Application.Run(new TrayContext(saved));
            return;
        }

        // Откат из командной строки — на случай, если окно недоступно.
        if (args.Length >= 1 && args[0] == "--revert")
        {
            // --revert --light — заодно вернуть светлую тему.
            var light = args.Length >= 2 && args[1] == "--light";
            var report = Reverter.RevertAll(engine: null, restoreLightTheme: light);
            Log("откат из командной строки: " + report);
            return;
        }

        Log("запуск окна утилиты");

        // --set "путь" — открыть окно с уже выбранной картинкой и сразу применить.
        var preselect = null as string;
        var autoApply = false;
        if (args.Length >= 2 && args[0] == "--set")
        {
            preselect = args[1];
            autoApply = true;
        }

        Application.Run(new MainForm(preselect, autoApply));
        Log("остановка");
    }

    /// <summary>
    /// Копирует картинку в %APPDATA%, чтобы оверлей не сломался после очистки «Загрузок».
    /// Возвращает полностью перерисованный Bitmap — исходный файл закрываем.
    /// </summary>
    internal static Image? LoadStableCopy(string source)
    {
        try
        {
            Directory.CreateDirectory(DataDir);
            var ext = Path.GetExtension(source).ToLowerInvariant();
            var target = Path.Combine(DataDir, "wallpaper" + ext);

            var needCopy = true;
            if (File.Exists(target))
            {
                needCopy = new FileInfo(target).Length != new FileInfo(source).Length ||
                           File.GetLastWriteTimeUtc(target) < File.GetLastWriteTimeUtc(source);
            }

            if (needCopy)
                File.Copy(source, target, true);

            using var img = Image.FromFile(target);
            return new Bitmap(img);
        }
        catch (Exception ex)
        {
            Log("не удалось загрузить картинку: " + ex.Message);
            MessageBox.Show(
                "Не удалось загрузить изображение:\n" + ex.Message,
                AppName, MessageBoxButtons.OK, MessageBoxIcon.Error);
            return null;
        }
    }
}

/// <summary>
/// Безоконный режим для автозапуска: оверлей + трей, форма не показывается.
/// </summary>
internal sealed class TrayContext : ApplicationContext
{
    private readonly OverlayEngine _engine = new();

    internal TrayContext(Image image)
    {
        _engine.ExitRequested += () => ExitThread();

        // Окна в этом режиме нет: «откатить» значит снять оверлей, вернуть
        // настройки и выключить автозапуск, иначе картинка вернётся при входе.
        _engine.RevertRequested += () =>
        {
            Reverter.RevertAll(_engine, restoreLightTheme: false);
            ExitThread();
        };

        _engine.Apply(image, tray: true);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _engine.Dispose();
        base.Dispose(disposing);
    }
}
