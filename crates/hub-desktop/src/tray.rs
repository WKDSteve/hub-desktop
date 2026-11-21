//! `hub-desktop tray`: the hub with its icon in the notification area (Windows).
//!
//! This is how the hub runs when nobody watches a terminal: the login entry, the link and a double click
//! start it (`start`), and it shows itself by its icon instead of being a process without a window. The
//! menu (`traymenu`) opens the page, opens and stops the hubs of the other apps, and quits. When it comes
//! up it starts the hubs of `start_with_hub` in the background (`<program> start`).
//!
//! The main thread runs the loop of the system messages (winit), which the icon needs; the hub itself runs
//! on the threads of the tokio runtime. They talk through events of the loop: the icon never waits for the
//! hub.

use std::sync::Mutex;
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::app::{self, Ready, ServeOptions};
use crate::launch::Request;
use crate::traymenu::{self, Action, HubLine, RunnerLine};

/// How often the menu looks at the hubs again (no program is run for it: a file and a loopback probe).
const LOOK_EVERY: Duration = Duration::from_secs(5);
/// The group icon of the program in its resources (`assets/hub-desktop.res`, written by the generator).
const ICON_RESOURCE: u16 = 1;
/// At the login the notification area may not be there yet: the icon is tried again this often, this far
/// apart, before the hub gives up (30 s).
const ICON_TRIES: u32 = 15;
const ICON_AGAIN: Duration = Duration::from_secs(2);

/// What reaches the loop.
enum Event {
    Ready(Ready),
    Look(Look),
    Menu(String),
    OpenHub,
    Ended(Result<(), String>),
}

/// Run the hub with its icon until "Quit hub", `<program> stop`, or an error of the hub.
///
/// # Errors
///
/// A text for the person: the hub could not start (another one runs, the port), or the system has no
/// notification area.
pub fn run(rt: Runtime) -> Result<(), String> {
    let event_loop = EventLoop::<Event>::with_user_event()
        .build()
        .map_err(|e| format!("no loop of system messages: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let (stop, stopped) = oneshot::channel::<()>();
    let to_loop = proxy.clone();
    rt.spawn(async move {
        let when_ready = to_loop.clone();
        let ended = app::serve_until(
            ServeOptions::default(),
            async {
                let _ = stopped.await;
            },
            move |ready| {
                let _ = when_ready.send_event(Event::Ready(ready));
            },
        )
        .await;
        let _ = to_loop.send_event(Event::Ended(ended));
    });
    let mut tray = Tray {
        rt: rt.handle().clone(),
        proxy,
        stop: Some(stop),
        icon: None,
        ready: None,
        look: None,
        ended: None,
        tries: 0,
    };
    let looped = event_loop.run_app(&mut tray);
    let ended = tray.ended.take();
    drop(tray);
    // A start or a stop of another app that is on its way is not waited for: it is that app's.
    rt.shutdown_timeout(Duration::from_secs(2));
    looped.map_err(|e| format!("the loop of system messages ended: {e}"))?;
    ended.unwrap_or(Ok(()))
}

/// What the menu shows: the hubs of the kit and how many hints the CI watcher has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Look {
    lines: Vec<HubLine>,
    ci_hints: usize,
    runners: Vec<RunnerLine>,
}

struct Tray {
    rt: tokio::runtime::Handle,
    proxy: EventLoopProxy<Event>,
    stop: Option<oneshot::Sender<()>>,
    icon: Option<TrayIcon>,
    ready: Option<Ready>,
    /// What the menu shows now; `None` until the first look.
    look: Option<Look>,
    ended: Option<Result<(), String>>,
    /// How often the icon was tried.
    tries: u32,
}

impl Tray {
    /// Send the clicks on the icon and its menu to the loop.
    fn listen(&self) {
        let menus = Mutex::new(self.proxy.clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Ok(proxy) = menus.lock() {
                let _ = proxy.send_event(Event::Menu(event.id.0));
            }
        }));
        let clicks = Mutex::new(self.proxy.clone());
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
                && let Ok(proxy) = clicks.lock()
            {
                let _ = proxy.send_event(Event::OpenHub);
            }
        }));
    }

    /// Put the icon into the notification area; when it is not there yet, try again a little later.
    fn show(&mut self, event_loop: &ActiveEventLoop) {
        let look = self.look.clone().unwrap_or_default();
        let built = Icon::from_resource(ICON_RESOURCE, None)
            .map_err(|e| e.to_string())
            .and_then(|image| {
                TrayIconBuilder::new()
                    .with_menu(Box::new(menu(&look)))
                    .with_menu_on_left_click(false)
                    .with_tooltip(traymenu::tooltip(&look.lines))
                    .with_icon(image)
                    .build()
                    .map_err(|e| e.to_string())
            });
        self.tries += 1;
        match built {
            Ok(icon) => {
                self.icon = Some(icon);
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Err(e) if self.tries < ICON_TRIES => {
                eprintln!("the icon is not shown yet ({e}); trying again");
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    std::time::Instant::now() + ICON_AGAIN,
                ));
            }
            Err(e) => {
                // A hub nobody can see or end is what this program must not be: it ends, and its end
                // ends the loop and keeps this reason.
                event_loop.set_control_flow(ControlFlow::Wait);
                self.ended = Some(Err(format!(
                    "the icon could not be shown in the notification area: {e}"
                )));
                self.quit();
            }
        }
    }

    fn quit(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    fn on_ready(&mut self, ready: Ready) {
        // The hubs that start with hub-desktop, one after the other, in the background and without a page.
        let starting = ready.clone();
        self.rt.spawn(async move {
            for id in &starting.start_with_hub {
                match starting.launcher.launch(id, Request::Background).await {
                    Ok(outcome) => eprintln!("start with hub-desktop: {id}: {outcome:?}"),
                    Err(e) => eprintln!("start with hub-desktop: {id}: {e:?}"),
                }
            }
        });
        // The menu follows the hubs; the look ends with the loop.
        let looking = ready.clone();
        let proxy = self.proxy.clone();
        self.rt.spawn(async move {
            loop {
                let runners = looking.runners.as_ref().map_or_else(Vec::new, |r| {
                    traymenu::runner_lines(r.board().as_ref(), &r.starting())
                });
                let look = Look {
                    lines: traymenu::lines(&looking.detector.kit_hubs().await),
                    ci_hints: looking.ci.hint_count(),
                    runners,
                };
                if proxy.send_event(Event::Look(look)).is_err() {
                    return;
                }
                tokio::time::sleep(LOOK_EVERY).await;
            }
        });
        self.ready = Some(ready);
    }

    fn on_look(&mut self, look: Look) {
        if self.look.as_ref() == Some(&look) {
            return;
        }
        if let Some(icon) = &self.icon {
            icon.set_menu(Some(Box::new(menu(&look))));
            let _ = icon.set_tooltip(Some(traymenu::tooltip(&look.lines)));
        }
        self.look = Some(look);
    }

    fn act(&mut self, action: Action) {
        let Some(ready) = self.ready.clone() else {
            // Before the hub serves there is nothing to open or stop; quitting still quits.
            if action == Action::Quit {
                self.quit();
            }
            return;
        };
        match action {
            Action::OpenHub => {
                self.rt.spawn(async move {
                    if let Err(e) = app::open().await.and_then(|url| app::deliver(&url)) {
                        eprintln!("the page was not opened: {e}");
                    }
                });
            }
            Action::Open(id) => {
                self.rt.spawn(async move {
                    if let Err(e) = ready.launcher.launch(id, Request::Default).await {
                        eprintln!("open {id}: {e:?}");
                    }
                });
            }
            Action::Stop(id) => {
                self.rt.spawn(async move {
                    if let Err(e) = ready.launcher.stop(id).await {
                        eprintln!("stop {id}: {e:?}");
                    }
                });
            }
            Action::StartRunner(repo) => {
                let Some(runners) = ready.runners.clone() else {
                    return;
                };
                self.rt.spawn_blocking(move || {
                    if let Err(why) = runners.start(&repo) {
                        eprintln!("runner {repo}: {why}");
                    }
                });
            }
            Action::Quit => self.quit(),
        }
    }
}

/// The menu for `look`: "Open hub", "CI: n hints" (when there are some), a submenu per app, "Quit hub".
fn menu(look: &Look) -> Menu {
    let lines = &look.lines;
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id(
        traymenu::OPEN_HUB,
        "Open hub",
        true,
        None,
    ));
    if let Some(label) = traymenu::ci_label(look.ci_hints) {
        let _ = menu.append(&MenuItem::with_id(traymenu::CI_HINTS, label, true, None));
    }
    if !lines.is_empty() {
        let _ = menu.append(&PredefinedMenuItem::separator());
    }
    for line in lines {
        let open = MenuItem::with_id(traymenu::open_id(line.id), "Open", true, None);
        let stop = MenuItem::with_id(
            traymenu::stop_id(line.id),
            "Stop",
            traymenu::can_stop(line),
            None,
        );
        if let Ok(sub) = Submenu::with_items(traymenu::label(line), true, &[&open, &stop]) {
            let _ = menu.append(&sub);
        }
    }
    if !look.runners.is_empty() {
        let items: Vec<MenuItem> = look
            .runners
            .iter()
            .map(|r| {
                MenuItem::with_id(
                    traymenu::runner_id(&r.repo),
                    traymenu::runner_label(r),
                    traymenu::can_start(r),
                    None,
                )
            })
            .collect();
        let refs: Vec<&dyn tray_icon::menu::IsMenuItem> = items
            .iter()
            .map(|i| i as &dyn tray_icon::menu::IsMenuItem)
            .collect();
        if let Ok(sub) = Submenu::with_items(traymenu::runners_title(&look.runners), true, &refs) {
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&sub);
        }
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id(traymenu::QUIT, "Quit hub", true, None));
    menu
}

impl ApplicationHandler<Event> for Tray {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        match cause {
            StartCause::Init => {
                self.listen();
                self.show(event_loop);
            }
            StartCause::ResumeTimeReached { .. } if self.icon.is_none() && self.ended.is_none() => {
                self.show(event_loop);
            }
            _ => {}
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Ready(ready) => self.on_ready(ready),
            Event::Look(look) => self.on_look(look),
            Event::Menu(id) => {
                if let Some(action) = traymenu::action(&id) {
                    self.act(action);
                }
            }
            Event::OpenHub => self.act(Action::OpenHub),
            Event::Ended(ended) => {
                // The icon goes with the hub (dropped before the loop ends, so no stale icon stays).
                self.icon = None;
                if self.ended.is_none() {
                    self.ended = Some(ended);
                }
                event_loop.exit();
            }
        }
    }
}
