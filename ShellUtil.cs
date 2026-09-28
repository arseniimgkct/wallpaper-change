using System.Diagnostics;

namespace DesktopOverlay;

internal static class ShellUtil
{
    private const string TaskbarClass = "Shell_TrayWnd";

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

        Thread.Sleep(800);

        if (WaitForTaskbar())
        {
            Program.Log("Проводник поднялся самостоятельно");
            return;
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
            Program.Log("Проводник запущен вручную");
        }
        catch (Exception ex)
        {
            Program.Log("ошибка при запуске explorer: " + ex.Message);
        }
    }

    private static bool WaitForTaskbar()
    {
        for (int i = 0; i < 48; i++)
        {
            if (Native.FindTopLevelByClass(TaskbarClass) != IntPtr.Zero)
                return true;
            Thread.Sleep(250);
        }
        return false;
    }
}
