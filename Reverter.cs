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
            ThemeUtil.SetLight();
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

internal static class ThemeUtil
{
    private const string Key = @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

    internal static bool IsDarkTheme()
    {
        try
        {
            using var handle = Registry.CurrentUser.OpenSubKey(Key, writable: false);
            var appTheme = handle?.GetValue("AppsUseLightTheme");
            var sysTheme = handle?.GetValue("SystemUsesLightTheme");

            if (appTheme is int app && app == 0) return true;
            if (sysTheme is int sys && sys == 0) return true;
            return false;
        }
        catch
        {
            return false;
        }
    }

    internal static void SetLight() => SetTheme(dark: false);

    internal static void SetDark() => SetTheme(dark: true);

    internal static void SetTheme(bool dark)
    {
        try
        {
            using var handle = Registry.CurrentUser.CreateSubKey(Key, writable: true);
            var val = dark ? 0 : 1;
            handle?.SetValue("AppsUseLightTheme", val, RegistryValueKind.DWord);
            handle?.SetValue("SystemUsesLightTheme", val, RegistryValueKind.DWord);
            Program.Log($"Тема Windows переключена на: {(dark ? "Тёмная" : "Светлая")}");
        }
        catch (Exception ex)
        {
            Program.Log("ошибка переключения темы: " + ex.Message);
        }
    }

    internal static bool ToggleThemeAndRestartExplorer()
    {
        bool currentIsDark = IsDarkTheme();
        bool newDark = !currentIsDark;
        SetTheme(newDark);
        RestartExplorer();
        return newDark;
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
