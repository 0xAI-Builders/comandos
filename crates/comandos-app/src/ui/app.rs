use crate::{
    config::{self, AppConfig},
    dash_client::DashClient,
    guard::WriteGuard,
    jobs::Jobs,
    poll::Poller,
    state_files::StateFiles,
    tabs::TabRegistry,
    tmux::TmuxCtl,
    ui,
};
use gtk::prelude::*;
use serde_json::Value;
use std::{
    cell::RefCell, collections::BTreeMap, process::ExitCode, rc::Rc, sync::Arc,
    sync::atomic::AtomicU64, time::Duration,
};

type Handler = Rc<dyn Fn(&Value) -> bool>;

pub struct App {
    pub cfg: AppConfig,
    pub tmux: TmuxCtl,
    pub guard: WriteGuard,
    pub jobs: Jobs,
    pub dash: DashClient,
    pub window: gtk::ApplicationWindow,
    pub paned: gtk::Paned,
    pub notebook: gtk::Notebook,
    pub webview: webkit2gtk::WebView,
    handlers: RefCell<BTreeMap<&'static str, Handler>>,
}

struct AppRuntime {
    app: Rc<App>,
    poller: Poller,
    poll_source: glib::source::SourceId,
}

impl AppRuntime {
    fn shutdown(self) {
        let AppRuntime {
            app,
            poller,
            poll_source,
        } = self;
        poll_source.remove();
        poller.stop();
        drop(app);
    }
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: AppConfig,
        tmux: TmuxCtl,
        guard: WriteGuard,
        jobs: Jobs,
        dash: DashClient,
        window: gtk::ApplicationWindow,
        paned: gtk::Paned,
        notebook: gtk::Notebook,
        webview: webkit2gtk::WebView,
    ) -> Rc<Self> {
        Rc::new(Self {
            cfg,
            tmux,
            guard,
            jobs,
            dash,
            window,
            paned,
            notebook,
            webview,
            handlers: RefCell::new(BTreeMap::new()),
        })
    }

    pub fn install_handler(&self, name: &'static str, callback: Handler) {
        self.handlers.borrow_mut().insert(name, callback);
    }

    pub fn dispatch(&self, message: &Value) -> bool {
        self.handlers
            .borrow()
            .values()
            .any(|handler| handler(message))
    }
}

pub fn run(args: &[String], default_live: bool) -> ExitCode {
    let cfg = match config::parse_args(args, default_live, &|k| std::env::var(k).ok()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("comandos-app: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = gtk::init() {
        eprintln!("comandos-app: GTK no inicializó: {e}");
        return ExitCode::from(2);
    }
    let application =
        gtk::Application::new(Some("comandos.desktop"), gio::ApplicationFlags::empty());
    let cfg_for_activate = cfg.clone();
    application.connect_activate(move |application| {
        let display = std::env::var("DISPLAY").unwrap_or_default();
        let guard = WriteGuard::from_config(&cfg_for_activate, &display);
        let Ok(tmux) = TmuxCtl::from_config(&cfg_for_activate, &|k| std::env::var(k).ok()) else {
            application.quit();
            return;
        };
        let _ = tmux.prepare_socket_dir(&guard);
        let Ok(dash) = DashClient::new(cfg_for_activate.dash_url(), cfg_for_activate.mode()) else {
            application.quit();
            return;
        };
        let window = ui::window::create(application, &cfg_for_activate);
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_wide_handle(true);
        let notebook = gtk::Notebook::new();
        notebook.set_hexpand(true);
        notebook.set_vexpand(true);
        populate_initial_tabs(&notebook, &cfg_for_activate, &guard);
        let webview = match ui::webview::create(&cfg_for_activate) {
            Ok(webview) => webview,
            Err(e) => {
                eprintln!("comandos-app: WebKit no disponible: {e:?}");
                application.quit();
                return;
            }
        };
        paned.pack1(&webview, true, false);
        paned.pack2(&notebook, true, false);
        window.add(&paned);
        let app = App::new(
            cfg_for_activate.clone(),
            tmux,
            guard,
            Jobs::new(2, "app"),
            dash.clone(),
            window.clone(),
            paned,
            notebook,
            webview,
        );
        // Registro T13: bridge de mensajes WebKit.
        // Registro T14: terminal quick/local.
        // Registro T15: extensiones.
        // Registro T16: snapshots visuales.
        // Registro T17: acciones de operador.
        // Registro T18: notificaciones nativas.
        let generation = Arc::new(AtomicU64::new(0));
        let (poller, rx) = Poller::start(dash, generation);
        let app_for_poll = Rc::clone(&app);
        let poll_source = glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok(update) = rx.try_recv() {
                let _ = app_for_poll.dispatch(&poll_update_message(update));
            }
            glib::ControlFlow::Continue
        });
        let runtime = Rc::new(RefCell::new(Some(AppRuntime {
            app,
            poller,
            poll_source,
        })));
        let runtime_for_delete = Rc::clone(&runtime);
        window.connect_delete_event(move |_, _| {
            if let Some(runtime) = runtime_for_delete.borrow_mut().take() {
                runtime.shutdown();
            }
            glib::Propagation::Proceed
        });
        window.show_all();
    });
    application.run();
    ExitCode::SUCCESS
}

fn populate_initial_tabs(notebook: &gtk::Notebook, cfg: &AppConfig, guard: &WriteGuard) {
    let state = StateFiles::new(cfg.clone(), guard.clone());
    let tabs = state
        .read("app-tabs.json")
        .ok()
        .and_then(|value| TabRegistry::from_json(&value).ok());
    let mut inserted = false;
    if let Some(tabs) = tabs {
        let labels = tabs.to_json();
        for key in tabs.ordered_keys() {
            let label = labels
                .get(&key)
                .and_then(Value::as_str)
                .filter(|label| !label.is_empty())
                .unwrap_or(&key);
            append_placeholder_tab(notebook, &key, label);
            inserted = true;
        }
    }
    if !inserted {
        append_placeholder_tab(notebook, "local", "Local");
    }
}

fn append_placeholder_tab(notebook: &gtk::Notebook, key: &str, label: &str) {
    let child = gtk::Label::new(Some(label));
    child.set_hexpand(true);
    child.set_vexpand(true);
    child.set_widget_name(&format!("tab-{key}"));
    let tab = ui::tab_label::tab_label(label, false);
    notebook.append_page(&child, Some(&tab));
}

fn poll_update_message(update: crate::poll::PollUpdate) -> Value {
    match update {
        crate::poll::PollUpdate::State(value) => serde_json::json!({
            "type": "poll.state",
            "value": value,
        }),
        crate::poll::PollUpdate::Prefs {
            value,
            favorite_generation,
        } => serde_json::json!({
            "type": "poll.prefs",
            "value": value,
            "favorite_generation": favorite_generation,
        }),
        crate::poll::PollUpdate::Notices { revision, badge } => serde_json::json!({
            "type": "poll.notices",
            "revision": revision,
            "badge": badge,
        }),
        crate::poll::PollUpdate::Workspace(value) => serde_json::json!({
            "type": "poll.workspace",
            "value": value,
        }),
        crate::poll::PollUpdate::Marks(value) => serde_json::json!({
            "type": "poll.marks",
            "value": value,
        }),
    }
}
