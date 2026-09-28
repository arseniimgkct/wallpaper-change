using System.Drawing.Imaging;
using System.Runtime.InteropServices;

namespace DesktopOverlay;

internal static class ClipboardImage
{
    private const int Attempts = 12;
    private const int RetryDelayMs = 80;

    internal static bool TryTake(out string path, out string error)
    {
        path = string.Empty;
        error = string.Empty;

        for (int attempt = 1; ; attempt++)
        {
            try
            {
                if (!TryRead(out path, out error))
                    return false;

                Program.Log("картинка взята из буфера обмена: " + path);
                return true;
            }
            catch (ExternalException ex)
            {
                if (attempt >= Attempts)
                {
                    error = "Буфер обмена занят другим приложением. Скопируйте картинку ещё раз и повторите.";
                    Program.Log("буфер обмена недоступен: " + ex.Message);
                    return false;
                }

                Thread.Sleep(RetryDelayMs);
            }
            catch (Exception ex)
            {
                error = ex.Message;
                Program.Log("ошибка чтения буфера обмена: " + ex.Message);
                return false;
            }
        }
    }

    private static bool TryRead(out string path, out string error)
    {
        path = string.Empty;
        error = string.Empty;

        if (Clipboard.ContainsFileDropList())
        {
            foreach (var file in Clipboard.GetFileDropList())
            {
                if (File.Exists(file) && Program.IsSupportedImage(file))
                {
                    path = file;
                    return true;
                }

                if (Directory.Exists(file))
                {
                    var found = Directory.EnumerateFiles(file)
                        .Where(Program.IsSupportedImage)
                        .OrderBy(f => f, StringComparer.OrdinalIgnoreCase)
                        .FirstOrDefault();
                    if (found is not null)
                    {
                        path = found;
                        return true;
                    }
                }
            }
        }

        if (Clipboard.ContainsText() && TryGetPathFromText(Clipboard.GetText(), out var fromText))
        {
            path = fromText;
            return true;
        }

        using (var image = Clipboard.GetImage())
        {
            if (image is not null && image.Width > 0 && image.Height > 0)
            {
                path = Save(image);
                return true;
            }
        }

        if (Clipboard.GetDataObject()?.GetData(DataFormats.Bitmap) is Bitmap raw && raw.Width > 0)
        {
            path = Save(raw);
            return true;
        }

        error = "В буфере обмена нет картинки. Скопируйте изображение (Ctrl+C) и нажмите «Вставить» ещё раз.";
        return false;
    }

    private static bool TryGetPathFromText(string? text, out string path)
    {
        path = string.Empty;

        if (string.IsNullOrWhiteSpace(text))
            return false;

        var line = text.Split('\n', '\r')[0].Trim().Trim('"');
        if (line.Length == 0 || !Program.IsSupportedImage(line))
            return false;

        if (!File.Exists(line))
            return false;

        path = Path.GetFullPath(line);
        return true;
    }

    private static string Save(Image image)
    {
        Directory.CreateDirectory(Program.DataDir);
        var path = Path.Combine(Program.DataDir, "wallpaper.png");

        using var bmp = new Bitmap(image.Width, image.Height, PixelFormat.Format32bppPArgb);
        using (var g = Graphics.FromImage(bmp))
            g.DrawImageUnscaled(image, 0, 0);

        bmp.Save(path, ImageFormat.Png);
        return path;
    }
}
