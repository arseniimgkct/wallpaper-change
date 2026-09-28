using Microsoft.Win32;

namespace DesktopOverlay;

internal static class TaskbarUtil
{
    private const string AdvancedKey = @"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
    private const string ValueName = "TaskbarSmallIcons";

    private const int SmallValue = 1;
    private const int NormalValue = 0;

    private const int Windows11Build = 22000;

    internal static bool IsSupported =>
        Environment.OSVersion.Version.Build < Windows11Build;

    internal static string IsSupportedMessage =>
        "Уменьшение панели задач поддерживается только в Windows 10. " +
        "В Windows 11 этот параметр отключён Microsoft, штатного способа нет.";

    internal static bool? IsSmall
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(AdvancedKey, writable: false);
            return ReadFlag(key, out var isSmall) ? isSmall : null;
        }
    }

    internal static bool SetSmall(bool small)
    {
        if (!IsSupported)
        {
            Program.Log("TaskbarUtil: система не поддерживается");
            return false;
        }

        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(AdvancedKey, writable: true);
            if (key is null)
            {
                Program.Log("TaskbarUtil: ветка реестра недоступна для записи");
                return false;
            }

            int value = small ? SmallValue : NormalValue;
            key.SetValue(ValueName, value, RegistryValueKind.DWord);

            bool applied = ReadFlag(key, out var isSmall) && isSmall == small;
            Program.Log($"Панель задач: запрошена {(small ? "маленькая" : "обычная")}, " +
                        $"подтверждена {(applied ? "да" : "нет")}");
            return applied;
        }
        catch (Exception ex)
        {
            Program.Log("ошибка изменения размера панели задач: " + ex.Message);
            return false;
        }
    }

    internal static bool RestoreDefault()
    {
        try
        {
            using var readOnly = Registry.CurrentUser.OpenSubKey(AdvancedKey, writable: false);
            if (!ReadFlag(readOnly, out var isSmall) || !isSmall)
                return false;

            using var writable = Registry.CurrentUser.CreateSubKey(AdvancedKey, writable: true);
            if (writable is null)
                return false;

            writable.DeleteValue(ValueName, throwOnMissingValue: false);
            Program.Log("Панель задач: параметр удалён, обычный размер восстановлен");
            return true;
        }
        catch (Exception ex)
        {
            Program.Log("ошибка восстановления панели задач: " + ex.Message);
            return false;
        }
    }

    private static bool ReadFlag(RegistryKey? key, out bool isSmall)
    {
        isSmall = false;

        if (key is null)
            return false;

        switch (key.GetValue(ValueName))
        {
            case int i: isSmall = i != 0; return true;
            case long l: isSmall = l != 0; return true;
            case byte b: isSmall = b != 0; return true;
            case string s when int.TryParse(s, out var parsed): isSmall = parsed != 0; return true;
            default: return false;
        }
    }
}
