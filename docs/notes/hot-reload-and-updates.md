# Hot-Reload & Self-Updating System

**Inspired by**: ccdash (Claude Code Dashboard) update mechanism

The control panel supports seamless updates and hot-reloading without interrupting operations.

---

## Hot-Reload Architecture

### File Watching & Auto-Reload

The control panel watches for file changes and automatically reloads code without restarting:

```python
from watchdog.observers import Observer
from watchdog.events import FileSystemEventHandler
import importlib
import sys

class ControlPanelReloader(FileSystemEventHandler):
    """Hot-reload code changes without restart"""

    def __init__(self, app):
        self.app = app
        self.reload_debounce = 0.5  # 500ms debounce
        self.last_reload = 0
        self.watched_modules = [
            'control_panel',
            'conversational_interface',
            'worker_manager',
            'cost_optimizer'
        ]

    def on_modified(self, event):
        """Called when Python file changes"""
        if not event.src_path.endswith('.py'):
            return

        # Debounce rapid changes
        now = time.time()
        if now - self.last_reload < self.reload_debounce:
            return

        self.last_reload = now
        self.reload_modules()

    def reload_modules(self):
        """Reload changed modules"""
        self.app.show_notification("🔄 Reloading code...")

        try:
            # Save current state
            state = self.app.save_state()

            # Reload modules
            for module_name in self.watched_modules:
                if module_name in sys.modules:
                    importlib.reload(sys.modules[module_name])

            # Restore state
            self.app.restore_state(state)

            self.app.show_notification("✓ Hot-reload complete", duration=2)

        except Exception as e:
            self.app.show_notification(f"✗ Reload failed: {e}", level="error")
            # Continue with old code
```

### State Preservation During Reload

```python
class ControlPanelApp:
    def save_state(self) -> Dict:
        """Save application state before reload"""
        return {
            'workers': self.worker_manager.get_all_workers(),
            'conversation_history': self.conversation.history,
            'active_panel': self.focused_panel_id,
            'scroll_positions': {
                panel: panel.scroll_offset
                for panel in self.panels
            },
            'filter_settings': self.filters,
            'sort_settings': self.sort_config,
            'user_preferences': self.preferences
        }

    def restore_state(self, state: Dict):
        """Restore application state after reload"""
        # Restore worker pool (don't restart workers)
        self.worker_manager.sync_state(state['workers'])

        # Restore conversation history
        self.conversation.history = state['conversation_history']

        # Restore UI state
        self.focus_panel(state['active_panel'])
        for panel, offset in state['scroll_positions'].items():
            panel.scroll_to(offset)

        self.filters = state['filter_settings']
        self.sort_config = state['sort_settings']
        self.preferences = state['user_preferences']

        # Refresh display
        self.refresh()
```

### Watched Files

```yaml
# control-panel-config.yaml
hot_reload:
  enabled: true
  watch_paths:
    - ./control_panel.py
    - ./conversational_interface.py
    - ./worker_manager.py
    - ./cost_optimizer.py
    - ./dashboard_widgets/
    - ./tools/

  exclude_patterns:
    - "**/__pycache__/**"
    - "**/*.pyc"
    - "**/tests/**"
    - "**/.git/**"

  debounce_ms: 500  # Wait 500ms after last change
  reload_notification: true
  auto_reload: true  # Set false to require manual reload
```

### Manual Reload

User can trigger reload manually:

```
[Press r]

┌─ RELOAD ────────────────────────────────────────────────────────────┐
│ 🔄 Reloading control panel...                                       │
│                                                                      │
│ • Saving current state...                            ✓              │
│ • Reloading control_panel.py...                      ✓              │
│ • Reloading conversational_interface.py...           ✓              │
│ • Reloading worker_manager.py...                     ✓              │
│ • Reloading dashboard_widgets...                     ✓              │
│ • Restoring state...                                 ✓              │
│                                                                      │
│ ✓ Reload complete (1.2s)                                            │
│                                                                      │
│                                            [Enter] Continue          │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Self-Updating System

### Update Check on Startup

```
╔═══════════════════════════════════════════════════════════════════╗
║ CONTROL PANEL v1.2.3                                             ║
║                                                                   ║
║ 🆕 Update available: v1.3.0                                      ║
║    • New: Multi-worker coordination                              ║
║    • New: Cost analytics dashboard                               ║
║    • Fix: Memory leak in worker spawning                         ║
║                                                                   ║
║    Press [U] to update now, or [Enter] to skip                   ║
╚═══════════════════════════════════════════════════════════════════╝
```

### One-Key Update (Press 'U')

```
[Press U]

┌─ UPDATE CONTROL PANEL ──────────────────────────────────────────────┐
│ Current version: v1.2.3                                             │
│ Latest version:  v1.3.0                                             │
│                                                                      │
│ Update source: github.com/user/control-panel                        │
│ Update method: git pull + pip install                               │
│                                                                      │
│ Proceed with update? [Y/n] _                                        │
└──────────────────────────────────────────────────────────────────────┘

[Press Y]

┌─ UPDATING... ───────────────────────────────────────────────────────┐
│ 🔄 Update in progress (do not close)                                │
│                                                                      │
│ Step 1/5: Checking prerequisites...                     ✓ 0.2s     │
│ Step 2/5: Fetching latest version...                    ✓ 2.1s     │
│   → git fetch origin                                                │
│   → git pull origin main                                            │
│                                                                      │
│ Step 3/5: Installing dependencies...                    ⏳ (15s)    │
│   → pip install -r requirements.txt --upgrade                       │
│   → Installing textual 0.85.0...                                    │
│   → Installing anthropic 1.5.2...                                   │
│                                                                      │
│ Step 4/5: Running migrations...                         (pending)   │
│ Step 5/5: Restarting application...                     (pending)   │
│                                                                      │
│                                          [Esc] Cancel (not safe yet)│
└──────────────────────────────────────────────────────────────────────┘
```

After successful update:

```
┌─ UPDATE COMPLETE ───────────────────────────────────────────────────┐
│ ✓ Successfully updated to v1.3.0                                    │
│                                                                      │
│ Changelog:                                                           │
│ • Added multi-worker coordination with bead-level locking           │
│ • New cost analytics dashboard with hourly breakdown                │
│ • Fixed memory leak in worker spawning (issue #42)                  │
│ • Improved conversation history persistence                         │
│ • Updated dependencies (Textual 0.85.0, Anthropic 1.5.2)            │
│                                                                      │
│ Restarting in 3 seconds...                                          │
│                                                                      │
│                              [Enter] Restart now | [Esc] Continue   │
└──────────────────────────────────────────────────────────────────────┘
```

### Update Architecture

```python
class SelfUpdater:
    """Handles self-updating from remote source"""

    def __init__(self, config):
        self.config = config
        self.current_version = self._get_current_version()
        self.update_source = config['update']['source']  # git, pypi, url

    async def check_for_updates(self) -> Optional[UpdateInfo]:
        """Check if newer version available"""
        if self.update_source == 'git':
            return await self._check_git_updates()
        elif self.update_source == 'pypi':
            return await self._check_pypi_updates()
        elif self.update_source == 'url':
            return await self._check_url_updates()

    async def _check_git_updates(self) -> Optional[UpdateInfo]:
        """Check for updates from Git repository"""
        # Fetch remote
        result = await self._run_command(['git', 'fetch', 'origin'])

        # Compare versions
        remote_version = await self._get_remote_git_version()
        if self._is_newer(remote_version, self.current_version):
            changelog = await self._get_git_changelog()
            return UpdateInfo(
                version=remote_version,
                changelog=changelog,
                release_date=await self._get_git_release_date()
            )

        return None

    async def perform_update(self, update_info: UpdateInfo) -> bool:
        """Execute the update"""
        try:
            # Step 1: Verify prerequisites
            self._show_progress("Checking prerequisites...", 1, 5)
            await self._verify_prerequisites()

            # Step 2: Fetch latest version
            self._show_progress("Fetching latest version...", 2, 5)
            await self._fetch_update()

            # Step 3: Install dependencies
            self._show_progress("Installing dependencies...", 3, 5)
            await self._install_dependencies()

            # Step 4: Run migrations
            self._show_progress("Running migrations...", 4, 5)
            await self._run_migrations()

            # Step 5: Prepare restart
            self._show_progress("Preparing restart...", 5, 5)
            self._schedule_restart()

            return True

        except Exception as e:
            self._show_error(f"Update failed: {e}")
            await self._rollback()
            return False

    async def _fetch_update(self):
        """Fetch update based on source type"""
        if self.update_source == 'git':
            await self._run_command(['git', 'pull', 'origin', 'main'])
        elif self.update_source == 'pypi':
            await self._run_command(['pip', 'install', '--upgrade', 'control-panel'])
        elif self.update_source == 'url':
            await self._download_and_extract_tarball()

    async def _install_dependencies(self):
        """Update dependencies"""
        await self._run_command([
            'pip', 'install', '-r', 'requirements.txt', '--upgrade'
        ])

    async def _rollback(self):
        """Rollback to previous version on failure"""
        if self.update_source == 'git':
            await self._run_command(['git', 'reset', '--hard', 'HEAD@{1}'])
        # Restore from backup
        await self._restore_backup()

    def _schedule_restart(self):
        """Schedule application restart"""
        # Save state for restoration after restart
        self.app.save_state_to_disk()

        # Set restart flag
        Path('~/.control-panel/restart_pending').touch()

        # Exit with special code to trigger restart
        sys.exit(42)  # 42 = restart requested
```

### Update Sources

#### 1. Git Repository (Default)
```yaml
# control-panel-config.yaml
update:
  enabled: true
  source: git  # git | pypi | url

  git:
    remote: origin
    branch: main
    auto_fetch: true  # Fetch updates on startup
    fetch_interval: 3600  # Check every hour

  check_on_startup: true
  notify_on_update: true
  auto_update: false  # Require manual confirmation
```

#### 2. PyPI Package
```yaml
update:
  source: pypi
  pypi:
    package_name: control-panel
    index_url: https://pypi.org/simple
    pre_release: false  # Allow pre-release versions
```

#### 3. Direct URL
```yaml
update:
  source: url
  url:
    manifest_url: https://example.com/control-panel/manifest.json
    download_url: https://example.com/control-panel/{version}.tar.gz
```

---

## Background Update Checking

```python
class BackgroundUpdateChecker:
    """Periodically check for updates in background"""

    def __init__(self, updater: SelfUpdater, interval: int = 3600):
        self.updater = updater
        self.interval = interval  # seconds
        self.running = False

    async def start(self):
        """Start background update checking"""
        self.running = True
        while self.running:
            try:
                update_info = await self.updater.check_for_updates()
                if update_info:
                    self._notify_update_available(update_info)
            except Exception as e:
                logger.error(f"Update check failed: {e}")

            await asyncio.sleep(self.interval)

    def _notify_update_available(self, update_info: UpdateInfo):
        """Show non-intrusive notification"""
        self.app.show_notification(
            f"🆕 Update available: {update_info.version}",
            action="Press [U] to update",
            duration=10,  # seconds
            priority="low"
        )
```

### Update Notification Banner

Non-intrusive banner at top of dashboard:

```
╔═══════════════════════════════════════════════════════════════════════════════════════════════════════════════╗
║ 🆕 Update available: v1.3.0 | [U] Update | [D] Details | [X] Dismiss (remind in 24h)                         ║
╠═══════════════════════════════════════════════════════════════════════════════════════════════════════════════╣
║ CONTROL PANEL DASHBOARD                                                                                       ║
```

Press `D` for details:

```
┌─ UPDATE DETAILS ────────────────────────────────────────────────────┐
│ Version v1.3.0 (released 2 days ago)                                │
│                                                                      │
│ What's New:                                                          │
│ ✨ Features:                                                         │
│   • Multi-worker coordination with bead-level locking               │
│   • Cost analytics dashboard with hourly breakdown                  │
│   • Chat history persistence across sessions                        │
│                                                                      │
│ 🐛 Bug Fixes:                                                        │
│   • Fixed memory leak in worker spawning (#42)                      │
│   • Fixed race condition in task assignment (#38)                   │
│   • Improved error handling in conversational interface             │
│                                                                      │
│ 📦 Dependencies:                                                     │
│   • Textual 0.84.2 → 0.85.0                                         │
│   • Anthropic 1.4.1 → 1.5.2                                         │
│                                                                      │
│ Download size: 2.4 MB                                                │
│ Estimated time: ~30 seconds                                          │
│                                                                      │
│              [U] Update Now | [L] View Full Changelog | [Esc] Close │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Migration System

Handle database/config schema changes:

```python
class MigrationRunner:
    """Run migrations during updates"""

    def __init__(self, from_version: str, to_version: str):
        self.from_version = from_version
        self.to_version = to_version
        self.migrations_dir = Path(__file__).parent / 'migrations'

    async def run_migrations(self):
        """Execute all pending migrations"""
        migrations = self._get_pending_migrations()

        for migration in migrations:
            await self._run_migration(migration)

    def _get_pending_migrations(self) -> List[Migration]:
        """Find migrations between versions"""
        all_migrations = sorted(
            self.migrations_dir.glob('*.py'),
            key=lambda p: p.stem
        )

        pending = []
        for migration_file in all_migrations:
            version = self._extract_version(migration_file)
            if self._is_between_versions(version, self.from_version, self.to_version):
                pending.append(self._load_migration(migration_file))

        return pending

    async def _run_migration(self, migration: Migration):
        """Execute a single migration"""
        logger.info(f"Running migration: {migration.name}")

        try:
            # Backup before migration
            await self._backup_data()

            # Run migration
            await migration.up()

            # Record migration
            self._record_migration(migration)

        except Exception as e:
            logger.error(f"Migration failed: {e}")
            # Rollback
            await migration.down()
            raise
```

Example migration:

```python
# migrations/0003_add_conversation_history_persistence.py

async def up():
    """Add conversation history persistence"""
    # Create new table
    db.execute('''
        CREATE TABLE conversation_history (
            id INTEGER PRIMARY KEY,
            turn INTEGER,
            user_input TEXT,
            agent_response TEXT,
            timestamp TEXT,
            dashboard_state JSON
        )
    ''')

    # Migrate existing data
    for session in db.query('SELECT * FROM sessions'):
        if 'conversation' in session:
            for exchange in session['conversation']:
                db.insert('conversation_history', exchange)

async def down():
    """Rollback migration"""
    db.execute('DROP TABLE conversation_history')
```

---

## Rollback Capability

If update fails, rollback to previous version:

```
┌─ UPDATE FAILED ─────────────────────────────────────────────────────┐
│ ✗ Update to v1.3.0 failed                                           │
│                                                                      │
│ Error: Failed to install dependencies                                │
│   → pip install textual>=0.85.0 failed                              │
│   → Incompatible with Python 3.8 (requires 3.9+)                    │
│                                                                      │
│ Rolling back to v1.2.3...                                           │
│ • Restoring code from backup...                         ✓           │
│ • Restoring database...                                 ✓           │
│ • Restoring configuration...                            ✓           │
│                                                                      │
│ ✓ Rollback complete, running v1.2.3                                 │
│                                                                      │
│ 💡 TIP: Update requires Python 3.9+                                 │
│    Current: Python 3.8.10                                           │
│                                                                      │
│                                                      [Enter] Continue│
└──────────────────────────────────────────────────────────────────────┘
```

### Automatic Backup Before Update

```python
class UpdateBackupManager:
    """Manage backups before updates"""

    def create_backup(self) -> Path:
        """Create full backup before update"""
        backup_dir = Path('~/.control-panel/backups') / datetime.now().strftime('%Y%m%d_%H%M%S')
        backup_dir.mkdir(parents=True, exist_ok=True)

        # Backup code
        shutil.copytree('.', backup_dir / 'code', ignore=shutil.ignore_patterns('.git'))

        # Backup database
        shutil.copy2('~/.control-panel/beads.db', backup_dir / 'beads.db')

        # Backup configuration
        shutil.copy2('control-panel-config.yaml', backup_dir / 'config.yaml')

        # Backup logs
        shutil.copytree('~/.control-panel/logs', backup_dir / 'logs')

        return backup_dir

    def restore_backup(self, backup_dir: Path):
        """Restore from backup"""
        # Restore code
        shutil.rmtree('.', ignore_errors=True)
        shutil.copytree(backup_dir / 'code', '.')

        # Restore database
        shutil.copy2(backup_dir / 'beads.db', '~/.control-panel/beads.db')

        # Restore configuration
        shutil.copy2(backup_dir / 'config.yaml', 'control-panel-config.yaml')
```

---

## Update Keyboard Shortcuts

```
[U] - Check for updates and install if available
[Shift+U] - Force update check (bypass cache)
[R] - Reload (hot-reload code without update)
[Ctrl+R] - Hard restart (exit and restart process)
```

---

## Configuration

```yaml
# control-panel-config.yaml

hot_reload:
  enabled: true
  watch_paths: ["./control_panel.py", "./worker_manager.py"]
  debounce_ms: 500
  auto_reload: true
  notification: true

update:
  enabled: true
  source: git  # git | pypi | url

  # Git settings
  git:
    remote: origin
    branch: main
    auto_fetch: true
    fetch_interval: 3600  # Check every hour

  # Update behavior
  check_on_startup: true
  notify_on_update: true
  auto_update: false  # Require manual confirmation

  # Background checking
  background_check:
    enabled: true
    interval: 3600  # seconds
    notify: true

  # Backup settings
  backup:
    create_before_update: true
    keep_backups: 5  # Keep last 5 backups
    backup_location: ~/.control-panel/backups

  # Migration settings
  migrations:
    enabled: true
    auto_run: true  # Run migrations automatically during update
    backup_before_migration: true

  # Rollback settings
  rollback:
    enabled: true
    auto_rollback_on_failure: true
    max_rollback_attempts: 3
```

---

## Restart Mechanism

After update, seamlessly restart application:

```python
def restart_application():
    """Restart the application after update"""
    # Save current state
    state_file = Path('~/.control-panel/state_before_restart.json')
    with open(state_file, 'w') as f:
        json.dump(app.save_state(), f)

    # Get current executable and arguments
    python = sys.executable
    script = sys.argv[0]

    # Restart with same arguments
    os.execv(python, [python, script] + sys.argv[1:])
```

On startup, check for saved state:

```python
def restore_state_after_restart():
    """Restore state after restart"""
    state_file = Path('~/.control-panel/state_before_restart.json')
    if state_file.exists():
        with open(state_file) as f:
            state = json.load(f)
        app.restore_state(state)
        state_file.unlink()  # Remove state file
        app.show_notification("✓ Resumed after update")
```

---

## Update Changelog Display

Fetch and display changelog from Git commits:

```python
async def get_changelog(from_version: str, to_version: str) -> str:
    """Generate changelog from Git commits"""
    result = await run_command([
        'git', 'log',
        f'{from_version}..{to_version}',
        '--pretty=format:%h %s',
        '--no-merges'
    ])

    commits = result.stdout.strip().split('\n')

    # Categorize commits
    features = []
    fixes = []
    other = []

    for commit in commits:
        if 'feat:' in commit or 'feature:' in commit:
            features.append(commit)
        elif 'fix:' in commit or 'bug:' in commit:
            fixes.append(commit)
        else:
            other.append(commit)

    # Format changelog
    changelog = []
    if features:
        changelog.append("✨ Features:")
        changelog.extend(f"  • {c}" for c in features)
    if fixes:
        changelog.append("\n🐛 Bug Fixes:")
        changelog.extend(f"  • {c}" for c in fixes)
    if other:
        changelog.append("\n📦 Other Changes:")
        changelog.extend(f"  • {c}" for c in other)

    return '\n'.join(changelog)
```

---

## Summary

The control panel supports:

1. **Hot-Reload**: Automatic code reloading on file changes (500ms debounce)
2. **One-Key Update**: Press `U` to update from Git/PyPI/URL
3. **Background Checking**: Hourly update checks with notifications
4. **State Preservation**: Workers and UI state maintained across reloads/updates
5. **Migration System**: Automatic database/config migrations during updates
6. **Automatic Backup**: Full backup before every update
7. **Rollback**: Automatic rollback on update failure
8. **Seamless Restart**: State restored after restart
9. **Changelog Display**: Auto-generated from Git commits
10. **Multiple Sources**: Git, PyPI, or direct URL downloads

Users simply press `U` and the system handles everything automatically, just like ccdash!
