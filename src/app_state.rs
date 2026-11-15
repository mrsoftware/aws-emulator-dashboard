use crate::aws::client::EmulatorConnection;
use crate::aws::manage::{MessageInfo, ObjectInfo};
use crate::catalog::Sort;
use crate::emulator::process::{EmulatorManager, StartupInfo};
use crate::emulator::proxy::{ProxyManager, TrafficEntry};
use crate::settings::Settings;
use crate::views::text_field::TextField;
use gpui_kit::{AppContext, Context, Entity};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct ServiceResources {
    pub loading: bool,
    pub error: Option<String>,
    pub items: Vec<Resource>,
}

#[derive(Clone, Default)]
pub struct Resource {
    pub name: String,
    pub detail: String,
}

#[derive(Clone, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected(ConnectionInfo),
    Error(String),
}

#[derive(Clone)]
pub struct ConnectionInfo {
    pub endpoint: String,
    pub ca_path: std::path::PathBuf,
}

#[derive(Clone, Default)]
pub struct EmulatorUiState {
    pub busy: bool,
    pub running: bool,
    pub startup: Option<StartupInfo>,
    pub started_at: Option<std::time::Instant>,
    pub last_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct DetailState {
    pub loading: bool,
    pub error: Option<String>,
    pub objects: Vec<ObjectInfo>,
    pub items: Vec<String>,
    pub key_schema: Vec<(String, String)>,
    pub messages: Vec<MessageInfo>,
    pub children: Vec<Resource>,
    /// What a read-only operation answered: log events, records, query rows, a dashboard
    /// body. Replaced by the next operation, cleared when the selection changes.
    pub output: Vec<String>,
    /// A credential the service only ever returns once, e.g. an access key secret. Shown in
    /// the detail card until dismissed; never written to a log or to disk.
    pub revealed: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum InstanceTab {
    #[default]
    All,
    Running,
    Stopped,
    Terminated,
}

impl InstanceTab {
    pub const ALL: [InstanceTab; 4] = [
        InstanceTab::All,
        InstanceTab::Running,
        InstanceTab::Stopped,
        InstanceTab::Terminated,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InstanceTab::All => "All",
            InstanceTab::Running => "Running",
            InstanceTab::Stopped => "Stopped",
            InstanceTab::Terminated => "Terminated",
        }
    }

    pub fn matches(self, state: &str) -> bool {
        match self {
            InstanceTab::All => true,
            InstanceTab::Running => state == "running" || state == "pending",
            InstanceTab::Stopped => state == "stopped" || state == "stopping",
            InstanceTab::Terminated => state == "terminated",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Dialog {
    CreateBucket,
    CreateTable,
    CreateQueue,
    /// Creates on whatever page is open: the resource name is the only field it collects.
    CreateNamed,
    /// A secret is named *and* valued; the emulator rejects a create with no value.
    CreateSecret,
    /// A parameter is named, valued and typed; the type defaults to String.
    CreateParameter,
    PutItem,
    SendMessage,
    LaunchInstance,
    Publish,
    Subscribe,
    /// An SDK call from `views::operations`. The operation's own descriptor says which
    /// fields it collects, so one variant covers every operation the dashboard exposes; the
    /// descriptor itself is in `AppState::dialog_op`.
    Operation,
}

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub connection: ConnectionState,
    pub resources: HashMap<String, ServiceResources>,
    pub selected_service: String,
    pub manager: Arc<EmulatorManager>,
    pub emulator: EmulatorUiState,
    pub proxy: Arc<ProxyManager>,
    pub proxy_running: bool,
    pub proxy_error: Option<String>,
    pub traffic: VecDeque<TrafficEntry>,
    pub selected_resource: Option<String>,
    pub catalog_sort: Sort,
    pub catalog_grid: bool,
    /// Which `ALL SERVICES` categories the sidebar has expanded. Collapsed by default: a
    /// category can hold 60 services, so the nav only earns height when asked.
    pub open_groups: Vec<String>,
    pub instance_tab: InstanceTab,
    pub instance_menu: Option<String>,
    pub instances: Vec<crate::aws::drivers::ec2::InstanceInfo>,
    pub instances_error: Option<String>,
    pub detail: DetailState,
    /// The sessions this dashboard's STS workbench minted and verified this run.
    /// STS has no list operation, so the wire cannot answer for them; what the
    /// page shows is what this dashboard minted itself.
    pub minted_sessions: Vec<crate::aws::drivers::sts::MintedSession>,
    pub dialog: Option<Dialog>,
    /// Which operation `Dialog::Operation` is collecting arguments for.
    pub dialog_op: Option<crate::views::operations::Operation>,
    pub action_busy: bool,
    pub action_error: Option<String>,
    pub input_name: Entity<TextField>,
    pub input_pk: Entity<TextField>,
    pub input_sk: Entity<TextField>,
    pub input_json: Entity<TextField>,
    pub input_body: Entity<TextField>,
    pub input_protocol: Entity<TextField>,
    pub input_endpoint: Entity<TextField>,
    pub input_search: Entity<TextField>,
    pub input_filter: Entity<TextField>,
    pub input_instance_type: Entity<TextField>,
    /// The arguments an operation dialog collects. `views::operations` names and hints each
    /// one when the dialog opens, so a single set of fields serves every operation.
    pub input_ops: [Entity<TextField>; 4],
}

impl AppState {
    pub fn new(settings: Settings, cx: &mut Context<Self>) -> Self {
        Self {
            settings,
            connection: ConnectionState::Disconnected,
            resources: HashMap::new(),
            selected_service: "overview".to_string(),
            manager: Arc::new(EmulatorManager::default()),
            emulator: EmulatorUiState::default(),
            proxy: Arc::new(ProxyManager::default()),
            proxy_running: false,
            proxy_error: None,
            traffic: VecDeque::new(),
            selected_resource: None,
            catalog_sort: Sort::default(),
            catalog_grid: true,
            open_groups: Vec::new(),
            instance_tab: InstanceTab::default(),
            instance_menu: None,
            instances: Vec::new(),
            instances_error: None,
            detail: DetailState::default(),
            minted_sessions: Vec::new(),
            dialog: None,
            dialog_op: None,
            action_busy: false,
            action_error: None,
            input_name: cx.new(|cx| TextField::new("name", cx)),
            input_pk: cx.new(|cx| TextField::new("partition key, e.g. id", cx)),
            input_sk: cx.new(|cx| TextField::new("sort key (optional)", cx)),
            input_json: cx.new(|cx| TextField::new(r#"{"id": "1", ...}"#, cx)),
            input_body: cx.new(|cx| TextField::new("message body", cx)),
            input_protocol: cx.new(|cx| TextField::new("sqs, email, sms, http…", cx)),
            input_endpoint: cx.new(|cx| TextField::new("queue ARN or address", cx)),
            input_search: cx.new(|cx| TextField::new("search services", cx)),
            input_filter: cx.new(|cx| TextField::new("filter by id, name or type", cx)),
            input_instance_type: cx.new(|cx| TextField::new("t3.micro", cx)),
            input_ops: [
                cx.new(|cx| TextField::new("argument", cx)),
                cx.new(|cx| TextField::new("argument", cx)),
                cx.new(|cx| TextField::new("argument", cx)),
                cx.new(|cx| TextField::new("argument", cx)),
            ],
        }
    }

    pub fn service(&self, name: &str) -> &ServiceResources {
        static EMPTY: std::sync::OnceLock<ServiceResources> = std::sync::OnceLock::new();
        self.resources
            .get(name)
            .unwrap_or_else(|| EMPTY.get_or_init(ServiceResources::default))
    }

    pub fn selected_resource(&self) -> Option<Resource> {
        let name = self.selected_resource.as_ref()?;
        self.service(&self.selected_service)
            .items
            .iter()
            .find(|r| &r.name == name)
            .cloned()
    }

    /// Open a page. Every route into a page comes through here so a switch always drops the
    /// row selection and any half-typed dialog with it.
    pub fn open_page(&mut self, key: &str, cx: &mut Context<Self>) {
        self.selected_service = key.to_string();
        self.selected_resource = None;
        self.detail = DetailState::default();
        self.dialog = None;
        self.dialog_op = None;
        cx.notify();
    }

    pub fn toggle_group(&mut self, name: &str, cx: &mut Context<Self>) {
        match self.open_groups.iter().position(|g| g == name) {
            Some(i) => {
                self.open_groups.remove(i);
            }
            None => self.open_groups.push(name.to_string()),
        }
        cx.notify();
    }
}

pub fn connection_for(s: &AppState) -> Option<EmulatorConnection> {
    match s.connection {
        ConnectionState::Connected(_) => {
            let mut conn = EmulatorConnection::from_port(s.settings.default_port);
            if s.settings.proxy_enabled && s.proxy_running {
                conn.endpoint = format!("http://127.0.0.1:{}", s.settings.proxy_port);
            }
            Some(conn)
        }
        _ => None,
    }
}
