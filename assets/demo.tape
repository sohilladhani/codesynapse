# codesynapse demo — run with: vhs assets/demo.tape
# Prerequisites: vhs installed (https://github.com/charmbracelet/vhs)
#                codesynapse binary in PATH
#                internet access (repos cloned automatically via codesynapse clone)

Output assets/demo.gif

Set Width    1300
Set Height   680
Set FontSize 13
Set FontFamily "JetBrainsMono Nerd Font"
Set Theme    "Catppuccin Mocha"
Set Padding  20
Set Margin   20
Set BorderRadius 12
Set WindowBar Colorful
Set CursorBlink false

# ── 1. setup ────────────────────────────────────────────────────────────────

Sleep 300ms
Type "codesynapse setup"
Sleep 100ms
Enter
Sleep 1s

# ── 2. clone + index first repo (express — small, fast) ─────────────────────

Sleep 300ms
Type "codesynapse clone https://github.com/gothinkster/node-express-realworld-example-app --out ./bench/repos/express-realworld"
Sleep 100ms
Enter
Sleep 4s
Type "codesynapse module add express ./bench/repos/express-realworld"
Sleep 100ms
Enter
Sleep 3s

# ── 3. clone + index second repo (django — medium) ───────────────────────────

Sleep 200ms
Type "codesynapse clone https://github.com/gothinkster/django-realworld-example-app --out ./bench/repos/django-realworld"
Sleep 100ms
Enter
Sleep 4s
Type "codesynapse module add django ./bench/repos/django-realworld"
Sleep 100ms
Enter
Sleep 4s

# ── 4. list modules ──────────────────────────────────────────────────────────

Sleep 200ms
Type "codesynapse module list"
Sleep 100ms
Enter
Sleep 500ms

# ── 5. query the graph ───────────────────────────────────────────────────────

Sleep 300ms
Type 'codesynapse query "authentication and user login"'
Sleep 100ms
Enter
Sleep 3s

# ── 6. hold on final state ───────────────────────────────────────────────────

Sleep 1s
