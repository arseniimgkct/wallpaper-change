namespace DesktopOverlay;

/// <summary>
/// Небольшой выбор: каким браузером заменить Edge.
/// Edge открепляется от панели задач после подтверждения.
/// </summary>
internal sealed class BrowserPickerForm : Form
{
    internal BrowserPickerForm(List<BrowserInfo> browsers)
    {
        Text = "Браузер по умолчанию — " + Program.AppName;
        ClientSize = new Size(430, 210);
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        MinimizeBox = false;
        StartPosition = FormStartPosition.CenterParent;
        ShowInTaskbar = false;
        BackColor = Color.FromArgb(18, 18, 18);
        ForeColor = Color.FromArgb(230, 230, 230);
        Font = new Font("Segoe UI", 9F);
        Icon = Program.LoadAppIcon();

        var title = new Label
        {
            Bounds = new Rectangle(20, 16, 390, 24),
            Text = "Кем заменить Microsoft Edge?",
            ForeColor = Color.FromArgb(235, 235, 235),
            Font = new Font("Segoe UI", 10F, FontStyle.Bold),
        };

        var current = new Label
        {
            Bounds = new Rectangle(20, 42, 390, 22),
            Text = "Сейчас по умолчанию: " + BrowserUtil.CurrentTitle(),
            ForeColor = Color.FromArgb(150, 150, 150),
            Font = new Font("Segoe UI", 8.5F),
        };

        Controls.Add(title);
        Controls.Add(current);

        int y = 70;
        for (int i = 0; i < browsers.Count; i++)
        {
            var info = browsers[i];
            var btn = MainForm.CreateDarkButton($"Сделать {info.Title} браузером по умолчанию", 20, y, 390, 34,
                isPrimary: i == 0);
            btn.Click += (_, _) => Choose(info);
            Controls.Add(btn);

            y += 40;
        }

        var note = new Label
        {
            Bounds = new Rectangle(20, y, 390, 40),
            ForeColor = Color.FromArgb(125, 125, 125),
            Font = new Font("Segoe UI", 8.5F),
            Text = "Firefox назначит себя сам. Chrome и Edge откроют свою страницу настроек — " +
                   "там нужно нажать «Сделать стандартным». Edge при этом открепится от панели задач.",
        };

        var cancel = MainForm.CreateDarkButton("Отмена", 300, ClientSize.Height - 40, 110, 30);
        cancel.Click += (_, _) => { DialogResult = DialogResult.Cancel; Close(); };
        cancel.DialogResult = DialogResult.Cancel;

        Controls.Add(note);
        Controls.Add(cancel);

        CancelButton = cancel;
        if (browsers.Count > 0)
            AcceptButton = Controls.OfType<Button>().First(b => b.Text.StartsWith("Сделать", StringComparison.Ordinal));
    }

    internal BrowserInfo? Selected { get; private set; }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        Native.EnableDarkModeForWindow(Handle);
    }

    private void Choose(BrowserInfo info)
    {
        Selected = info;
        DialogResult = DialogResult.OK;
        Close();
    }
}