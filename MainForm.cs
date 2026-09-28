using System.Drawing.Imaging;

namespace DesktopOverlay;

internal sealed class MainForm : Form
{
    private readonly DropZone _drop;
    private readonly Label _hint;
    private readonly Button _pickFile;
    private readonly Button _pickColor;
    private readonly Button _paste;
    private readonly Button _apply;
    private readonly Button _revert;
    private readonly Button _themeToggle;
    private readonly Button _taskbarToggle;
    private readonly CheckBox _autoStart;
    private readonly Label _status;

    private readonly OverlayEngine _engine = new();
    private string? _chosen;
    private bool _reallyExit;

    internal MainForm(string? preselect = null, bool autoApply = false)
    {
        Text = "Обои рабочего стола — " + Program.AppName;
        ClientSize = new Size(500, 472);
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        AllowDrop = true;
        KeyPreview = true;
        BackColor = Color.FromArgb(18, 18, 18);
        ForeColor = Color.FromArgb(230, 230, 230);
        Font = new Font("Segoe UI", 9F);
        Icon = Program.LoadAppIcon();

        _drop = new DropZone
        {
            Bounds = new Rectangle(20, 16, 460, 170),
        };

        _hint = new Label
        {
            Dock = DockStyle.Fill,
            TextAlign = ContentAlignment.MiddleCenter,
            ForeColor = Color.FromArgb(140, 140, 140),
            Font = new Font("Segoe UI", 9.5F),
            Text = "Перетащите картинку сюда\r\nили выберите файл / сплошной цвет ниже\r\n\r\nCtrl+V — вставить из буфера обмена\r\nJPG, PNG, BMP, GIF, TIFF",
            BackColor = Color.Transparent,
            Cursor = Cursors.Hand,
        };
        _drop.Controls.Add(_hint);

        var colorLabel = new Label
        {
            Text = "Цвет:",
            Bounds = new Rectangle(20, 198, 42, 24),
            TextAlign = ContentAlignment.MiddleLeft,
            ForeColor = Color.FromArgb(150, 150, 150),
            Font = new Font("Segoe UI", 8.5F),
        };
        Controls.Add(colorLabel);

        var presets = new (Color Color, string Name)[]
        {
            (Color.FromArgb(0, 0, 0), "Чёрный"),
            (Color.FromArgb(18, 18, 18), "Тёмный графит"),
            (Color.FromArgb(28, 33, 40), "Тёмно-синий"),
            (Color.FromArgb(37, 42, 52), "Сланцевый"),
            (Color.FromArgb(50, 50, 50), "Серый"),
            (Color.FromArgb(245, 245, 245), "Белый"),
        };

        int presetX = 64;
        foreach (var preset in presets)
        {
            var btn = CreatePresetButton(preset.Color, preset.Name, presetX, 196, 26);
            btn.Click += (_, _) => SetSolidColor(preset.Color);
            Controls.Add(btn);
            presetX += 32;
        }

        _pickColor = CreateDarkButton("Палитра…", 256, 196, 74, 26);
        _pickColor.Click += (_, _) => PickCustomColor();
        Controls.Add(_pickColor);

        _pickFile = CreateDarkButton("Обзор…", 334, 196, 74, 26);
        _pickFile.Click += (_, _) => PickFile();
        Controls.Add(_pickFile);

        _paste = CreateDarkButton("📋 Буфер", 412, 196, 68, 26);
        _paste.Click += (_, _) => PasteFromClipboard();
        new ToolTip().SetToolTip(_paste, "Вставить картинку из буфера обмена (Ctrl+V)");
        Controls.Add(_paste);

        _apply = CreateDarkButton("Установить обои", 20, 234, 300, 34, isPrimary: true);
        _apply.Click += (_, _) => ApplyWallpaper();
        _apply.Enabled = false;

        _revert = CreateDarkButton("Откатить всё", 330, 234, 150, 34);
        _revert.Click += (_, _) => Revert();

        _themeToggle = CreateDarkButton("🌓 Переключить тему Windows", 20, 278, 460, 34);
        _themeToggle.Click += (_, _) => ToggleWindowsTheme();
        UpdateThemeButtonText();

        _taskbarToggle = CreateDarkButton("", 20, 320, 460, 34);
        _taskbarToggle.Click += (_, _) => ToggleTaskbarSize();
        UpdateTaskbarButtonText();

        _autoStart = new CheckBox
        {
            Bounds = new Rectangle(20, 364, 460, 24),
            Text = "Запускать вместе с Windows",
            AutoSize = false,
            Checked = AutoStart.IsEnabled,
            ForeColor = Color.FromArgb(200, 200, 200),
            BackColor = Color.Transparent,
            Cursor = Cursors.Hand,
        };
        _autoStart.CheckedChanged += (_, _) =>
        {
            if (_autoStart.Checked != AutoStart.IsEnabled)
                AutoStart.Set(_autoStart.Checked);
        };

        _status = new Label
        {
            Bounds = new Rectangle(20, 396, 460, 65),
            ForeColor = Color.FromArgb(125, 125, 125),
            Font = new Font("Segoe UI", 8.5F),
            Text = "Оверлей работает поверх политик, пока запущена программа.\nЗначок приложения находится в системном трее.",
        };

        Controls.AddRange(new Control[] { _drop, _apply, _revert, _themeToggle, _taskbarToggle, _autoStart, _status });

        _hint.Click += (_, _) => PickFile();
        _drop.Click += (_, _) => PickFile();

        _drop.DragEnter += OnDragEnter;
        _drop.DragDrop += OnDragDrop;
        DragEnter += OnDragEnter;
        DragDrop += OnDragDrop;

        _engine.ShowRequested += () => BeginInvoke(new Action(ShowWindow));
        _engine.ExitRequested += () => BeginInvoke(new Action(ReallyExit));
        _engine.RevertRequested += () => BeginInvoke(new Action(Revert));
        _engine.StatusChanged += msg => _status.Text = msg;

        if (AutoStart.IsEnabled)
            _status.Text = "Автозапуск активен. Оверлей прикреплен к рабочему столу.";

        if (preselect is not null)
        {
            SelectFile(preselect);
            if (autoApply)
                ApplyWallpaper();
        }
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        Native.EnableDarkModeForWindow(Handle);
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        if (keyData == (Keys.Control | Keys.V))
        {
            PasteFromClipboard();
            return true;
        }

        return base.ProcessCmdKey(ref msg, keyData);
    }

    private static Button CreateDarkButton(string text, int x, int y, int width, int height, bool isPrimary = false)
    {
        var btn = new Button
        {
            Text = text,
            Bounds = new Rectangle(x, y, width, height),
            FlatStyle = FlatStyle.Flat,
            Cursor = Cursors.Hand,
            Font = new Font("Segoe UI", 9F, isPrimary ? FontStyle.Bold : FontStyle.Regular),
        };
        btn.FlatAppearance.BorderSize = 1;

        if (isPrimary)
        {
            btn.BackColor = Color.FromArgb(240, 240, 240);
            btn.ForeColor = Color.FromArgb(15, 15, 15);
            btn.FlatAppearance.BorderColor = Color.FromArgb(255, 255, 255);
            btn.FlatAppearance.MouseOverBackColor = Color.White;
            btn.FlatAppearance.MouseDownBackColor = Color.FromArgb(210, 210, 210);
        }
        else
        {
            btn.BackColor = Color.FromArgb(28, 28, 28);
            btn.ForeColor = Color.FromArgb(220, 220, 220);
            btn.FlatAppearance.BorderColor = Color.FromArgb(50, 50, 50);
            btn.FlatAppearance.MouseOverBackColor = Color.FromArgb(40, 40, 40);
            btn.FlatAppearance.MouseDownBackColor = Color.FromArgb(20, 20, 20);
        }

        return btn;
    }

    private static Button CreatePresetButton(Color color, string tooltip, int x, int y, int size)
    {
        var btn = new Button
        {
            Bounds = new Rectangle(x, y, size, size),
            FlatStyle = FlatStyle.Flat,
            BackColor = color,
            Cursor = Cursors.Hand,
            Text = string.Empty,
        };
        btn.FlatAppearance.BorderSize = 1;
        btn.FlatAppearance.BorderColor = Color.FromArgb(60, 60, 60);
        btn.FlatAppearance.MouseOverBackColor = color;

        var tip = new ToolTip();
        tip.SetToolTip(btn, tooltip);
        return btn;
    }

    private void PickFile()
    {
        using var dialog = new OpenFileDialog
        {
            Title = "Выберите картинку для обоев",
            Filter = "Изображения|*.jpg;*.jpeg;*.png;*.bmp;*.gif;*.tif;*.tiff|Все файлы|*.*",
            CheckFileExists = true,
        };

        if (dialog.ShowDialog(this) == DialogResult.OK)
            SelectFile(dialog.FileName);
    }

    private void PasteFromClipboard()
    {
        if (!ClipboardImage.TryTake(out var path, out var error))
        {
            _status.Text = error;
            return;
        }

        SelectFile(path);
        if (_chosen is null)
            return;

        _status.Text = "Картинка из буфера обмена выбрана, устанавливаю...";
        ApplyWallpaper("Картинка из буфера обмена установлена на рабочий стол.");
    }

    private void PickCustomColor()
    {
        using var dialog = new ColorDialog
        {
            FullOpen = true,
            AnyColor = true,
            Color = Color.FromArgb(18, 18, 18),
        };

        if (dialog.ShowDialog(this) == DialogResult.OK)
            SetSolidColor(dialog.Color);
    }

    private void SetSolidColor(Color color)
    {
        try
        {
            Directory.CreateDirectory(Program.DataDir);
            var path = Path.Combine(Program.DataDir, "wallpaper.png");

            var screen = Screen.PrimaryScreen?.Bounds ?? new Rectangle(0, 0, 1920, 1080);
            int w = Math.Max(1920, screen.Width);
            int h = Math.Max(1080, screen.Height);

            using (var bmp = new Bitmap(w, h, PixelFormat.Format32bppPArgb))
            {
                using (var g = Graphics.FromImage(bmp))
                {
                    using var brush = new SolidBrush(color);
                    g.FillRectangle(brush, 0, 0, w, h);
                }
                bmp.Save(path, ImageFormat.Png);
            }

            SelectFile(path, isSolidColor: true, colorHex: $"#{color.R:X2}{color.G:X2}{color.B:X2}");
        }
        catch (Exception ex)
        {
            _status.Text = "Ошибка генерации цвета: " + ex.Message;
        }
    }

    private void OnDragEnter(object? sender, DragEventArgs e)
    {
        e.Effect = ResolveDrop(e.Data) is null ? DragDropEffects.None : DragDropEffects.Copy;
    }

    private void OnDragDrop(object? sender, DragEventArgs e)
    {
        var path = ResolveDrop(e.Data);
        if (path is not null)
            SelectFile(path);
    }

    private static string? ResolveDrop(IDataObject? data)
    {
        if (data is null || !data.GetDataPresent(DataFormats.FileDrop) ||
            data.GetData(DataFormats.FileDrop) is not string[] files)
            return null;

        foreach (var file in files)
        {
            if (File.Exists(file) && Program.IsSupportedImage(file))
                return file;

            if (Directory.Exists(file))
            {
                var found = Directory.EnumerateFiles(file)
                    .Where(Program.IsSupportedImage)
                    .OrderBy(f => f, StringComparer.OrdinalIgnoreCase)
                    .FirstOrDefault();
                if (found is not null)
                    return found;
            }
        }

        return null;
    }

    private void SelectFile(string path, bool isSolidColor = false, string? colorHex = null)
    {
        try
        {
            using var stream = File.OpenRead(path);
            using var probe = Image.FromStream(stream);
            _drop.Image?.Dispose();
            _drop.Image = new Bitmap(probe);
        }
        catch (Exception ex)
        {
            _drop.Image = null;
            _hint.Visible = true;
            _status.Text = "Не удалось открыть файл: " + ex.Message;
            return;
        }

        _chosen = path;
        _hint.Visible = false;
        _apply.Enabled = true;
        _status.Text = isSolidColor
            ? $"Выбран сплошной цвет: {colorHex} (нажмите «Установить обои»)"
            : $"Выбран файл: {Path.GetFileName(path)}";
    }

    private void UpdateThemeButtonText()
    {
        bool isDark = ThemeUtil.IsDarkTheme();
        _themeToggle.Text = isDark
            ? "☀ Сменить тему Windows на светлую"
            : "🌙 Сменить тему Windows на тёмную";
    }

    private void ToggleWindowsTheme()
    {
        bool dark = !ThemeUtil.IsDarkTheme();

        _status.Text = "Переключение темы Windows и перезапуск проводника...";
        Cursor = Cursors.WaitCursor;
        _themeToggle.Enabled = false;
        Application.DoEvents();

        try
        {
            bool applied = ThemeUtil.ApplyAndRestartExplorer(dark);
            UpdateThemeButtonText();

            _status.Text = applied
                ? $"Тема переключена: {(dark ? "Тёмная" : "Светлая")}. Проводник перезапущен."
                : "Не удалось переключить тему Windows — проверьте права записи в реестр.";
        }
        catch (Exception ex)
        {
            _status.Text = "Ошибка смены темы: " + ex.Message;
        }
        finally
        {
            Cursor = Cursors.Default;
            _themeToggle.Enabled = true;
        }
    }

    private void UpdateTaskbarButtonText()
    {
        if (!TaskbarUtil.IsSupported)
        {
            _taskbarToggle.Text = "⤢ Уменьшить панель задач (недоступно)";
            _taskbarToggle.Enabled = false;
            new ToolTip().SetToolTip(_taskbarToggle, TaskbarUtil.IsSupportedMessage);
            return;
        }

        _taskbarToggle.Text = TaskbarUtil.IsSmall == true
            ? "⤢ Вернуть обычную панель задач"
            : "⤢ Уменьшить панель задач";

        new ToolTip().SetToolTip(
            _taskbarToggle,
            "Панель задач станет ниже на 10 пикселей. Проводник перезапустится.");
    }

    private void ToggleTaskbarSize()
    {
        if (!TaskbarUtil.IsSupported)
        {
            _status.Text = TaskbarUtil.IsSupportedMessage;
            return;
        }

        bool target = TaskbarUtil.IsSmall != true;

        _status.Text = "Меняю размер панели задач и перезапускаю Проводник...";
        Cursor = Cursors.WaitCursor;
        _taskbarToggle.Enabled = false;
        Application.DoEvents();

        try
        {
            if (!TaskbarUtil.SetSmall(target))
            {
                _status.Text = "Не удалось изменить панель задач — проверьте права записи в реестр.";
                return;
            }

            _status.Text = target
                ? "Панель задач уменьшена. Проводник перезапущен."
                : "Панель задач возвращена к обычному размеру. Проводник перезапущен.";

            ShellUtil.RestartExplorer();

            Thread.Sleep(400);
            Application.DoEvents();
            Native.RedrawDesktop();

            UpdateTaskbarButtonText();
        }
        catch (Exception ex)
        {
            _status.Text = "Ошибка изменения панели задач: " + ex.Message;
        }
        finally
        {
            Cursor = Cursors.Default;
            _taskbarToggle.Enabled = TaskbarUtil.IsSupported;
        }
    }

    private void ApplyWallpaper(string? successStatus = null)
    {
        if (_chosen is null)
            return;

        var image = Program.LoadStableCopy(_chosen);
        if (image is null)
            return;

        if (!_engine.Apply(image, tray: true))
        {
            _status.Text = "Не удалось встроиться в рабочий стол. Подробности в overlay.log.";
            image.Dispose();
            return;
        }

        _status.Text = successStatus ?? "Обои успешно установлены поверх системных.";
        _autoStart.Checked = AutoStart.IsEnabled;
    }

    private void Revert()
    {
        var answer = MessageBox.Show(
            this,
            "Убрать оверлей с рабочего стола и вернуть системные обои?",
            "Откатить обои",
            MessageBoxButtons.YesNo,
            MessageBoxIcon.Question);

        if (answer != DialogResult.Yes)
            return;

        var alsoLight = MessageBox.Show(
            this,
            "Также вернуть светлую тему Windows?",
            "Тема оформления",
            MessageBoxButtons.YesNo,
            MessageBoxIcon.Question);

        PerformRevert(restoreLight: alsoLight == DialogResult.Yes);
    }

    private void PerformRevert(bool restoreLight)
    {
        var report = Reverter.RevertAll(_engine, restoreLight);

        _drop.Image?.Dispose();
        _drop.Image = null;
        _hint.Visible = true;
        _chosen = null;
        _apply.Enabled = false;
        _autoStart.Checked = AutoStart.IsEnabled;
        UpdateThemeButtonText();
        UpdateTaskbarButtonText();

        _status.Text = "Откат выполнен: " + report + ".";
    }

    protected override void OnFormClosing(FormClosingEventArgs e)
    {
        if (!_reallyExit)
        {
            e.Cancel = true;
            Hide();
            return;
        }

        _engine.Remove();
        base.OnFormClosing(e);
    }

    private void ShowWindow()
    {
        Show();
        WindowState = FormWindowState.Normal;
        Activate();
    }

    private void ReallyExit()
    {
        _reallyExit = true;
        _engine.Remove();
        Close();
        Application.Exit();
    }
}

internal sealed class DropZone : PictureBox
{
    public DropZone()
    {
        BackColor = Color.FromArgb(24, 24, 24);
        SizeMode = PictureBoxSizeMode.Zoom;
        AllowDrop = true;
        DoubleBuffered = true;
    }

    protected override void OnPaint(PaintEventArgs pe)
    {
        base.OnPaint(pe);
        using var pen = new Pen(Color.FromArgb(42, 42, 42), 1);
        pe.Graphics.DrawRectangle(pen, 0, 0, Width - 1, Height - 1);
    }
}
