use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Disks, Networks, System};
use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

// 자루 옆 휴식 6프레임(idle, 0~5) + 먹기 시퀀스 18프레임(eating, 6~23, 6번은 눕기<->앉기 전환 겸용).
const STORY_FRAMES: [&[u8]; 24] = [
    include_bytes!("../icons/capybara_pixel/story1.png"),
    include_bytes!("../icons/capybara_pixel/story2.png"),
    include_bytes!("../icons/capybara_pixel/story3.png"),
    include_bytes!("../icons/capybara_pixel/story4.png"),
    include_bytes!("../icons/capybara_pixel/story5.png"),
    include_bytes!("../icons/capybara_pixel/story6.png"),
    include_bytes!("../icons/capybara_pixel/story7.png"),
    include_bytes!("../icons/capybara_pixel/story8.png"),
    include_bytes!("../icons/capybara_pixel/story9.png"),
    include_bytes!("../icons/capybara_pixel/story10.png"),
    include_bytes!("../icons/capybara_pixel/story11.png"),
    include_bytes!("../icons/capybara_pixel/story12.png"),
    include_bytes!("../icons/capybara_pixel/story13.png"),
    include_bytes!("../icons/capybara_pixel/story14.png"),
    include_bytes!("../icons/capybara_pixel/story15.png"),
    include_bytes!("../icons/capybara_pixel/story16.png"),
    include_bytes!("../icons/capybara_pixel/story17.png"),
    include_bytes!("../icons/capybara_pixel/story18.png"),
    include_bytes!("../icons/capybara_pixel/story19.png"),
    include_bytes!("../icons/capybara_pixel/story20.png"),
    include_bytes!("../icons/capybara_pixel/story21.png"),
    include_bytes!("../icons/capybara_pixel/story22.png"),
    include_bytes!("../icons/capybara_pixel/story23.png"),
    include_bytes!("../icons/capybara_pixel/story24.png"),
];
const IDLE_END: usize = 5;
const TRANSITION: usize = 6;
const EAT_START: usize = 7;
const EAT_END: usize = 23;

#[derive(Clone, Copy, PartialEq, Serialize)]
enum Stage {
    Idle,
    Normal,
    High,
}

impl Stage {
    fn from_usage(usage: f32) -> Self {
        if usage < 20.0 {
            Stage::Idle
        } else if usage < 70.0 {
            Stage::Normal
        } else {
            Stage::High
        }
    }

    fn frame_interval(self) -> Duration {
        match self {
            Stage::Idle => Duration::from_millis(700),
            Stage::Normal => Duration::from_millis(350),
            Stage::High => Duration::from_millis(130),
        }
    }
}

#[derive(Clone, Serialize)]
struct MemoryStatus {
    used_gb: f64,
    total_gb: f64,
    percent: f64,
    swap_used_gb: f64,
    swap_total_gb: f64,
}

#[derive(Clone, Serialize)]
struct StorageStatus {
    used_gb: f64,
    total_gb: f64,
    percent: f64,
}

#[derive(Clone, Serialize)]
struct BatteryStatus {
    percent: f64,
    charging: bool,
}

#[derive(Clone, Serialize)]
struct NetworkStatus {
    upload_kbps: f64,
    download_kbps: f64,
    interface: String,
    local_ip: String,
}

#[derive(Clone, Serialize)]
struct SystemStatus {
    cpu_percent: f32,
    stage: Stage,
    memory: MemoryStatus,
    storage: StorageStatus,
    battery: Option<BatteryStatus>,
    network: NetworkStatus,
    uptime_secs: u64,
    hostname: String,
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

type CarrotCount = Arc<Mutex<u64>>;

fn carrot_count_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("carrot_count.json"))
}

fn load_carrot_count(app: &AppHandle) -> u64 {
    carrot_count_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<u64>(&s).ok())
        .unwrap_or(0)
}

fn save_carrot_count(app: &AppHandle, count: u64) {
    if let Some(path) = carrot_count_path(app) {
        let _ = std::fs::write(path, count.to_string());
    }
}

#[tauri::command]
fn get_carrot_count(count: tauri::State<CarrotCount>) -> u64 {
    *count.lock().unwrap()
}

fn load_png(bytes: &[u8]) -> Image<'static> {
    let img = image::load_from_memory(bytes)
        .expect("bad icon bytes")
        .to_rgba8();
    let (width, height) = img.dimensions();
    Image::new_owned(img.into_raw(), width, height)
}

fn read_battery() -> Option<BatteryStatus> {
    let manager = starship_battery::Manager::new().ok()?;
    let battery = manager.batteries().ok()?.next()?.ok()?;
    let percent = battery.state_of_charge().value as f64 * 100.0;
    let charging = battery.state() == starship_battery::State::Charging;
    Some(BatteryStatus { percent, charging })
}

fn start_status_polling(app: AppHandle, stage: Arc<Mutex<Stage>>, show_cpu_text: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut sys = System::new();
        let mut networks = Networks::new_with_refreshed_list();
        let mut prev_rx: u64 = networks.iter().map(|(_, d)| d.total_received()).sum();
        let mut prev_tx: u64 = networks.iter().map(|(_, d)| d.total_transmitted()).sum();

        loop {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            std::thread::sleep(Duration::from_millis(1000));

            let usage = sys.global_cpu_usage();
            let current = Stage::from_usage(usage);
            *stage.lock().unwrap() = current;

            let mem_total = sys.total_memory() as f64 / 1_073_741_824.0;
            let mem_used = sys.used_memory() as f64 / 1_073_741_824.0;

            let disks = Disks::new_with_refreshed_list();
            let (disk_total, disk_avail) = disks
                .iter()
                .find(|d| d.mount_point().to_str() == Some("/"))
                .map(|d| (d.total_space(), d.available_space()))
                .unwrap_or((0, 0));
            let disk_total_gb = disk_total as f64 / 1_073_741_824.0;
            let disk_used_gb = (disk_total.saturating_sub(disk_avail)) as f64 / 1_073_741_824.0;

            networks.refresh();
            let rx: u64 = networks.iter().map(|(_, d)| d.total_received()).sum();
            let tx: u64 = networks.iter().map(|(_, d)| d.total_transmitted()).sum();
            let download_kbps = (rx.saturating_sub(prev_rx)) as f64 / 1024.0;
            let upload_kbps = (tx.saturating_sub(prev_tx)) as f64 / 1024.0;
            prev_rx = rx;
            prev_tx = tx;

            let active_iface = networks
                .iter()
                .filter(|(name, _)| *name != "lo0")
                .max_by_key(|(_, d)| d.total_received() + d.total_transmitted());
            let (iface_name, local_ip) = match active_iface {
                Some((name, data)) => {
                    let ip = data
                        .ip_networks()
                        .iter()
                        .find(|n| n.addr.is_ipv4())
                        .map(|n| n.addr.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    (name.clone(), ip)
                }
                None => ("-".to_string(), "-".to_string()),
            };

            let status = SystemStatus {
                cpu_percent: usage,
                stage: current,
                memory: MemoryStatus {
                    used_gb: mem_used,
                    total_gb: mem_total,
                    percent: if mem_total > 0.0 {
                        mem_used / mem_total * 100.0
                    } else {
                        0.0
                    },
                    swap_used_gb: sys.used_swap() as f64 / 1_073_741_824.0,
                    swap_total_gb: sys.total_swap() as f64 / 1_073_741_824.0,
                },
                storage: StorageStatus {
                    used_gb: disk_used_gb,
                    total_gb: disk_total_gb,
                    percent: if disk_total_gb > 0.0 {
                        disk_used_gb / disk_total_gb * 100.0
                    } else {
                        0.0
                    },
                },
                battery: read_battery(),
                network: NetworkStatus {
                    upload_kbps,
                    download_kbps,
                    interface: iface_name,
                    local_ip,
                },
                uptime_secs: System::uptime(),
                hostname: System::host_name().unwrap_or_else(|| "-".to_string()),
            };

            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_tooltip(Some(format!("CapyBite — CPU {:.0}%", usage)));
                let title = if show_cpu_text.load(Ordering::Relaxed) {
                    Some(format!("{:.0}%", usage))
                } else {
                    None
                };
                let _ = tray.set_title(title);
            }
            let _ = app.emit("system-status", &status);
        }
    });
}

// Idle(쉬는 중)일 땐 눕기/뒹굴기 구간만 왔다갔다, 그 이상 부하일 때만 일어나서 먹기 구간 재생,
// 다시 idle로 돌아갈 때 눕기 전환 구간을 한 번 재생. 프레임셋/구간은 스킨마다 다름(Skin::ranges).
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    RisingUp,
    Eating,
    LyingDown,
}

fn start_icon_animation(app: AppHandle, stage: Arc<Mutex<Stage>>, carrot_count: CarrotCount) {
    std::thread::spawn(move || {
        let frames: Vec<Image<'static>> = STORY_FRAMES.iter().map(|b| load_png(b)).collect();
        let mut index: usize = 0;
        let mut direction: i32 = 1;
        let mut phase = Phase::Idle;
        let mut previous_stage: Option<Stage> = None;

        loop {
            let current = { *stage.lock().unwrap() };
            std::thread::sleep(current.frame_interval());

            let is_idle = matches!(current, Stage::Idle);

            // 빠른 속도(High)로 막 올라온 순간엔 먹던 당근을 버리고 처음부터
            // 다시 줍는다 — 진행 중이던 사이클은 카운트되지 않는다.
            if matches!(current, Stage::High)
                && !matches!(previous_stage, Some(Stage::High))
                && matches!(phase, Phase::Eating)
            {
                index = EAT_START;
                direction = 1;
            }
            previous_stage = Some(current);

            match phase {
                Phase::Idle => {
                    if !is_idle {
                        phase = Phase::RisingUp;
                        index = TRANSITION;
                    } else {
                        let mut signed = index as i32 + direction;
                        if signed >= IDLE_END as i32 {
                            signed = IDLE_END as i32;
                            direction = -1;
                        } else if signed <= 0 {
                            signed = 0;
                            direction = 1;
                        }
                        index = signed as usize;
                    }
                }
                Phase::RisingUp => {
                    phase = Phase::Eating;
                    index = EAT_START;
                    direction = 1;
                }
                Phase::Eating => {
                    if is_idle {
                        phase = Phase::LyingDown;
                        index = TRANSITION;
                    } else {
                        let mut signed = index as i32 + direction;
                        if signed >= EAT_END as i32 {
                            if direction == 1 {
                                // 당근 하나 다 먹음(먹기 구간 끝까지 도달)
                                let mut count = carrot_count.lock().unwrap();
                                *count += 1;
                                let _ = app.emit("carrot-count", *count);
                                save_carrot_count(&app, *count);
                            }
                            signed = EAT_END as i32;
                            direction = -1;
                        } else if signed <= EAT_START as i32 {
                            signed = EAT_START as i32;
                            direction = 1;
                        }
                        index = signed as usize;
                    }
                }
                Phase::LyingDown => {
                    phase = Phase::Idle;
                    index = 0;
                    direction = 1;
                }
            }

            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_icon(Some(frames[index].clone()));
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![quit_app, get_carrot_count])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);

            let show_cpu_text_item = CheckMenuItemBuilder::with_id("toggle_cpu_text", "CPU % 표시")
                .checked(false)
                .build(app)?;
            let autostart_item = CheckMenuItemBuilder::with_id("toggle_autostart", "로그인 시 자동 실행")
                .checked(autostart_enabled)
                .build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "종료").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show_cpu_text_item)
                .item(&autostart_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let show_cpu_text = Arc::new(AtomicBool::new(false));
            let last_shown_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

            let closed = load_png(STORY_FRAMES[0]);
            {
                let last_shown_at = last_shown_at.clone();
                TrayIconBuilder::with_id("main")
                    .icon(closed)
                    .tooltip("CapyBite")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(move |tray, event| {
                        if let TrayIconEvent::Click {
                            rect,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let visible = window.is_visible().unwrap_or(false);
                                if visible {
                                    let _ = window.hide();
                                } else {
                                    let scale = window.scale_factor().unwrap_or(1.0);
                                    let icon_pos = rect.position.to_physical::<f64>(scale);
                                    let icon_size = rect.size.to_physical::<f64>(scale);
                                    let win_width = window
                                        .outer_size()
                                        .map(|s| s.width as f64)
                                        .unwrap_or(320.0);
                                    let x =
                                        icon_pos.x + icon_size.width / 2.0 - win_width / 2.0;
                                    let y = icon_pos.y + icon_size.height;
                                    let _ =
                                        window.set_position(tauri::PhysicalPosition::new(x, y));
                                    *last_shown_at.lock().unwrap() = Some(Instant::now());
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                    })
                    .build(app)?;
            }

            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(false) = event {
                        let recently_shown = last_shown_at
                            .lock()
                            .unwrap()
                            .map(|t| t.elapsed() < Duration::from_millis(300))
                            .unwrap_or(false);
                        if !recently_shown {
                            let _ = window_clone.hide();
                        }
                    }
                });
            }

            {
                let show_cpu_text = show_cpu_text.clone();
                app.on_menu_event(move |app, event| match event.id().as_ref() {
                    "toggle_cpu_text" => {
                        let next = !show_cpu_text.load(Ordering::Relaxed);
                        show_cpu_text.store(next, Ordering::Relaxed);
                        let _ = show_cpu_text_item.set_checked(next);
                    }
                    "toggle_autostart" => {
                        let autolaunch = app.autolaunch();
                        let enabled = autolaunch.is_enabled().unwrap_or(false);
                        if enabled {
                            let _ = autolaunch.disable();
                        } else {
                            let _ = autolaunch.enable();
                        }
                        let _ = autostart_item.set_checked(!enabled);
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                });
            }

            let stage = Arc::new(Mutex::new(Stage::Idle));
            let carrot_count: CarrotCount = Arc::new(Mutex::new(load_carrot_count(&app.handle())));
            app.manage(carrot_count.clone());
            start_status_polling(app.handle().clone(), stage.clone(), show_cpu_text);
            start_icon_animation(app.handle().clone(), stage, carrot_count);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
