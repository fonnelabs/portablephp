#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// FonneLabs PortablePHP CLI & GUI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Port to run the server on
    #[arg(short, long)]
    port: Option<String>,

    /// Root folder of your web project
    #[arg(short, long)]
    www: Option<String>,

    /// Force terminate all active Apache httpd.exe processes
    #[arg(long = "terminateall", short = 'k')]
    terminate_all: bool,
}

#[derive(Clone)]
struct LogEntry {
    timestamp: String,
    text: String,
    tag: String,
}

#[derive(Serialize, Deserialize)]
struct SavedSettings {
    apache_dir: String,
    php_dir: String,
    www_dir: String,
    port: String,
}

struct PortablePhpApp {
    apache_dir: String,
    php_dir: String,
    www_dir: String,
    port: String,
    show_post: bool,
    show_about: bool,
    about_logo: Option<egui::TextureHandle>,
    is_running: bool,
    start_time: String,
    logs: Arc<Mutex<Vec<LogEntry>>>,
    process: Arc<Mutex<Option<Child>>>,
    stop_signal: Arc<Mutex<bool>>,
}

impl Default for PortablePhpApp {
    fn default() -> Self {
        Self {
            apache_dir: "".into(),
            php_dir: "".into(),
            www_dir: "".into(),
            port: "8080".into(),
            show_post: true,
            show_about: false,
            about_logo: None,
            is_running: false,
            start_time: String::new(),
            logs: Arc::new(Mutex::new(Vec::new())),
            process: Arc::new(Mutex::new(None)),
            stop_signal: Arc::new(Mutex::new(false)),
        }
    }
}

impl PortablePhpApp {
    fn popup_about(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }

        if self.about_logo.is_none() {
            if let Ok(icon) = image::load_from_memory(include_bytes!("app_icon.ico")) {
                let rgba = icon.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                self.about_logo = Some(ctx.load_texture(
                    "about_logo",
                    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                    egui::TextureOptions::LINEAR,
                ));
            }
        }

        egui::Window::new("About")
            .open(&mut self.show_about)
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .frame(egui::Frame::window(&ctx.style())
                .fill(egui::Color32::WHITE)
                .inner_margin(20.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(egui::Color32::BLACK);
                ui.vertical_centered(|ui| {
                    if let Some(logo) = &self.about_logo {
                        ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(64.0, 64.0)));
                        ui.add_space(8.0);
                    }
                    ui.heading("FonneLabs PortablePHP");
                    ui.label("v 1.0.0.0");
                    ui.add_space(8.0);
                    ui.hyperlink_to("website: http://fonnelabs.com", "http://fonnelabs.com");
                });
            });
    }

    fn exe_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    fn settings_path() -> PathBuf {
        Self::exe_dir().join("settings.json")
    }

    fn load_settings() -> Self {
        let mut app = Self::default();
        let path = Self::settings_path();

        if path.exists() {
            if let Ok(data) = fs::read_to_string(path) {
                if let Ok(saved) = serde_json::from_str::<SavedSettings>(&data) {
                    app.apache_dir = saved.apache_dir;
                    app.php_dir = saved.php_dir;
                    app.www_dir = saved.www_dir;
                    app.port = saved.port;
                }
            }
        }
        app
    }

    fn save_settings(&self) -> Result<(), Box<dyn std::error::Error>> {
        let saved = SavedSettings {
            apache_dir: self.apache_dir.clone(),
            php_dir: self.php_dir.clone(),
            www_dir: self.www_dir.clone(),
            port: self.port.clone(),
        };
        let json = serde_json::to_string_pretty(&saved)?;
        fs::write(Self::settings_path(), json)?;
        Ok(())
    }

    fn terminate_all_servers() {
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("cmd")
                .args(["/C", "taskkill /F /IM httpd.exe /T >nul 2>&1"])
                .output();
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = Command::new("pkill").args(["-9", "httpd"]).output();
        }
    }

    fn is_port_available(port: u16) -> bool {
        if TcpListener::bind(("0.0.0.0", port)).is_err() {
            return false;
        }

        if TcpListener::bind(("127.0.0.1", port)).is_err() {
            return false;
        }

        let addr: SocketAddr = match format!("127.0.0.1:{}", port).parse() {
            Ok(a) => a,
            Err(_) => return false,
        };

        if TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
            return false;
        }

        true
    }

    fn find_available_port(&self) -> String {
        let start_port: u16 = self.port.parse().unwrap_or(8080);
        for port in start_port..(start_port + 100) {
            if Self::is_port_available(port) {
                return port.to_string();
            }
        }
        self.port.clone()
    }

    fn stop_server(&mut self) {
        *self.stop_signal.lock().unwrap() = true;

        if let Some(mut child) = self.process.lock().unwrap().take() {
            let pid = child.id();

            #[cfg(target_os = "windows")]
            {
                let _ = Command::new("cmd")
                    .args(["/C", &format!("taskkill /F /T /PID {} >nul 2>&1", pid)])
                    .output();
            }

            #[cfg(not(target_os = "windows"))]
            {
                let _ = child.kill();
            }

            let _ = child.wait();
        }

        self.is_running = false;
        self.append_log("[INFO] Server stopped", "error");

        std::thread::sleep(Duration::from_millis(200));
    }

    fn append_log(&self, text: &str, tag: &str) {
        let entry = LogEntry {
            timestamp: chrono::Local::now().format("[%H:%M:%S] ").to_string(),
            text: text.to_string(),
            tag: tag.to_string(),
        };
        self.logs.lock().unwrap().push(entry);
    }

    fn start_server(&mut self) {
        self.stop_server();

        let active_port = self.find_available_port();
        if active_port != self.port {
            self.append_log(
                &format!("[INFO] Port {} was busy. Auto-selected port {}", self.port, active_port),
                "info",
            );
            self.port = active_port;
        }

        let _ = self.save_settings();

        let tmp_dir = Self::exe_dir().join("tmp").join(format!("port_{}", self.port));
        let _ = fs::create_dir_all(&tmp_dir);
        let conf_path = tmp_dir.join("portable_httpd.conf");
        let pid_file = tmp_dir.join("httpd.pid");

        let mut php_dll = Path::new(&self.php_dir).join("php8apache2_4.dll");
        if let Ok(entries) = fs::read_dir(&self.php_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("php") && name.ends_with("apache2_4.dll") {
                    php_dll = entry.path();
                    break;
                }
            }
        }

        let esc = |p: &Path| p.to_string_lossy().replace('\\', "/");
        let access_log = tmp_dir.join("access.log");
        let error_log = tmp_dir.join("error.log");

        let conf = format!(
            "ServerRoot \"{}\"\n\
            DefaultRuntimeDir \"{}\"\n\
            Listen {}\n\
            ServerName localhost:{}\n\
            DocumentRoot \"{}\"\n\
            DirectoryIndex index.php index.html\n\
            <Directory />\n\
                AllowOverride All\n\
                Require all granted\n\
            </Directory>\n\
            <Directory \"{}\">\n\
                Options +Indexes +FollowSymLinks\n\
                AllowOverride All\n\
                Require all granted\n\
            </Directory>\n\
            LoadModule log_config_module modules/mod_log_config.so\n\
            LoadModule env_module modules/mod_env.so\n\
            LoadModule setenvif_module modules/mod_setenvif.so\n\
            LoadModule mime_module modules/mod_mime.so\n\
            LoadModule dir_module modules/mod_dir.so\n\
            LoadModule autoindex_module modules/mod_autoindex.so\n\
            LoadModule rewrite_module modules/mod_rewrite.so\n\
            LoadModule authz_core_module modules/mod_authz_core.so\n\
            LoadModule authz_host_module modules/mod_authz_host.so\n\
            LoadModule alias_module modules/mod_alias.so\n\
            LogFormat \"%h %l %u %t \\\"%r\\\" %>s %b\" common\n\
            LoadModule php_module \"{}\"\n\
            PHPIniDir \"{}\"\n\
            <FilesMatch \\.php$>\n\
                SetHandler application/x-httpd-php\n\
            </FilesMatch>\n\
            ErrorLog \"{}\"\n\
            CustomLog \"{}\" common\n\
            PidFile \"{}\"\n",
            esc(Path::new(&self.apache_dir)),
            esc(&tmp_dir),
            self.port,
            self.port,
            esc(Path::new(&self.www_dir)),
            esc(Path::new(&self.www_dir)),
            esc(&php_dll),
            esc(Path::new(&self.php_dir)),
            esc(&error_log),
            esc(&access_log),
            esc(&pid_file)
        );

        if fs::write(&conf_path, conf).is_err() {
            return;
        }

        let httpd_exe = Path::new(&self.apache_dir).join("bin").join("httpd.exe");
        let mut cmd = Command::new(&httpd_exe);
        cmd.args(["-f", conf_path.to_str().unwrap(), "-X"])
            .current_dir(&self.apache_dir);

        #[cfg(target_os = "windows")]
        use std::os::windows::process::CommandExt;
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);

        if let Ok(child) = cmd.spawn() {
            *self.process.lock().unwrap() = Some(child);
            self.is_running = true;
            *self.stop_signal.lock().unwrap() = false;
            self.start_time = chrono::Local::now().format("%H:%M").to_string();

            self.append_log(&format!("[INFO] Apache started on port {}", self.port), "info");
            self.append_log(&format!("[INFO] Serving directory: {}", self.www_dir), "info");

            self.spawn_tailer(access_log, "".into(), "info");
            self.spawn_tailer(error_log, "[ERROR] ".into(), "error");

            let url = format!("http://localhost:{}", self.port);
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(1));
                let _ = open::that(url);
            });
        }
    }

    fn spawn_tailer(&self, path: PathBuf, prefix: String, tag: &'static str) {
        let stop_sig = Arc::clone(&self.stop_signal);
        let logs = Arc::clone(&self.logs);
        thread::spawn(move || {
            while !path.exists() && !*stop_sig.lock().unwrap() {
                thread::sleep(Duration::from_millis(200));
            }
            if let Ok(file) = File::open(&path) {
                let mut reader = BufReader::new(file);
                let _ = reader.seek(SeekFrom::End(0));
                while !*stop_sig.lock().unwrap() {
                    let mut line = String::new();
                    if let Ok(n) = reader.read_line(&mut line) {
                        if n > 0 {
                            let entry = LogEntry {
                                timestamp: chrono::Local::now().format("[%H:%M:%S] ").to_string(),
                                text: format!("{}{}", prefix, line),
                                tag: tag.to_string(),
                            };
                            logs.lock().unwrap().push(entry);
                        } else {
                            thread::sleep(Duration::from_millis(200));
                        }
                    } else {
                        break;
                    }
                }
            }
        });
    }
}

impl eframe::App for PortablePhpApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(100));

        // Keep native window close requests (including Alt+F4) from closing
        // the app while the About dialog is open.
        if self.show_about && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        let mut style = (*ctx.style()).clone();
        style.visuals.panel_fill = egui::Color32::from_rgb(248, 249, 250);
        ctx.set_style(style);

        egui::CentralPanel::default().show(ctx, |ui| {
            // The About dialog is rendered separately so only its controls
            // remain interactive while it is open.
            if self.show_about {
                ui.disable();
            }
            ui.add_space(5.0);

            fn field_label(ui: &mut egui::Ui, label: &str, text: &str) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    ui.label(
                        egui::RichText::new(label)
                            .size(11.0)
                            .color(egui::Color32::from_rgb(120, 120, 120)),
                    );
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    if !label.is_empty() && text.trim().is_empty() {
                        ui.painter().circle_filled(
                            rect.center(),
                            4.0,
                            egui::Color32::from_rgb(231, 76, 60),
                        );
                    }
                });
            }

            fn compact_folder_field(ui: &mut egui::Ui, label: &str, text: &mut String, width: f32) {
                ui.allocate_ui_with_layout(egui::vec2(width, 42.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    field_label(ui, label, text);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.add_sized(
                            [width - 62.0, 26.0],
                            egui::TextEdit::singleline(text)
                                .margin(egui::Margin { left: 6.0, right: 6.0, top: 5.0, bottom: 5.0 }),
                        );
                        if ui.add_sized([60.0, 26.0], egui::Button::new("Browse")).clicked() {
                            let mut dialog = rfd::FileDialog::new();
                            let trimmed = text.trim();
                            if !trimmed.is_empty() && std::path::Path::new(trimmed).exists() {
                                dialog = dialog.set_directory(trimmed);
                            } else {
                                dialog = dialog.set_directory("C:\\");
                            }

                            if let Some(folder) = dialog.pick_folder() {
                                *text = folder.to_string_lossy().to_string();
                            }
                        }
                    });
                });
            }

            ui.add_enabled_ui(!self.is_running, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let folder_width = (ui.available_width() - 80.0) / 2.0;
                compact_folder_field(ui, "Apache Folder", &mut self.apache_dir, folder_width);
                compact_folder_field(ui, "PHP Folder", &mut self.php_dir, folder_width);
                ui.vertical(|ui| {
                    field_label(ui, "", "");
                    if ui.add_sized([60.0, 26.0], egui::Button::new("Save")).clicked() {
                        match self.save_settings() {
                            Ok(()) => self.append_log("[INFO] Settings saved", "info"),
                            Err(error) => self.append_log(&format!("[ERROR] Could not save settings: {}", error), "error"),
                        }
                    }
                });
            });
            });

            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);

            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let project_width = ui.available_width() - 280.0;
                ui.add_enabled_ui(!self.is_running, |ui| {
                    compact_folder_field(ui, "Server Folder (Your Project)", &mut self.www_dir, project_width);
                });

                ui.add_enabled_ui(!self.is_running, |ui| {
                ui.allocate_ui_with_layout(egui::vec2(60.0, 42.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    field_label(ui, "Port", &self.port);
                    ui.add_sized(
                        [60.0, 26.0],
                        egui::TextEdit::singleline(&mut self.port)
                            .margin(egui::Margin { left: 6.0, right: 6.0, top: 5.0, bottom: 5.0 }),
                    );
                });
                });

                ui.vertical(|ui| {
                    field_label(ui, "", "");
                    ui.horizontal(|ui| {
                        let can_start = !self.is_running
                            && [&self.apache_dir, &self.php_dir, &self.www_dir, &self.port]
                                .iter()
                                .all(|field| !field.trim().is_empty());
                        if ui.add_enabled_ui(can_start, |ui| {
                            let button = if can_start {
                                egui::Button::new(egui::RichText::new("Start").color(egui::Color32::WHITE))
                                    .fill(egui::Color32::BLACK)
                            } else {
                                egui::Button::new("Start")
                            };
                            ui.add_sized([60.0, 26.0], button)
                        }).inner.clicked() {
                            self.start_server();
                        }

                        if ui.add_enabled_ui(self.is_running, |ui| {
                            let button = if self.is_running {
                                egui::Button::new(egui::RichText::new("Stop").color(egui::Color32::WHITE))
                                    .fill(egui::Color32::RED)
                            } else {
                                egui::Button::new("Stop")
                            };
                            ui.add_sized([60.0, 26.0], button)
                        }).inner.clicked() {
                            self.stop_server();
                        }
                        if ui.add_sized([60.0, 26.0], egui::Button::new("About")).clicked() {
                            self.show_about = true;
                        }
                    });
                });
            });

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.show_post, "Show POST");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (status_text, dot_color) = if self.is_running {
                        (
                            format!("Running since {} • localhost:{}", self.start_time, self.port),
                            egui::Color32::from_rgb(46, 196, 182),
                        )
                    } else {
                        ("Stopped".to_string(), egui::Color32::from_rgb(231, 76, 60))
                    };

                    ui.label(status_text);

                    let (rect, _response) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, dot_color);
                });
            });

            ui.add_space(5.0);

            egui::Frame::none()
                .fill(egui::Color32::from_rgb(30, 30, 30))
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;

                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let logs = self.logs.lock().unwrap();
                            for entry in logs.iter() {
                                let trimmed_text = entry.text.trim_end();

                                if !self.show_post && trimmed_text.contains("\"POST ") {
                                    continue;
                                }

                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    ui.label(
                                        egui::RichText::new(&entry.timestamp)
                                            .monospace()
                                            .size(12.0)
                                            .color(egui::Color32::from_rgb(119, 119, 119)),
                                    );

                                    let text_color = if entry.tag == "error" {
                                        egui::Color32::from_rgb(255, 107, 107)
                                    } else {
                                        egui::Color32::WHITE
                                    };

                                    ui.label(
                                        egui::RichText::new(trimmed_text)
                                            .monospace()
                                            .size(12.0)
                                            .color(text_color),
                                    );
                                });
                            }
                        });
                });
        });

        self.popup_about(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let cli = Cli::parse();

    if cli.terminate_all {
        #[cfg(target_os = "windows")]
        unsafe {
            extern "system" {
                fn AttachConsole(dwProcessId: u32) -> i32;
            }
            const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
            AttachConsole(ATTACH_PARENT_PROCESS);
        }

        println!("\nTerminating all active Apache (httpd.exe) processes...");
        let _ = std::io::stdout().flush();

        PortablePhpApp::terminate_all_servers();

        println!("All Apache processes terminated successfully.\n");
        let _ = std::io::stdout().flush();

        return Ok(());
    }

    let exe_dir = PortablePhpApp::exe_dir();
    let current_dir = std::env::current_dir().unwrap_or_default();

    let is_terminal_run = current_dir != exe_dir || cli.port.is_some() || cli.www.is_some();

    let mut app = PortablePhpApp::load_settings();

    if is_terminal_run {
        #[cfg(target_os = "windows")]
        unsafe {
            extern "system" {
                fn AttachConsole(dwProcessId: u32) -> i32;
            }
            const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
            AttachConsole(ATTACH_PARENT_PROCESS);
        }

        if let Some(p) = cli.port {
            app.port = p;
        }

        if let Some(w) = cli.www {
            app.www_dir = w;
        } else {
            app.www_dir = current_dir.to_string_lossy().to_string();
        }

        let mut missing_folders = false;
        for (label, folder) in [("Apache", &app.apache_dir), ("PHP", &app.php_dir)] {
            if folder.trim().is_empty() {
                eprintln!("[ERROR] {} folder is not set.", label);
                missing_folders = true;
            }
        }
        if missing_folders {
            eprintln!("Open PortablePHP in GUI mode, select the Apache and PHP folders, and click Save before running from the terminal.");
            let _ = std::io::stderr().flush();
            std::process::exit(1);
        }

        app.start_server();

        println!("\nPortablePHP Running...");
        println!("Server: http://localhost:{}", app.port);
        println!("Document Root: {}", app.www_dir);
        println!("Press Ctrl+C to stop...\n");
        let _ = std::io::stdout().flush();

        let process_handle = Arc::clone(&app.process);

        let _ = ctrlc::set_handler(move || {
            println!("\n[Ctrl+C] Stopping PortablePHP...");
            let _ = std::io::stdout().flush();

            if let Some(child) = process_handle.lock().unwrap().take() {
                let pid = child.id();
                #[cfg(target_os = "windows")]
                {
                    let _ = Command::new("cmd")
                        .args(["/C", &format!("taskkill /F /T /PID {} >nul 2>&1", pid)])
                        .output();
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let mut c = child;
                    let _ = c.kill();
                }
            }

            PortablePhpApp::terminate_all_servers();

            std::process::exit(0);
        });

        loop {
            thread::sleep(Duration::from_millis(500));
        }
    }

    let icon_bytes = include_bytes!("app_icon.ico");
    let icon = image::load_from_memory(icon_bytes)
        .ok()
        .map(|img| {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            Arc::new(egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            })
        });

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([900.0, 420.0])
        .with_min_inner_size([600.0, 260.0]);

    if let Some(icon_data) = icon {
        viewport = viewport.with_icon(icon_data);
    }

    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "FonneLabs PortablePHP",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
}
