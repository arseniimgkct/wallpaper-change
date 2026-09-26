using System.Diagnostics;
using Microsoft.Win32;

namespace DesktopOverlay;

internal static class Reverter
{
    private const string DesktopKey = @"Control Panel\Desktop";

    internal static string RevertAll(OverlayEngine? engine, bool restoreLightTheme)
    {
        var done = new List<string>();

        engine?.Remove();
        done.Add("оверлей снят");

        if (AutoStart.IsEnabled)
        {
            AutoStart.Set(false);
            done.Add("автозапуск выключен");
        }

        if (ClearWallpaperOverride())
            done.Add("системные обои возвращены");

        if (restoreLightTheme)
        {
            if (ThemeUtil.SetLight())
                done.Add("светлая тема восстановлена");
        }

        if (DeleteSavedImage())
            done.Add("копия картинки удалена");

        Native.RedrawDesktop();

        var report = string.Join(", ", done);
        Program.Log("откат: " + report);
        return report;
    }

    private static bool ClearWallpaperOverride()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(DesktopKey, writable: false);
            var wallpaper = key?.GetValue("Wallpaper") as string;

            if (string.IsNullOrWhiteSpace(wallpaper))
                return false;

            if (wallpaper.StartsWith(@"\\", StringComparison.Ordinal))
                return false;

            using var writable = Registry.CurrentUser.CreateSubKey(DesktopKey, writable: true);
            if (writable is null)
                return false;

            foreach (var name in new[] { "Wallpaper", "WallpaperStyle", "TileWallpaper" })
                writable.DeleteValue(name, throwOnMissingValue: false);

            return true;
        }
        catch (Exception ex)
        {
            Program.Log("ошибка сброса обоев: " + ex.Message);
            return false;
        }
    }

    private static bool DeleteSavedImage()
    {
        try
        {
            if (Directory.Exists(Program.DataDir))
            {
                var removed = false;
                foreach (var file in Directory.EnumerateFiles(Program.DataDir, "wallpaper.*"))
                {
                    File.Delete(file);
                    removed = true;
                }
                return removed;
            }
        }
        catch (Exception ex)
        {
            Program.Log("ошибка удаления копии картинки: " + ex.Message);
        }

        return false;
    }
}

internal enum WindowsTheme
{
    Light,
    Dark,
}

internal static class ThemeUtil
{
    private const string Key = @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

    internal static bool IsDarkTheme() => ReadTheme() == WindowsTheme.Dark;

    internal static WindowsTheme ReadTheme()
    {
        try
        {
            using var handle = Registry.CurrentUser.OpenSubKey(Key, writable: false);

            if (TryReadFlag(handle, "AppsUseLightTheme", out var apps))
                return apps ? WindowsTheme.Light : WindowsTheme.Dark;

            if (TryReadFlag(handle, "SystemUsesLightTheme", out var system))
                return system ? WindowsTheme.Light : WindowsTheme.Dark;

            return WindowsTheme.Light;
        }
        catch (Exception ex)
        {
            Program.Log("не удалось прочитать тему Windows: " + ex.Message);
            return WindowsTheme.Light;
        }
    }

    internal static bool SetLight() => SetTheme(dark: false);

    internal static bool SetDark() => SetTheme(dark: true);

    internal static bool SetTheme(bool dark)
    {
        var target = dark ? WindowsTheme.Dark : WindowsTheme.Light;

        try
        {
            using var handle = Registry.CurrentUser.CreateSubKey(Key, writable: true);
            if (handle is null)
                return false;

            var val = dark ? 0 : 1;
            handle.SetValue("AppsUseLightTheme", val, RegistryValueKind.DWord);
            handle.SetValue("SystemUsesLightTheme", val, RegistryValueKind.DWord);

            bool applied = ReadTheme() == target;
            Program.Log($"Тема Windows: запрошена {(dark ? "тёмная" : "светлая")}, " +
                        $"подтверждена {(applied ? "да" : "нет")}");
            return applied;
        }
        catch (Exception ex)
        {
            Program.Log("ошибка переключения темы: " + ex.Message);
            return false;
        }
    }

    internal static bool ApplyAndRestartExplorer(bool dark)
    {
        bool changed = ReadTheme() != (dark ? WindowsTheme.Dark : WindowsTheme.Light);
        bool applied = SetTheme(dark);

        if (changed || !applied)
            RestartExplorer();

        return applied;
    }

    private static bool TryReadFlag(RegistryKey? key, string name, out bool isLight)
    {
        isLight = true;

        if (key is null)
            return false;

        switch (key.GetValue(name))
        {
            case int i: isLight = i != 0; return true;
            case long l: isLight = l != 0; return true;
            case byte b: isLight = b != 0; return true;
            case string s when int.TryParse(s, out var parsed): isLight = parsed != 0; return true;
            default: return false;
        }
    }

    internal static void RestartExplorer()
    {
        Program.Log("Перезапуск explorer.exe...");
        try
        {
            var processes = Process.GetProcessesByName("explorer");
            foreach (var p in processes)
            {
                try
                {
                    p.Kill();
                    p.WaitForExit(1500);
                }
                catch { }
            }
        }
        catch (Exception ex)
        {
            Program.Log("ошибка при завершении explorer: " + ex.Message);
        }

        try
        {
            var windir = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
            var explorerPath = Path.Combine(windir, "explorer.exe");
            Process.Start(new ProcessStartInfo
            {
                FileName = File.Exists(explorerPath) ? explorerPath : "explorer.exe",
                UseShellExecute = true
            });
        }
        catch (Exception ex)
        {
            Program.Log("ошибка при запуске explorer: " + ex.Message);
        }
    }
}
