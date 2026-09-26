using Microsoft.Win32;

namespace DesktopOverlay;

internal static class AutoStart
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "DesktopOverlay";

    internal static bool IsEnabled => GetValue() is not null;

    internal static string? GetValue()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey, writable: false);
            return key?.GetValue(ValueName) as string;
        }
        catch (Exception ex)
        {
            Program.Log("не удалось прочитать автозапуск: " + ex.Message);
            return null;
        }
    }

    internal static string? BuildCommand()
    {
        var exe = Environment.ProcessPath;
        if (string.IsNullOrEmpty(exe))
            return null;

        var image = Program.SavedWallpaperPath;
        if (image is null || !File.Exists(image))
            return null;

        return $"\"{exe}\" --apply \"{image}\"";
    }

    internal static void Set(bool enabled)
    {
        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(RunKey, writable: true);
            if (key is null)
                return;

            if (enabled)
            {
                var cmd = BuildCommand();
                if (string.IsNullOrEmpty(cmd))
                {
                    MessageBox.Show(
                        "Не удалось построить команду автозапуска: сначала установите обои.",
                        Program.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning);
                    return;
                }
                key.SetValue(ValueName, cmd, RegistryValueKind.String);
            }
            else
            {
                key.DeleteValue(ValueName, throwOnMissingValue: false);
            }

            Program.Log(enabled ? "автозапуск включён" : "автозапуск выключен");
        }
        catch (Exception ex)
        {
            Program.Log("ошибка автозапуска: " + ex.Message);
        }
    }
}
