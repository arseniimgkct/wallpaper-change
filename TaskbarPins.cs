using System.Runtime.InteropServices;

namespace DesktopOverlay;

/// <summary>
/// Открепление и закрепление значков на панели задач.
/// Использует штатный глагол оболочки «Открепить от панели задач»,
/// поэтому работает и на Windows 10, и на Windows 11 без прав администратора.
/// </summary>
internal static class TaskbarPins
{
    internal const string EdgeName = "Microsoft Edge";

    private const string AllProgramsNamespace = "shell:::{4234d49b-0245-4df3-b780-3893943456e1}";

    private static readonly string[] UnpinVerbHints =
    {
        "unpin from taskbar",
        "открепить от панели задач",
    };

    private static readonly string[] PinVerbHints =
    {
        "pin to taskbar",
        "закрепить на панели задач",
    };

    private static string PinnedFolder => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "Microsoft", "Internet Explorer", "Quick Launch", "User Pinned", "TaskBar");

    internal static string? FindPinnedShortcut(string itemName) =>
        FindPinnedShortcutFor(itemName + ".lnk");

    internal static bool IsPinned(string itemName) => FindPinnedShortcut(itemName) is not null;

    /// <summary>Снимает значок с панели задач.</summary>
    internal static TaskbarResult TryUnpin(string itemName, out string report)
    {
        string? shortcut = FindPinnedShortcut(itemName);
        if (shortcut is null)
        {
            report = $"«{itemName}» не был закреплён на панели задач.";
            return TaskbarResult.NothingToDo;
        }

        bool viaVerb = InvokeTaskbarVerb(itemName, "taskbarunpin", UnpinVerbHints);
        Program.Log($"Панель задач: попытка открепить «{itemName}», глагол={(viaVerb ? "сработал" : "не найден")}");

        if (viaVerb)
            Thread.Sleep(300);

        shortcut = FindPinnedShortcut(itemName);
        if (shortcut is not null)
        {
            // Запасной путь: Windows не дала глагол, убираем ярлык вручную.
            try
            {
                File.Delete(shortcut);
                Program.Log($"Панель задач: ярлык «{itemName}» удалён напрямую");
                report = $"«{itemName}» откреплён от панели задач.";
                return TaskbarResult.DoneNeedsExplorerRestart;
            }
            catch (Exception ex)
            {
                Program.Log("ошибка удаления ярлыка панели задач: " + ex.Message);
            }
        }

        bool pinned = IsPinned(itemName);
        report = pinned
            ? $"Не удалось открепить «{itemName}» — правый клик по значку → «Открепить от панели задач»."
            : $"«{itemName}» откреплён от панели задач.";
        return pinned ? TaskbarResult.Failed : TaskbarResult.Done;
    }

    /// <summary>Возвращает значок на панель задач.</summary>
    internal static bool TryPin(string itemName, out string report)
    {
        if (IsPinned(itemName))
        {
            report = $"«{itemName}» уже закреплён на панели задач.";
            return true;
        }

        bool viaVerb = InvokeTaskbarVerb(itemName, "taskbarpin", PinVerbHints);
        Program.Log($"Панель задач: попытка закрепить «{itemName}», глагол={(viaVerb ? "сработал" : "не найден")}");

        Thread.Sleep(400);

        bool pinned = IsPinned(itemName);
        report = pinned
            ? $"«{itemName}» закреплён на панели задач."
            : $"Не удалось закрепить «{itemName}» автоматически — правый клик по значку в меню «Пуск».";
        return pinned;
    }

    private static string? FindPinnedShortcutFor(string fileName)
    {
        try
        {
            string path = Path.Combine(PinnedFolder, fileName);
            return File.Exists(path) ? path : null;
        }
        catch (Exception ex)
        {
            Program.Log("ошибка чтения папки закреплённого: " + ex.Message);
            return null;
        }
    }

    /// <summary>
    /// Вызывает глагол оболочки по каноническому имени (taskbarpin / taskbarunpin),
    /// а если его нет — ищет пункт меню по названию на текущем языке интерфейса.
    /// </summary>
    private static bool InvokeTaskbarVerb(string itemName, string canonicalVerb, string[] displayHints)
    {
        try
        {
            var shellType = Type.GetTypeFromProgID("Shell.Application");
            if (shellType is null)
                return false;

            dynamic? shell = Activator.CreateInstance(shellType);
            if (shell is null)
                return false;

            dynamic? folder = shell.NameSpace(AllProgramsNamespace);
            if (folder is null)
                return false;

            foreach (dynamic item in folder.Items())
            {
                string name = item.Name as string ?? string.Empty;
                if (!name.Contains(itemName, StringComparison.CurrentCultureIgnoreCase))
                    continue;

                try
                {
                    item.InvokeVerb(canonicalVerb);
                    return true;
                }
                catch (Exception ex)
                {
                    Program.Log($"Панель задач: глагол {canonicalVerb} для «{name}» недоступен ({ex.Message})");
                }

                foreach (dynamic verb in item.Verbs())
                {
                    string text = (verb.Name as string ?? string.Empty).Replace("&", string.Empty);
                    bool match = displayHints.Any(hint =>
                        text.Contains(hint, StringComparison.CurrentCultureIgnoreCase));

                    if (!match)
                        continue;

                    verb.DoIt();
                    return true;
                }
            }
        }
        catch (Exception ex)
        {
            Program.Log("ошибка работы с оболочкой при изменении панели задач: " + ex.Message);
        }

        return false;
    }
}

internal enum TaskbarResult
{
    /// <summary>Значок и не был закреплён — ничего не меняли.</summary>
    NothingToDo,

    /// <summary>Значок откреплён штатным глаголом оболочки.</summary>
    Done,

    /// <summary>Значок откреплён удалением ярлыка, нужен перезапуск проводника.</summary>
    DoneNeedsExplorerRestart,

    /// <summary>Не получилось.</summary>
    Failed,
}