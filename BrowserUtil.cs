using System.Diagnostics;
using Microsoft.Win32;

namespace DesktopOverlay;

internal enum BrowserKind
{
    Chrome,
    Firefox,
    Edge,
}

internal sealed class BrowserInfo
{
    internal BrowserInfo(BrowserKind kind, string title, string exePath, string urlProgId, string htmlProgId)
    {
        Kind = kind;
        Title = title;
        ExePath = exePath;
        UrlProgId = urlProgId;
        HtmlProgId = htmlProgId;
    }

    internal BrowserKind Kind { get; }

    internal string Title { get; }

    internal string ExePath { get; }

    internal string UrlProgId { get; }

    internal string HtmlProgId { get; }
}

/// <summary>
/// Определение установленных браузеров и текущего браузера по умолчанию.
///
/// Смена самого браузера по умолчанию намеренно не делается записью в реестр:
/// Windows защищает ветки UserChoice и отклоняет прямую запись ProgId/Hash
/// даже для владельца ключа. Рабочий путь — сам браузер или диалог Параметров.
/// </summary>
internal static class BrowserUtil
{
    private const string UrlAssociationsKey =
        @"Software\Microsoft\Windows\Shell\Associations\UrlAssociations";

    private const string FileExtsKey =
        @"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

    private static readonly string[] Protocols = { "http", "https" };
    private static readonly string[] WebExtensions = { ".htm", ".html" };

    private static string[]? _classNames;

    internal static string TitleOf(BrowserKind kind) => kind switch
    {
        BrowserKind.Chrome => "Google Chrome",
        BrowserKind.Firefox => "Mozilla Firefox",
        _ => "Microsoft Edge",
    };

    /// <summary>Информация о браузере либо null, если он не установлен.</summary>
    internal static BrowserInfo? Detect(BrowserKind kind)
    {
        string? exePath = FindAppPath(ExeName(kind));
        if (exePath is null)
            return null;

        string? urlProgId = ResolveProgId(UrlProgIdPrefix(kind));
        string? htmlProgId = ResolveProgId(HtmlProgIdPrefix(kind));

        return new BrowserInfo(
            kind,
            TitleOf(kind),
            exePath,
            urlProgId ?? $"{UrlProgIdPrefix(kind)}.x",
            htmlProgId ?? $"{HtmlProgIdPrefix(kind)}.x");
    }

    /// <summary>Chrome и Firefox, которые реально установлены.</summary>
    internal static List<BrowserInfo> DetectAlternatives()
    {
        var list = new List<BrowserInfo>();
        foreach (var kind in new[] { BrowserKind.Chrome, BrowserKind.Firefox })
        {
            var info = Detect(kind);
            if (info is not null)
                list.Add(info);
        }
        return list;
    }

    internal static string CurrentTitle()
    {
        var kind = CurrentKind();
        return kind is null ? "неизвестен" : TitleOf(kind.Value);
    }

    internal static BrowserKind? CurrentKind()
    {
        string? progId = ReadProgId(UrlAssociationsKey + @"\http\UserChoice");
        if (string.IsNullOrWhiteSpace(progId))
            progId = ReadProgId(UrlAssociationsKey + @"\https\UserChoice");

        if (string.IsNullOrWhiteSpace(progId))
            progId = ReadProgId(FileExtsKey + @"\.htm\UserChoice");

        if (string.IsNullOrWhiteSpace(progId))
            return null;

        // Достаточно сравнения по началу ProgId — так не нужно обходить все ветки реестра.
        if (progId.StartsWith("ChromeHTML", StringComparison.OrdinalIgnoreCase))
            return BrowserKind.Chrome;
        if (progId.StartsWith("FirefoxURL", StringComparison.OrdinalIgnoreCase) ||
            progId.StartsWith("FirefoxHTML", StringComparison.OrdinalIgnoreCase))
            return BrowserKind.Firefox;
        if (progId.StartsWith("MSEdge", StringComparison.OrdinalIgnoreCase))
            return BrowserKind.Edge;

        return null;
    }

    /// <summary>
    /// Передаёт выбранному браузеру http, https, .htm и .html.
    /// Firefox умеет назначить себя сам, Chrome — только через Параметры,
    /// поэтому в последнем случае открывается нужная страница настроек.
    /// </summary>
    internal static bool MakeDefault(BrowserInfo browser, out string report)
    {
        if (!LaunchAsDefault(browser))
        {
            report = $"Windows не дал назначить браузер без вашего участия. " +
                     $"Откройте «Параметры → Приложения → Веб-браузер по умолчанию» и выберите {browser.Title}.";
            Program.Log($"Браузер по умолчанию: {browser.Title} не смог назначить себя сам, нужны Параметры");
            return false;
        }

        // Проверяем результат: браузер мог не успеть или не суметь.
        for (int i = 0; i < 10 && CurrentKind() != browser.Kind; i++)
            Thread.Sleep(300);

        bool done = CurrentKind() == browser.Kind;

        Program.Log($"Браузер по умолчанию: запрошен {browser.Title}, " +
                    $"подтверждено {(done ? "да" : "нет")}");

        if (done)
        {
            report = $"{browser.Title} теперь браузер по умолчанию (http, https, .htm, .html).";
            return true;
        }

        report = $"Не удалось подтвердить смену браузера на {browser.Title}. " +
                 $"Проверьте «Параметры → Приложения → Веб-браузер по умолчанию».";
        return false;
    }

    private static bool LaunchAsDefault(BrowserInfo browser)
    {
        try
        {
            // Firefox обрабатывает флаг и прописывает себя сам.
            if (browser.Kind == BrowserKind.Firefox)
            {
                Start(browser.ExePath, "-setDefaultBrowser");
                return true;
            }

            // Ни Chrome, ни Edge больше не назначают себя из командной строки,
            // поэтому открываем их страницу настроек и ждём подтверждения от Параметров.
            Start(browser.ExePath, browser.Kind == BrowserKind.Chrome
                ? "chrome://settings/defaultBrowser"
                : "edge://settings/defaultBrowser");

            // Параметры всё равно открываются, но ждать подтверждения здесь бессмысленно.
            return false;
        }
        catch (Exception ex)
        {
            Program.Log($"не удалось запустить {browser.Title}: {ex.Message}");
            return false;
        }
    }

    private static void Start(string fileName, string arguments)
    {
        Process.Start(new ProcessStartInfo
        {
            FileName = fileName,
            Arguments = arguments,
            UseShellExecute = true,
        });
    }

    private static string ExeName(BrowserKind kind) => kind switch
    {
        BrowserKind.Chrome => "chrome.exe",
        BrowserKind.Firefox => "firefox.exe",
        _ => "msedge.exe",
    };

    private static string UrlProgIdPrefix(BrowserKind kind) => kind switch
    {
        BrowserKind.Chrome => "ChromeHTML",
        BrowserKind.Firefox => "FirefoxURL",
        _ => "MSEdgeHTM",
    };

    private static string HtmlProgIdPrefix(BrowserKind kind) => kind switch
    {
        BrowserKind.Chrome => "ChromeHTML",
        BrowserKind.Firefox => "FirefoxHTML",
        _ => "MSEdgeHTM",
    };

    /// <summary>Ищет ProgId обработчика: сначала точный, затем зарегистрированный с суффиксом.</summary>
    private static string? ResolveProgId(string prefix)
    {
        if (IsHandler(prefix))
            return prefix;

        foreach (string name in ClassNames())
        {
            if (name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase) && IsHandler(name))
                return name;
        }

        return null;
    }

    private static string[] ClassNames()
    {
        if (_classNames is not null)
            return _classNames;

        try
        {
            _classNames = Registry.ClassesRoot.GetSubKeyNames();
        }
        catch (Exception ex)
        {
            Program.Log("ошибка чтения списка классов: " + ex.Message);
            _classNames = Array.Empty<string>();
        }

        return _classNames;
    }

    private static bool IsHandler(string progId)
    {
        try
        {
            using var command = Registry.ClassesRoot.OpenSubKey(progId + @"\shell\open\command");
            return command is not null;
        }
        catch
        {
            return false;
        }
    }

    private static string? ReadProgId(string keyPath)
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(keyPath, writable: false);
            return key?.GetValue("ProgId") as string;
        }
        catch (Exception ex)
        {
            Program.Log("ошибка чтения UserChoice: " + ex.Message);
            return null;
        }
    }

    private static string? FindAppPath(string exeName)
    {
        string relative = $@"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exeName}";

        foreach (var hive in new[] { RegistryHive.LocalMachine, RegistryHive.CurrentUser })
        {
            foreach (var view in new[] { RegistryView.Registry64, RegistryView.Registry32 })
            {
                try
                {
                    using var baseKey = RegistryKey.OpenBaseKey(hive, view);
                    using var key = baseKey.OpenSubKey(relative);

                    if (key?.GetValue(null) is string path && File.Exists(path))
                        return path;
                }
                catch (Exception ex)
                {
                    Program.Log($"ошибка чтения App Paths для {exeName}: {ex.Message}");
                }
            }
        }

        return null;
    }
}