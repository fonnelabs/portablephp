# FonneLabs PortablePHP

**FonneLabs PortablePHP** is a lightweight, portable Apache + PHP launcher for Windows, built in Rust. It lets you serve any local PHP project instantly—no complex configurations or Windows service installations required.

![FonneLabs PortablePHP](https://www.fonnelabs.com/images/fonnelabs-portablephp-screenshot.webp)

## 🚀 Why PortablePHP?

* **Zero-Install Apache:** Runs out-of-the-box by generating isolated runtime configurations for your projects on the fly.
* **Dual Interface:** Use the friendly desktop GUI to configure paths and watch live request logs, or jump straight into **Terminal Mode** for fast developer workflows.
* **Lightning-Fast Terminal Workflow:** Just add PortablePHP to your system `PATH`, open a terminal inside any project folder, and type `portablephp` to spin up the server and open your browser automatically.

Website: [fonnelabs.com](http://fonnelabs.com)

## Features

- **Instant Project Launch:** Add PortablePHP to your system `PATH`, navigate to any project folder in your terminal, type `portablephp`, and immediately open your site in the browser.
- **Unified Executable:** Supports both a full desktop interface and a fast command-line terminal mode from the same binary.
- **Folder Management:** Convenient directory browsing for your Apache installation, PHP path, and project document root.
- **Persistent Settings:** Automatically saves your configuration beside the executable in `settings.json`.
- **Smart Port Fallback:** Automatically scans up to 100 sequential ports starting from your preference if the requested port is busy.
- **Automatic PHP Module Detection:** Scans the PHP folder to automatically locate matching Apache modules like `php*apache2_4.dll`.
- **Isolated Runtimes:** Generates separate Apache configuration files and runtime artifacts for each active port.
- **Full PHP & Apache Support:** Handles PHP execution, standard directory indexes (`index.php` / `index.html`), directory listings, and `.htaccess` rewrite overrides.
- **Live Monitoring:** Real-time access and error log viewer in the desktop interface featuring smooth scrolling, auto-scroll to new entries, and a **Show POST** filter.


## Requirements

- Windows with a compatible Apache installation containing `bin\httpd.exe` and its `modules` folder.
- A compatible PHP installation containing its Apache module, such as `php8apache2_4.dll`. Use a PHP build that supports loading into your Apache installation, with matching architecture.
- A project folder containing PHP or HTML files.
- Write access to the executable's folder, where settings and runtime files are created.

Apache and PHP are supplied separately. The launcher does not download or install them.

## Desktop usage

1. Place `portablephp.exe` in a writable folder and open it from File Explorer.
2. Fill in the settings using the fields or **Browse** buttons.
3. Click **Save** to store the settings. Starting the server also saves them.
4. Click **Start**. Your default browser opens `http://localhost:<active-port>`.
5. Watch the access and error logs. Uncheck **Show POST** to hide POST request entries from the viewer.
6. Click **Stop** before changing settings or exiting. To serve another project, stop the server, select the new folder, and start again.

| Field | What to select | Example |
| --- | --- | --- |
| Apache Folder | Apache root containing `bin` and `modules` | `E:\xampp\apache` |
| PHP Folder | PHP root containing the Apache PHP module | `E:\xampp\php` |
| Server Folder (Your Project) | Project document root | `E:\projects\my-site\www` |
| Port | Preferred server port | `8080` |


### Desktop versus terminal mode

With no `--port` or `--www` argument, PortablePHP opens the desktop interface when the working directory is the executable's directory. If the working directory differs, it starts in terminal mode. Providing either argument explicitly selects terminal mode.

To open desktop mode reliably from PowerShell, change into the executable's folder first:

```powershell
Set-Location 'E:\tools\PortablePHP'
.\portablephp.exe
```

## Terminal usage

First open the desktop interface, set the Apache and PHP folders, and click **Save**. Terminal mode reads those paths from `settings.json` beside the executable; there are no Apache or PHP folder command-line options.

Run from your project directory to use it as the document root:

```powershell
Set-Location 'E:\projects\my-site\www'
& 'E:\tools\PortablePHP\portablephp.exe' -p 8080
```

Or supply the project folder explicitly:

```powershell
& 'E:\tools\PortablePHP\portablephp.exe' --port 8080 --www 'E:\projects\my-site\www'
```

If the executable is on your `PATH`, you can also run:

```powershell
# From a project directory different from the executable's directory:
portablephp

# Explicit terminal mode with a preferred port:
portablephp -p 8080

# Explicit document root; uses the saved port or 8080 for fresh settings:
portablephp -w 'E:\projects\my-site\www'
```

Terminal mode prints the server URL and document root, opens the browser, and stays running until **Ctrl+C**. Request logs are written to disk; the live log viewer belongs to desktop mode. Command-line settings and any automatically selected port are saved for later runs.

| Option | Description |
| --- | --- |
| `-p`, `--port <PORT>` | Preferred port; searches for an available port starting here. |
| `-w`, `--www <WWW>` | Document root; defaults to the current working directory in terminal mode. |
| `-k`, `--terminateall` | Force-terminate all active `httpd.exe` processes and exit. |
| `-h`, `--help` | Show command-line help. |
| `-V`, `--version` | Show the package version. |

To force-stop Apache processes:

```powershell
portablephp --terminateall
# Equivalent:
portablephp -k
```

`--terminateall` affects every `httpd.exe` process, including Apache instances launched elsewhere. The current Ctrl+C handler also performs this cleanup after stopping its own server.

## Settings and runtime files

All generated files are relative to the executable's directory, rather than your project directory:

```text
PortablePHP/
├── portablephp.exe
├── settings.json
└── tmp/
    └── port_8080/
        ├── portable_httpd.conf
        ├── httpd.pid
        ├── access.log
        └── error.log
```

Each active port uses its own `tmp/port_<port>/` folder. PortablePHP launches Apache with this generated configuration; it does not overwrite the Apache installation's existing `httpd.conf`.

`settings.json` stores `apache_dir`, `php_dir`, `www_dir`, and `port`. Update saved paths after moving Apache, PHP, or the project.

## Build from source

Build on Windows with Rust and Cargo installed, plus the C/C++ build tools and Windows SDK needed by the Windows MSVC toolchain. The build script uses Windows resource tools to embed the executable icon.

Make sure `src/app_icon.ico` exists. It is required by both `include_bytes!` in the source and `build.rs`. The current `.gitignore` excludes `*.ico`, so a source checkout may need this asset supplied separately.

From the folder containing `Cargo.toml`:

```powershell
# Check compilation:
cargo check --locked

# Build the release executable:
cargo build --release --locked
```

The output is `target\release\portablephp.exe`. Copy it into your preferred writable app folder, then configure Apache and PHP through the desktop interface.

To launch the newly built desktop app directly:

```powershell
Set-Location .\target\release
.\portablephp.exe
```

For development:

```powershell
cargo build --locked
Set-Location .\target\debug
.\portablephp.exe
```

Running `cargo run` from the source root selects terminal mode because that working directory differs from `target\debug`. Its settings must already exist beside the debug executable. To serve a project through Cargo, run from the source root:

```powershell
cargo run --locked -- -p 8080 -w 'E:\projects\my-site\www'
```

## Source layout

| File | Purpose |
| --- | --- |
| `src/main.rs` | Desktop interface, `popup_about()`, settings, CLI, Apache configuration, and process/log handling. |
| `src/app_icon.ico` | Icon embedded in the executable, window, and About popup. |
| `build.rs` | Embeds the Windows executable icon. |
| `Cargo.toml` | Package metadata and dependencies. |
| `Cargo.lock` | Locked dependency versions. |

Change About content in `popup_about()` in `src/main.rs`. Its displayed version is currently `1.0.0.0`; the Cargo package and CLI version are `1.0.0`.

## Troubleshooting

| Problem | What to check |
| --- | --- |
| Terminal reports that Apache or PHP is not set | Open desktop mode for that executable and save both paths. Debug and release executables have separate settings folders. |
| Start button is disabled | Close About, stop any running server, and fill in all four settings. |
| Settings cannot be edited | Click Stop first. Close About if it is open. |
| Browser cannot connect | Check `tmp/port_<port>/error.log`, the actual port, Apache's executable path, and PHP module compatibility. |
| Requested port changes | The requested port was busy. Use the active port shown in the desktop status, terminal output, or browser URL. |
| Settings or runtime files are missing | Check beside the executable and ensure that directory is writable. |
| Desktop opens instead of terminal mode | Supply `-p` or `-w`, or launch from a project directory different from the executable's directory. |
| Terminal mode starts when desktop was expected | Launch without `-p`/`-w` from the executable's own directory. |
| Build cannot find `app_icon.ico` | Supply `src/app_icon.ico`; ICO files are currently ignored by Git. |
| Windows executable icon appears outdated | Refresh File Explorer or try a renamed copy; Windows may cache executable icons. |

PortablePHP is intended for development. Its generated configuration permits directory listings and `.htaccess` overrides, and `Listen <port>` does not restrict Apache to the loopback interface. Network access depends on your host and firewall configuration.
