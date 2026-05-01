use crate::{
    appearance::Appearance, root_view::SubshellCommandArg, settings::MonolithSettings,
    terminal::shell::ShellType, workspace::WorkspaceAction,
};
use serde::Deserialize;
use settings::Setting as _;
use std::{collections::HashSet, path::Path};
use warp_core::ui::Icon;
use warpui::clipboard::ClipboardContent;
use warpui::elements::{
    Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, Element, Fill, Flex, Hoverable, MainAxisAlignment, MainAxisSize,
    MouseStateHandle, Padding, ParentElement, Radius, ScrollbarWidth, Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext};

const OPEN_SUBSHELL_ACTION: &str =
    "root_view:open_new_tab_insert_subshell_command_and_bootstrap_if_supported";
const STAGING_API_URL: &str = "https://raava-fleet-api-staging-lmbn6fkciq-ue.a.run.app";
const PROD_API_URL: &str = "https://api.fleetos.raavasolutions.com";
const GCP_PROJECT: &str = "raava-481318";
const STAGING_PROFILE_PATH: &str =
    "/Users/master/projects/warp-monolith/examples/monolith-cockpit-profile.live.json";
const PROD_PROFILE_PATH: &str =
    "/Users/master/projects/warp-monolith/examples/monolith-cockpit-profile.prod.json";

#[derive(Clone, Debug, Deserialize)]
struct RuntimeProfile {
    name: String,
    status: String,
    workdir: String,
    git_ref: String,
    #[serde(default)]
    service_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct HostProfile {
    name: String,
    zone: String,
    status: String,
    #[serde(default)]
    project: Option<String>,
    runtimes: Vec<RuntimeProfile>,
}

#[derive(Clone, Debug, Deserialize)]
struct TenantProfile {
    name: String,
    environment: String,
    hosts: Vec<HostProfile>,
}

#[derive(Clone, Debug, Deserialize)]
struct CockpitProfile {
    tenants: Vec<TenantProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SelectedRuntimeScope {
    tenant_name: String,
    host_name: String,
    runtime_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SelectedHostScope {
    tenant_name: String,
    host_name: String,
}

#[derive(Clone, Debug)]
pub enum MonolithCockpitAction {
    OpenCommand {
        command: String,
    },
    OpenSshWorkbench {
        command: String,
    },
    RunTerminalCommand {
        command: String,
    },
    StartTenantChat {
        tenant_name: String,
        prompt: String,
    },
    CopyTenantContext {
        tenant_name: String,
        context: String,
    },
    CopyRuntimeContext {
        scope: SelectedRuntimeScope,
        context: String,
    },
    SelectRuntime {
        tenant_name: String,
        host_name: String,
        runtime_name: String,
    },
    SelectHost {
        tenant_name: String,
        host_name: String,
    },
    ClearSelectedRuntime,
    ClearSelectedHost,
    ClearSelectedTenant,
    ShowTenantFilter {
        filter: TenantFilter,
    },
    ExpandAllTenants {
        tenant_names: Vec<String>,
    },
    CollapseAllTenants,
    ToggleTenant {
        tenant_name: String,
    },
    SwitchEnvironment {
        environment: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantFilter {
    All,
    Active,
    Offboarded,
    WithVms,
    RunningAgents,
    DownAgents,
}

pub struct MonolithCockpitView {
    button_mouse_states: Vec<MouseStateHandle>,
    tenant_mouse_states: Vec<MouseStateHandle>,
    host_mouse_states: Vec<MouseStateHandle>,
    runtime_mouse_states: Vec<MouseStateHandle>,
    environment_mouse_states: Vec<MouseStateHandle>,
    cloud_mouse_states: Vec<MouseStateHandle>,
    filter_mouse_states: Vec<MouseStateHandle>,
    fleet_control_mouse_states: Vec<MouseStateHandle>,
    tenant_context_mouse_states: Vec<MouseStateHandle>,
    scroll_state: ClippedScrollStateHandle,
    expanded_tenants: HashSet<String>,
    tenant_filter: TenantFilter,
    selected_tenant: Option<String>,
    selected_host: Option<SelectedHostScope>,
    selected_runtime: Option<SelectedRuntimeScope>,
}

impl MonolithCockpitView {
    pub fn new(_: &mut ViewContext<Self>) -> Self {
        Self {
            button_mouse_states: (0..512).map(|_| MouseStateHandle::default()).collect(),
            tenant_mouse_states: (0..128).map(|_| MouseStateHandle::default()).collect(),
            host_mouse_states: (0..256).map(|_| MouseStateHandle::default()).collect(),
            runtime_mouse_states: (0..512).map(|_| MouseStateHandle::default()).collect(),
            environment_mouse_states: (0..2).map(|_| MouseStateHandle::default()).collect(),
            cloud_mouse_states: (0..16).map(|_| MouseStateHandle::default()).collect(),
            filter_mouse_states: (0..16).map(|_| MouseStateHandle::default()).collect(),
            fleet_control_mouse_states: (0..8).map(|_| MouseStateHandle::default()).collect(),
            tenant_context_mouse_states: (0..4).map(|_| MouseStateHandle::default()).collect(),
            scroll_state: ClippedScrollStateHandle::default(),
            expanded_tenants: HashSet::new(),
            tenant_filter: TenantFilter::WithVms,
            selected_tenant: None,
            selected_host: None,
            selected_runtime: None,
        }
    }

    fn default_profile() -> CockpitProfile {
        CockpitProfile {
            tenants: Vec::new(),
        }
    }

    fn load_profile(app: &AppContext) -> (CockpitProfile, Option<String>) {
        let profile_path = MonolithSettings::as_ref(app).cockpit_profile_path.value();
        if profile_path.trim().is_empty() {
            return (Self::default_profile(), None);
        }

        match std::fs::read_to_string(Path::new(profile_path)) {
            Ok(contents) => match serde_json::from_str::<CockpitProfile>(&contents) {
                Ok(profile) => (profile, Some(format!("profile: {profile_path}"))),
                Err(error) => (
                    Self::default_profile(),
                    Some(format!("profile parse failed: {error}")),
                ),
            },
            Err(error) => (
                Self::default_profile(),
                Some(format!("profile read failed: {error}")),
            ),
        }
    }

    fn shell_escape(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn gcloud_ssh_prefix(host: &HostProfile) -> String {
        let mut command = format!("gcloud compute ssh {}", Self::shell_escape(&host.name));
        if !host.zone.trim().is_empty() {
            command.push_str(&format!(" --zone {}", Self::shell_escape(&host.zone)));
        }
        if let Some(project) = host
            .project
            .as_ref()
            .filter(|project| !project.trim().is_empty())
        {
            command.push_str(&format!(" --project {}", Self::shell_escape(project)));
        }
        command
    }

    fn remote_command(host: &HostProfile, command: &str) -> String {
        format!(
            "{} --command {}",
            Self::gcloud_ssh_prefix(host),
            Self::shell_escape(command)
        )
    }

    fn remote_runtime_cd_command(workdir: &str) -> String {
        format!(
            "cd {} && printf '\\nMonolith runtime: %s\\n' \"$PWD\"",
            Self::shell_escape(workdir)
        )
    }

    fn remote_file_browse_command(host: &HostProfile, workdir: &str) -> String {
        Self::remote_command(
            host,
            &format!(
                "cd {} && printf '\\nMonolith files: %s\\n\\n' \"$PWD\" && (command -v tree >/dev/null 2>&1 && tree -a -L 2 -I '.git|node_modules|__pycache__|.venv' || find . -maxdepth 2 -not -path './.git/*' -not -path './node_modules/*' -not -path './__pycache__/*' -not -path './.venv/*' | sort | sed 's#^./##') && printf '\\nReady in %s\\n' \"$PWD\" && exec ${{SHELL:-bash}} -l",
                Self::shell_escape(workdir)
            ),
        )
    }

    fn runtime_agent_prompt(
        tenant: &TenantProfile,
        host: &HostProfile,
        runtime: &RuntimeProfile,
        service_name: &str,
    ) -> String {
        format!(
            "/agent You are operating inside a Monolith VM runtime.\n\
Tenant: {}\n\
Tenant status: {}\n\
VM: {}\n\
Zone: {}\n\
Project: {}\n\
Runtime: {}\n\
Runtime status: {}\n\
Workdir: {}\n\
Git ref: {}\n\
Service: {}\n\n\
Start by opening an SSH workbench if it is not already open:\n{}\n\
Then switch the active remote session to the runtime directory:\n{}\n\n\
Work only in the runtime workdir unless I explicitly widen scope. First inspect files, git status, service status, and recent logs. Do not deploy, restart, start, pause, or mutate production without showing the exact command and getting explicit confirmation.",
            tenant.name,
            tenant.environment,
            host.name,
            host.zone,
            host.project.as_deref().unwrap_or(GCP_PROJECT),
            runtime.name,
            runtime.status,
            runtime.workdir,
            runtime.git_ref,
            service_name,
            Self::gcloud_ssh_prefix(host),
            Self::remote_runtime_cd_command(&runtime.workdir)
        )
    }

    fn runtime_diagnosis_prompt(
        tenant: &TenantProfile,
        host: &HostProfile,
        runtime: &RuntimeProfile,
        service_name: &str,
    ) -> String {
        format!(
            "/agent Diagnose this Monolith runtime incident from inside Warp.\n\
Tenant: {}\n\
Tenant status: {}\n\
VM: {}\n\
Zone: {}\n\
Project: {}\n\
Runtime: {}\n\
Runtime status: {}\n\
Workdir: {}\n\
Git ref: {}\n\
Service: {}\n\n\
Open or use the SSH workbench:\n{}\n\
Then move to the runtime directory:\n{}\n\n\
Run a read-only diagnosis first: pwd, git status --short --branch, systemctl --user status {}, recent journal logs, process list, disk space, and relevant env/config checks. Summarize the likely cause and propose the safest next command. Do not start, restart, deploy, edit files, or mutate production without explicit confirmation.",
            tenant.name,
            tenant.environment,
            host.name,
            host.zone,
            host.project.as_deref().unwrap_or(GCP_PROJECT),
            runtime.name,
            runtime.status,
            runtime.workdir,
            runtime.git_ref,
            service_name,
            Self::gcloud_ssh_prefix(host),
            Self::remote_runtime_cd_command(&runtime.workdir),
            service_name,
        )
    }

    fn runtime_context(
        tenant: &TenantProfile,
        host: &HostProfile,
        runtime: &RuntimeProfile,
        active_environment: &str,
        api_url: &str,
    ) -> String {
        format!(
            "Tenant: {}\n\
Tenant environment/status: {}\n\
Active cockpit environment: {}\n\
Fleet API: {}\n\
GCP project: {}\n\
VM: {}\n\
Zone: {}\n\
VM status: {}\n\
Runtime: {}\n\
Runtime status: {}\n\
Workdir: {}\n\
Git ref: {}\n\
Service: {}",
            tenant.name,
            tenant.environment,
            active_environment,
            api_url,
            host.project.as_deref().unwrap_or(GCP_PROJECT),
            host.name,
            host.zone,
            host.status,
            runtime.name,
            runtime.status,
            runtime.workdir,
            runtime.git_ref,
            Self::runtime_service_name(runtime),
        )
    }

    fn selected_runtime_profiles<'a>(
        profile: &'a CockpitProfile,
        scope: &SelectedRuntimeScope,
    ) -> Option<(&'a TenantProfile, &'a HostProfile, &'a RuntimeProfile)> {
        let tenant = profile
            .tenants
            .iter()
            .find(|tenant| tenant.name == scope.tenant_name)?;
        let host = tenant
            .hosts
            .iter()
            .find(|host| host.name == scope.host_name)?;
        let runtime = host
            .runtimes
            .iter()
            .find(|runtime| runtime.name == scope.runtime_name)?;

        Some((tenant, host, runtime))
    }

    fn selected_host_profiles<'a>(
        profile: &'a CockpitProfile,
        scope: &SelectedHostScope,
    ) -> Option<(&'a TenantProfile, &'a HostProfile)> {
        let tenant = profile
            .tenants
            .iter()
            .find(|tenant| tenant.name == scope.tenant_name)?;
        let host = tenant
            .hosts
            .iter()
            .find(|host| host.name == scope.host_name)?;

        Some((tenant, host))
    }

    fn tenant_status_label(tenant: &TenantProfile) -> &'static str {
        if tenant.environment.contains("offboarded") {
            "offboarded"
        } else if tenant.environment.contains("active") {
            "active"
        } else {
            "unknown"
        }
    }

    fn tenant_environment_label(tenant: &TenantProfile) -> &'static str {
        if tenant.environment.contains("prod") {
            "prod"
        } else if tenant.environment.contains("staging") {
            "staging"
        } else {
            "env"
        }
    }

    fn runtime_count(tenant: &TenantProfile) -> usize {
        tenant
            .hosts
            .iter()
            .map(|host| host.runtimes.len())
            .sum::<usize>()
    }

    fn running_runtime_count(tenant: &TenantProfile) -> usize {
        tenant
            .hosts
            .iter()
            .flat_map(|host| &host.runtimes)
            .filter(|runtime| runtime.status.contains("running"))
            .count()
    }

    fn tenant_matches_filter(tenant: &TenantProfile, filter: TenantFilter) -> bool {
        match filter {
            TenantFilter::All => true,
            TenantFilter::Active => Self::tenant_status_label(tenant) == "active",
            TenantFilter::Offboarded => Self::tenant_status_label(tenant) == "offboarded",
            TenantFilter::WithVms => !tenant.hosts.is_empty(),
            TenantFilter::RunningAgents => tenant.hosts.iter().any(|host| {
                host.runtimes
                    .iter()
                    .any(|runtime| Self::runtime_matches_filter(runtime, filter))
            }),
            TenantFilter::DownAgents => tenant.hosts.iter().any(|host| {
                host.runtimes
                    .iter()
                    .any(|runtime| Self::runtime_matches_filter(runtime, filter))
            }),
        }
    }

    fn tenant_filter_label(filter: TenantFilter) -> &'static str {
        match filter {
            TenantFilter::All => "all",
            TenantFilter::Active => "active",
            TenantFilter::Offboarded => "offboarded",
            TenantFilter::WithVms => "with vms",
            TenantFilter::RunningAgents => "running agents",
            TenantFilter::DownAgents => "down agents",
        }
    }

    fn runtime_matches_filter(runtime: &RuntimeProfile, filter: TenantFilter) -> bool {
        match filter {
            TenantFilter::RunningAgents => runtime.status.contains("running"),
            TenantFilter::DownAgents => !runtime.status.contains("running"),
            _ => true,
        }
    }

    fn host_matches_filter(host: &HostProfile, filter: TenantFilter) -> bool {
        match filter {
            TenantFilter::RunningAgents | TenantFilter::DownAgents => host
                .runtimes
                .iter()
                .any(|runtime| Self::runtime_matches_filter(runtime, filter)),
            _ => true,
        }
    }

    fn visible_runtime_count(tenant: &TenantProfile, filter: TenantFilter) -> usize {
        tenant
            .hosts
            .iter()
            .flat_map(|host| &host.runtimes)
            .filter(|runtime| Self::runtime_matches_filter(runtime, filter))
            .count()
    }

    fn visible_host_count(tenant: &TenantProfile, filter: TenantFilter) -> usize {
        tenant
            .hosts
            .iter()
            .filter(|host| Self::host_matches_filter(host, filter))
            .count()
    }

    fn filter_count(profile: &CockpitProfile, filter: TenantFilter) -> usize {
        match filter {
            TenantFilter::RunningAgents => profile
                .tenants
                .iter()
                .map(Self::running_runtime_count)
                .sum::<usize>(),
            TenantFilter::DownAgents => profile
                .tenants
                .iter()
                .map(|tenant| Self::runtime_count(tenant) - Self::running_runtime_count(tenant))
                .sum::<usize>(),
            _ => profile
                .tenants
                .iter()
                .filter(|tenant| Self::tenant_matches_filter(tenant, filter))
                .count(),
        }
    }

    fn cockpit_summary(profile: &CockpitProfile) -> (usize, usize, usize, usize, usize) {
        let tenants = profile.tenants.len();
        let active = profile
            .tenants
            .iter()
            .filter(|tenant| tenant.environment.contains("active"))
            .count();
        let offboarded = profile
            .tenants
            .iter()
            .filter(|tenant| tenant.environment.contains("offboarded"))
            .count();
        let vms = profile
            .tenants
            .iter()
            .map(|tenant| tenant.hosts.len())
            .sum::<usize>();
        let runtimes = profile
            .tenants
            .iter()
            .map(Self::runtime_count)
            .sum::<usize>();

        (tenants, active, offboarded, vms, runtimes)
    }

    fn tenant_chat_prompt(
        tenant: &TenantProfile,
        active_environment: &str,
        api_url: &str,
    ) -> String {
        format!(
            "/agent You are managing one Monolith tenant from the Warp cockpit.\n{}\n\n\
Operate only within this tenant by default. Start read-only: summarize health, risk, and the safest next actions. \
Before any write, show the exact command, target tenant, target VM/runtime, environment, and ask for explicit confirmation. \
Production writes require explicit elevated workflow confirmation.",
            Self::tenant_context(tenant, active_environment, api_url)
        )
    }

    fn tenant_context(tenant: &TenantProfile, active_environment: &str, api_url: &str) -> String {
        let host_lines = if tenant.hosts.is_empty() {
            "- no VMs listed in the current cockpit profile".to_string()
        } else {
            tenant
                .hosts
                .iter()
                .map(|host| {
                    let runtime_names = if host.runtimes.is_empty() {
                        "no runtimes".to_string()
                    } else {
                        host.runtimes
                            .iter()
                            .map(|runtime| {
                                format!("{}:{}:{}", runtime.name, runtime.status, runtime.workdir)
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    format!(
                        "- {} zone={} status={} runtimes=[{}]",
                        host.name, host.zone, host.status, runtime_names
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            "Tenant: {}\n\
Tenant environment/status: {}\n\
Active cockpit environment: {}\n\
Fleet API: {}\n\
GCP project: {}\n\
VMs and runtimes:\n{}",
            tenant.name, tenant.environment, active_environment, api_url, GCP_PROJECT, host_lines
        )
    }

    fn runtime_service_name(runtime: &RuntimeProfile) -> String {
        runtime
            .service_name
            .clone()
            .unwrap_or_else(|| format!("monolith-agent-{}", runtime.name))
    }

    fn section_label(label: &str, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        Text::new(label.to_string(), appearance.ui_font_family(), 11.)
            .with_color(theme.disabled_ui_text_color().into_solid())
            .with_style(Properties::default().weight(Weight::Semibold))
            .finish()
    }

    fn muted_text(value: impl Into<String>, size: f32, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        Text::new(value.into(), appearance.ui_font_family(), size)
            .with_color(theme.disabled_ui_text_color().into_solid())
            .finish()
    }

    fn meta_text(value: impl Into<String>, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        Text::new(value.into(), appearance.ui_font_family(), 12.)
            .with_color(theme.nonactive_ui_text_color().into_solid())
            .finish()
    }

    fn next_mouse_state(
        mouse_states: &[MouseStateHandle],
        button_index: &mut usize,
    ) -> MouseStateHandle {
        let index = *button_index;
        *button_index += 1;
        mouse_states.get(index).cloned().unwrap_or_default()
    }

    fn action_button(
        label: &str,
        command: String,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let button = Container::new(
            Text::new(label.to_string(), appearance.ui_font_family(), 11.)
                .with_color(theme.active_ui_text_color().into_solid())
                .finish(),
        )
        .with_padding(Padding::uniform(3.).with_left(7.).with_right(7.))
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish();

        Hoverable::new(mouse_state, |_| button)
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(MonolithCockpitAction::OpenCommand {
                    command: command.clone(),
                });
            })
            .finish()
    }

    fn ssh_workbench_button(
        label: &str,
        command: String,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        Self::typed_button(
            label,
            MonolithCockpitAction::OpenSshWorkbench { command },
            mouse_state,
            app,
        )
    }

    fn terminal_command_button(
        label: &str,
        command: String,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        Self::typed_button(
            label,
            MonolithCockpitAction::RunTerminalCommand { command },
            mouse_state,
            app,
        )
    }

    fn typed_button(
        label: &str,
        action: MonolithCockpitAction,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let button = Container::new(
            Text::new(label.to_string(), appearance.ui_font_family(), 11.)
                .with_color(theme.active_ui_text_color().into_solid())
                .finish(),
        )
        .with_padding(Padding::uniform(3.).with_left(7.).with_right(7.))
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish();

        Hoverable::new(mouse_state, |_| button)
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(action.clone());
            })
            .finish()
    }

    fn primary_typed_button(
        label: &str,
        action: MonolithCockpitAction,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let button = Container::new(
            Text::new(label.to_string(), appearance.ui_font_family(), 11.)
                .with_color(theme.active_ui_text_color().into_solid())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_padding(Padding::uniform(4.).with_left(8.).with_right(8.))
        .with_background(theme.surface_2())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish();

        Hoverable::new(mouse_state, |_| button)
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(action.clone());
            })
            .finish()
    }

    fn tenant_filter_button(
        label: &str,
        filter: TenantFilter,
        is_active: bool,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let mut button = Container::new(
            Text::new(label.to_string(), appearance.ui_font_family(), 11.)
                .with_color(theme.active_ui_text_color().into_solid())
                .with_style(Properties::default().weight(if is_active {
                    Weight::Semibold
                } else {
                    Weight::Normal
                }))
                .finish(),
        )
        .with_padding(Padding::uniform(3.).with_left(7.).with_right(7.))
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

        if is_active {
            button = button
                .with_background(theme.surface_3())
                .with_border(Border::all(1.).with_border_fill(theme.surface_3()));
        }

        let button = button.finish();

        Hoverable::new(mouse_state, |_| button)
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(MonolithCockpitAction::ShowTenantFilter { filter });
            })
            .finish()
    }

    fn status_chip(label: &str, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        Container::new(
            Text::new(label.to_string(), appearance.ui_font_family(), 10.)
                .with_color(theme.nonactive_ui_text_color().into_solid())
                .finish(),
        )
        .with_padding(Padding::uniform(2.).with_left(5.).with_right(5.))
        .with_background(theme.surface_2())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish()
    }

    fn environment_button(
        label: &str,
        environment: &str,
        is_active: bool,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let environment = environment.to_string();

        let mut button = Container::new(
            Text::new(label.to_string(), appearance.ui_font_family(), 11.)
                .with_color(theme.active_ui_text_color().into_solid())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_padding(Padding::uniform(5.).with_left(9.).with_right(9.))
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

        if is_active {
            button = button.with_background(theme.surface_3());
        }

        Hoverable::new(mouse_state, |_| button.finish())
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(MonolithCockpitAction::SwitchEnvironment {
                    environment: environment.clone(),
                });
            })
            .finish()
    }

    fn render_environment_switcher(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let settings = MonolithSettings::as_ref(app);
        let active_environment = settings.cockpit_environment.value();
        let api_url = settings.api_url.value();
        let is_prod = active_environment == "prod";

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(7.)
            .with_child(
                Flex::row()
                    .with_spacing(6.)
                    .with_child(Self::environment_button(
                        "staging",
                        "staging",
                        !is_prod,
                        self.environment_mouse_states
                            .first()
                            .cloned()
                            .unwrap_or_default(),
                        app,
                    ))
                    .with_child(Self::environment_button(
                        "prod",
                        "prod",
                        is_prod,
                        self.environment_mouse_states
                            .get(1)
                            .cloned()
                            .unwrap_or_default(),
                        app,
                    ))
                    .finish(),
            )
            .with_child(
                Text::new(api_url.clone(), appearance.ui_font_family(), 10.)
                    .with_color(
                        Appearance::as_ref(app)
                            .theme()
                            .disabled_ui_text_color()
                            .into_solid(),
                    )
                    .finish(),
            )
            .finish()
    }

    fn render_cloud_toolbar(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let active_environment = MonolithSettings::as_ref(app).cockpit_environment.value();

        let setup_command = format!(
            "gcloud auth login && printf '\\nMonolith cockpit passes --project {} explicitly; it does not mutate global gcloud project or ADC credentials.\\n'",
            Self::shell_escape(GCP_PROJECT),
        );
        let status_command = format!(
            "printf 'account: '; gcloud auth list --filter=status:ACTIVE --format='value(account)'; printf 'project: '; gcloud config get-value project; gcloud compute instances list --project {} --filter={} --format='table(name,zone.basename(),status,labels.raava-tenant,labels.raava-agent)'",
            Self::shell_escape(GCP_PROJECT),
            Self::shell_escape("labels.raava-managed=true"),
        );
        let project_command = format!(
            "printf 'cockpit project: {}\\n'; printf 'global gcloud project: '; gcloud config get-value project",
            Self::shell_escape(GCP_PROJECT),
        );
        let access_check_command = format!(
            "printf 'gcloud account: '; gcloud auth list --filter=status:ACTIVE --format='value(account)'; printf 'cockpit project: {}\\n'; gcloud compute instances list --project {} --filter={} --format='value(name)' >/dev/null && printf 'gcloud vm inventory: ok\\n'; if [ -f ~/.monolith/platform-admin-keys.env ]; then . ~/.monolith/platform-admin-keys.env; if [ {} = prod ]; then api_key=\"$MONOLITH_PROD_PLATFORM_ADMIN_KEY\"; api_url=\"$MONOLITH_PROD_API_URL\"; else api_key=\"$MONOLITH_STAGING_PLATFORM_ADMIN_KEY\"; api_url=\"$MONOLITH_STAGING_API_URL\"; fi; if [ -n \"$api_key\" ]; then curl -fsS -H \"Authorization: Bearer $api_key\" \"$api_url/health\" >/dev/null && printf 'fleet api auth/health: ok\\n' || printf 'fleet api auth/health: failed\\n'; else printf 'fleet api key: missing for {}\\n'; fi; else printf 'local key file: missing ~/.monolith/platform-admin-keys.env\\n'; fi",
            Self::shell_escape(GCP_PROJECT),
            Self::shell_escape(GCP_PROJECT),
            Self::shell_escape("labels.raava-managed=true"),
            Self::shell_escape(active_environment),
            Self::shell_escape(active_environment),
        );

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(6.)
            .with_child(
                Text::new(
                    format!("cloud: gcloud / {}", GCP_PROJECT),
                    appearance.ui_font_family(),
                    10.,
                )
                .with_color(theme.disabled_ui_text_color().into_solid())
                .finish(),
            )
            .with_child(
                Flex::row()
                    .with_spacing(6.)
                    .with_child(Self::action_button(
                        "auth",
                        setup_command,
                        self.cloud_mouse_states.first().cloned().unwrap_or_default(),
                        app,
                    ))
                    .with_child(Self::action_button(
                        "status",
                        status_command,
                        self.cloud_mouse_states.get(1).cloned().unwrap_or_default(),
                        app,
                    ))
                    .finish(),
            )
            .with_child(
                Flex::row()
                    .with_spacing(6.)
                    .with_child(Self::action_button(
                        "project",
                        project_command,
                        self.cloud_mouse_states.get(2).cloned().unwrap_or_default(),
                        app,
                    ))
                    .with_child(Self::action_button(
                        "check access",
                        access_check_command,
                        self.cloud_mouse_states.get(3).cloned().unwrap_or_default(),
                        app,
                    ))
                    .finish(),
            )
            .finish()
    }

    fn render_selected_runtime_workbench(
        &self,
        profile: &CockpitProfile,
        scope: &SelectedRuntimeScope,
        active_environment: &str,
        api_url: &str,
        mouse_states: &[MouseStateHandle],
        button_index: &mut usize,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        let (tenant, host, runtime) = Self::selected_runtime_profiles(profile, scope)?;
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let service_name = Self::runtime_service_name(runtime);
        let runtime_context =
            Self::runtime_context(tenant, host, runtime, active_environment, api_url);
        let diagnosis_prompt = Self::runtime_diagnosis_prompt(tenant, host, runtime, &service_name);
        let agent_prompt = Self::runtime_agent_prompt(tenant, host, runtime, &service_name);
        let ssh_command = Self::gcloud_ssh_prefix(host);
        let cd_command = Self::remote_runtime_cd_command(&runtime.workdir);
        let files_command = Self::remote_file_browse_command(host, &runtime.workdir);
        let git_command = Self::remote_command(
            host,
            &format!(
                "cd {} && git status --short --branch",
                Self::shell_escape(&runtime.workdir)
            ),
        );
        let logs_command = Self::remote_command(
            host,
            &format!(
                "cd {} && (test -d logs && tail -n 200 -f logs/*.log || journalctl --user -u {} -f)",
                Self::shell_escape(&runtime.workdir),
                Self::shell_escape(&service_name),
            ),
        );
        let service_status = Self::remote_command(
            host,
            &format!(
                "cd {} && printf 'runtime: {}\\nservice: {}\\n\\n' && systemctl --user status {} --no-pager || true && printf '\\nrecent logs\\n' && journalctl --user -u {} -n 80 --no-pager || true",
                Self::shell_escape(&runtime.workdir),
                Self::shell_escape(&runtime.name),
                Self::shell_escape(&service_name),
                Self::shell_escape(&service_name),
                Self::shell_escape(&service_name),
            ),
        );
        let prod_locked = tenant.environment.contains("prod");
        let inactive_target = tenant.environment.contains("offboarded")
            || host.status.contains("terminated")
            || host.status.contains("unknown");
        let guard_command = |action: &str, reason: &str| {
            format!(
                "printf '%s\\n' {}",
                Self::shell_escape(&format!(
                    "Monolith cockpit blocked {action} for {}/{}/{}: {reason}",
                    tenant.name, host.name, runtime.name
                ))
            )
        };
        let guarded_service_command = |action: &str, command: String| {
            if prod_locked {
                guard_command(action, "prod writes require explicit elevated workflow")
            } else if inactive_target {
                guard_command(action, "target is offboarded, terminated, or unknown")
            } else {
                command
            }
        };
        let start_command = guarded_service_command(
            "start",
            Self::remote_command(
                host,
                &format!(
                    "systemctl --user start {}",
                    Self::shell_escape(&service_name)
                ),
            ),
        );
        let pause_command = guarded_service_command(
            "pause",
            Self::remote_command(
                host,
                &format!(
                    "systemctl --user stop {}",
                    Self::shell_escape(&service_name)
                ),
            ),
        );
        let restart_command = guarded_service_command(
            "restart",
            Self::remote_command(
                host,
                &format!(
                    "systemctl --user restart {}",
                    Self::shell_escape(&service_name)
                ),
            ),
        );

        Some(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(10.)
                    .with_child(
                        Flex::column()
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .with_spacing(5.)
                            .with_child(Self::section_label("RUNTIME", app))
                            .with_child(
                                Text::new(runtime.name.clone(), appearance.ui_font_family(), 14.)
                                    .with_color(theme.active_ui_text_color().into_solid())
                                    .with_style(Properties::default().weight(Weight::Semibold))
                                    .finish(),
                            )
                            .with_child(Self::muted_text(
                                format!("{} / {} / {}", tenant.name, host.name, runtime.workdir),
                                11.,
                                app,
                            ))
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(Self::status_chip(
                                &format!("status {}", runtime.status),
                                app,
                            ))
                            .with_child(Self::status_chip(
                                &format!("service {}", service_name),
                                app,
                            ))
                            .with_child(Self::status_chip(&format!("zone {}", host.zone), app))
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(Self::primary_typed_button(
                                "diagnose",
                                MonolithCockpitAction::StartTenantChat {
                                    tenant_name: tenant.name.clone(),
                                    prompt: diagnosis_prompt,
                                },
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::typed_button(
                                "agent",
                                MonolithCockpitAction::StartTenantChat {
                                    tenant_name: tenant.name.clone(),
                                    prompt: agent_prompt,
                                },
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::ssh_workbench_button(
                                "ssh",
                                ssh_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::terminal_command_button(
                                "cd",
                                cd_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::action_button(
                                "status",
                                service_status,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(Self::action_button(
                                "logs",
                                logs_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::action_button(
                                "git",
                                git_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::action_button(
                                "files",
                                files_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(Self::action_button(
                                "start",
                                start_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::action_button(
                                "pause",
                                pause_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::action_button(
                                "restart",
                                restart_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(Self::typed_button(
                                "copy context",
                                MonolithCockpitAction::CopyRuntimeContext {
                                    scope: scope.clone(),
                                    context: runtime_context,
                                },
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::typed_button(
                                "back to vm",
                                MonolithCockpitAction::ClearSelectedRuntime,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .finish(),
                    )
                    .finish(),
            )
            .with_padding(Padding::uniform(10.))
            .with_background(theme.surface_1())
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish(),
        )
    }

    fn render_selected_tenant_workbench(
        &self,
        profile: &CockpitProfile,
        active_environment: &str,
        api_url: &str,
        filter: TenantFilter,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        let tenant_name = self.selected_tenant.as_ref()?;
        let tenant = profile
            .tenants
            .iter()
            .find(|tenant| &tenant.name == tenant_name)?;
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let context = Self::tenant_context(tenant, active_environment, api_url);
        let prompt = Self::tenant_chat_prompt(tenant, active_environment, api_url);
        let visible_hosts = tenant
            .hosts
            .iter()
            .filter(|host| Self::host_matches_filter(host, filter))
            .collect::<Vec<_>>();

        let mut host_list = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(8.);
        for host in &visible_hosts {
            host_list.add_child(Self::host_summary_row(host, filter, app));
        }

        Some(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(10.)
                    .with_child(
                        Flex::column()
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .with_spacing(6.)
                            .with_child(Self::section_label("TENANT", app))
                            .with_child(
                                Text::new(tenant.name.clone(), appearance.ui_font_family(), 14.)
                                    .with_color(theme.active_ui_text_color().into_solid())
                                    .with_style(Properties::default().weight(Weight::Semibold))
                                    .finish(),
                            )
                            .with_child(Self::muted_text(
                                format!(
                                    "{} · {} / {} vms visible · {} / {} runtimes running",
                                    tenant.environment,
                                    Self::visible_host_count(tenant, filter),
                                    tenant.hosts.len(),
                                    Self::running_runtime_count(tenant),
                                    Self::runtime_count(tenant)
                                ),
                                11.,
                                app,
                            ))
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(Self::primary_typed_button(
                                "chat",
                                MonolithCockpitAction::StartTenantChat {
                                    tenant_name: tenant.name.clone(),
                                    prompt,
                                },
                                self.tenant_context_mouse_states
                                    .first()
                                    .cloned()
                                    .unwrap_or_default(),
                                app,
                            ))
                            .with_child(Self::typed_button(
                                "copy",
                                MonolithCockpitAction::CopyTenantContext {
                                    tenant_name: tenant.name.clone(),
                                    context,
                                },
                                self.tenant_context_mouse_states
                                    .get(1)
                                    .cloned()
                                    .unwrap_or_default(),
                                app,
                            ))
                            .with_child(Self::typed_button(
                                "exit",
                                MonolithCockpitAction::ClearSelectedTenant,
                                self.tenant_context_mouse_states
                                    .get(2)
                                    .cloned()
                                    .unwrap_or_default(),
                                app,
                            ))
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(Self::status_chip(
                                &format!(
                                    "visible vms {}",
                                    Self::visible_host_count(tenant, filter)
                                ),
                                app,
                            ))
                            .with_child(Self::status_chip(
                                &format!(
                                    "visible runtimes {}",
                                    Self::visible_runtime_count(tenant, filter)
                                ),
                                app,
                            ))
                            .with_child(Self::status_chip(Self::tenant_filter_label(filter), app))
                            .finish(),
                    )
                    .with_child(Self::muted_text(
                        "Choose a VM in the navigator to open the next stage.",
                        12.,
                        app,
                    ))
                    .with_child(if visible_hosts.is_empty() {
                        Self::muted_text("No VMs match the current fleet filter.", 12., app)
                    } else {
                        host_list.finish()
                    })
                    .finish(),
            )
            .with_padding(Padding::uniform(10.))
            .with_background(theme.surface_1())
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish(),
        )
    }

    fn render_selected_host_workbench(
        &self,
        profile: &CockpitProfile,
        filter: TenantFilter,
        mouse_states: &[MouseStateHandle],
        button_index: &mut usize,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        let scope = self.selected_host.as_ref()?;
        let (tenant, host) = Self::selected_host_profiles(profile, scope)?;
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let ssh_command = Self::gcloud_ssh_prefix(host);
        let visible_runtimes = host
            .runtimes
            .iter()
            .filter(|runtime| Self::runtime_matches_filter(runtime, filter))
            .collect::<Vec<_>>();

        let mut runtime_list = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(8.);
        for runtime in &visible_runtimes {
            runtime_list.add_child(Self::runtime_summary_row(
                runtime,
                self.selected_runtime.as_ref().is_some_and(|selected| {
                    selected.host_name == host.name && selected.runtime_name == runtime.name
                }),
                app,
            ));
        }

        let mut status_row = Flex::row()
            .with_spacing(6.)
            .with_child(Self::status_chip(&format!("status {}", host.status), app))
            .with_child(Self::status_chip(
                &format!("visible runtimes {}", visible_runtimes.len()),
                app,
            ));
        if let Some(project) = host.project.as_deref() {
            status_row.add_child(Self::status_chip(&format!("project {project}"), app));
        }

        Some(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(10.)
                    .with_child(
                        Flex::column()
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .with_spacing(6.)
                            .with_child(Self::section_label("VM", app))
                            .with_child(
                                Text::new(host.name.clone(), appearance.ui_font_family(), 14.)
                                    .with_color(theme.active_ui_text_color().into_solid())
                                    .with_style(Properties::default().weight(Weight::Semibold))
                                    .finish(),
                            )
                            .with_child(Self::muted_text(
                                format!("{} / {}", tenant.name, host.zone),
                                11.,
                                app,
                            ))
                            .finish(),
                    )
                    .with_child(status_row.finish())
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(Self::ssh_workbench_button(
                                "ssh",
                                ssh_command,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .with_child(Self::typed_button(
                                "back to tenant",
                                MonolithCockpitAction::ClearSelectedHost,
                                Self::next_mouse_state(mouse_states, button_index),
                                app,
                            ))
                            .finish(),
                    )
                    .with_child(Self::muted_text(
                        "Choose a runtime in the navigator to open the runtime cockpit.",
                        12.,
                        app,
                    ))
                    .with_child(if visible_runtimes.is_empty() {
                        Self::muted_text("No runtimes match the current fleet filter.", 12., app)
                    } else {
                        runtime_list.finish()
                    })
                    .finish(),
            )
            .with_padding(Padding::uniform(10.))
            .with_background(theme.surface_1())
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish(),
        )
    }

    fn render_cockpit_summary(
        &self,
        profile: &CockpitProfile,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let settings = MonolithSettings::as_ref(app);
        let active_environment = settings.cockpit_environment.value();
        let is_prod = active_environment == "prod";
        let environment_label = if is_prod {
            "PRODUCTION COCKPIT"
        } else {
            "STAGING COCKPIT"
        };
        let write_mode = if is_prod {
            "production writes locked behind explicit confirmation"
        } else {
            "staging writes guarded"
        };
        let (tenants, active, offboarded, vms, runtimes) = Self::cockpit_summary(profile);

        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(8.)
                .with_child(
                    Flex::row()
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(Self::section_label(environment_label, app))
                        .with_child(Self::status_chip(&format!("api {active_environment}"), app))
                        .finish(),
                )
                .with_child(
                    Flex::row()
                        .with_spacing(6.)
                        .with_child(Self::status_chip(&format!("tenants {tenants}"), app))
                        .with_child(Self::status_chip(&format!("active {active}"), app))
                        .with_child(Self::status_chip(&format!("offboarded {offboarded}"), app))
                        .finish(),
                )
                .with_child(
                    Flex::row()
                        .with_spacing(6.)
                        .with_child(Self::status_chip(&format!("vms {vms}"), app))
                        .with_child(Self::status_chip(&format!("runtimes {runtimes}"), app))
                        .finish(),
                )
                .with_child(
                    Text::new(
                        format!("operator mode: {write_mode}"),
                        appearance.ui_font_family(),
                        11.,
                    )
                    .with_color(theme.disabled_ui_text_color().into_solid())
                    .finish(),
                )
                .finish(),
        )
        .with_padding(Padding::uniform(10.))
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish()
    }

    fn render_scope_strip(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let tenant = self.selected_tenant.as_deref().unwrap_or("fleet");
        let host = self
            .selected_host
            .as_ref()
            .map(|scope| scope.host_name.as_str())
            .unwrap_or("choose vm");
        let runtime = self
            .selected_runtime
            .as_ref()
            .map(|scope| scope.runtime_name.as_str())
            .unwrap_or("choose runtime");

        let mut actions = Flex::row().with_spacing(6.);
        if self.selected_runtime.is_some() {
            actions.add_child(Self::typed_button(
                "exit runtime",
                MonolithCockpitAction::ClearSelectedRuntime,
                self.tenant_context_mouse_states
                    .first()
                    .cloned()
                    .unwrap_or_default(),
                app,
            ));
        }
        if self.selected_host.is_some() {
            actions.add_child(Self::typed_button(
                "back to tenant",
                MonolithCockpitAction::ClearSelectedHost,
                self.tenant_context_mouse_states
                    .get(1)
                    .cloned()
                    .unwrap_or_default(),
                app,
            ));
        }
        if self.selected_tenant.is_some() {
            actions.add_child(Self::typed_button(
                "clear scope",
                MonolithCockpitAction::ClearSelectedTenant,
                self.tenant_context_mouse_states
                    .get(2)
                    .cloned()
                    .unwrap_or_default(),
                app,
            ));
        }

        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(8.)
                .with_child(Self::section_label("SCOPE", app))
                .with_child(
                    Flex::row()
                        .with_spacing(6.)
                        .with_child(Self::status_chip(&format!("tenant {tenant}"), app))
                        .with_child(Self::status_chip(&format!("vm {host}"), app))
                        .with_child(Self::status_chip(&format!("runtime {runtime}"), app))
                        .finish(),
                )
                .with_child(
                    Text::new(
                        "Navigate top-down: tenant > VM > runtime. Actions appear only at the selected level.",
                        appearance.ui_font_family(),
                        11.,
                    )
                    .with_color(theme.disabled_ui_text_color().into_solid())
                    .finish(),
                )
                .with_child(actions.finish())
                .finish(),
        )
        .with_padding(Padding::uniform(10.))
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish()
    }

    fn stage_empty_state(title: &str, message: &str, app: &AppContext) -> Box<dyn Element> {
        let theme = Appearance::as_ref(app).theme();
        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(6.)
                .with_child(Self::section_label(title, app))
                .with_child(Self::muted_text(message, 12., app))
                .finish(),
        )
        .with_padding(Padding::uniform(8.))
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish()
    }

    fn host_summary_row(
        host: &HostProfile,
        filter: TenantFilter,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(5.)
                .with_child(
                    Flex::row()
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Flex::row()
                                .with_spacing(6.)
                                .with_child(
                                    Text::new("VM", appearance.ui_font_family(), 10.)
                                        .with_color(theme.disabled_ui_text_color().into_solid())
                                        .with_style(Properties::default().weight(Weight::Semibold))
                                        .finish(),
                                )
                                .with_child(
                                    Text::new(host.name.clone(), appearance.ui_font_family(), 12.)
                                        .with_color(theme.active_ui_text_color().into_solid())
                                        .with_style(Properties::default().weight(Weight::Semibold))
                                        .finish(),
                                )
                                .finish(),
                        )
                        .with_child(Self::status_chip(&host.status, app))
                        .finish(),
                )
                .with_child(Self::meta_text(
                    format!(
                        "{} · runtimes {} / {} visible",
                        host.zone,
                        host.runtimes
                            .iter()
                            .filter(|runtime| Self::runtime_matches_filter(runtime, filter))
                            .count(),
                        host.runtimes.len()
                    ),
                    app,
                ))
                .finish(),
        )
        .with_padding(Padding::uniform(8.))
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish()
    }

    fn runtime_summary_row(
        runtime: &RuntimeProfile,
        is_selected: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let service_name = Self::runtime_service_name(runtime);

        let mut container = Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(5.)
                .with_child(
                    Flex::row()
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Text::new(runtime.name.clone(), appearance.ui_font_family(), 12.)
                                .with_color(theme.active_ui_text_color().into_solid())
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                        )
                        .with_child(Self::status_chip(&runtime.status, app))
                        .finish(),
                )
                .with_child(Self::meta_text(runtime.workdir.clone(), app))
                .with_child(Self::meta_text(
                    format!("git {} · {}", runtime.git_ref, service_name),
                    app,
                ))
                .finish(),
        )
        .with_padding(Padding::uniform(8.))
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

        if is_selected {
            container = container.with_background(theme.surface_1());
        }

        container.finish()
    }

    fn host_navigator_row(
        tenant: &TenantProfile,
        host: &HostProfile,
        filter: TenantFilter,
        is_selected: bool,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let tenant_name = tenant.name.clone();
        let host_name = host.name.clone();

        Hoverable::new(mouse_state, |_| {
            let mut container = Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(4.)
                    .with_child(
                        Flex::row()
                            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_child(
                                Text::new(
                                    format!("VM {}", host.name),
                                    appearance.ui_font_family(),
                                    12.,
                                )
                                .with_color(theme.active_ui_text_color().into_solid())
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                            )
                            .with_child(Self::status_chip(&host.status, app))
                            .finish(),
                    )
                    .with_child(Self::meta_text(
                        format!(
                            "{} · runtimes {} / {} visible",
                            host.zone,
                            host.runtimes
                                .iter()
                                .filter(|runtime| Self::runtime_matches_filter(runtime, filter))
                                .count(),
                            host.runtimes.len()
                        ),
                        app,
                    ))
                    .finish(),
            )
            .with_padding(Padding::uniform(6.).with_left(18.))
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

            if is_selected {
                container = container.with_background(theme.surface_1());
            }

            container.finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(MonolithCockpitAction::SelectHost {
                tenant_name: tenant_name.clone(),
                host_name: host_name.clone(),
            });
        })
        .finish()
    }

    fn runtime_navigator_row(
        tenant: &TenantProfile,
        host: &HostProfile,
        runtime: &RuntimeProfile,
        is_selected: bool,
        mouse_state: MouseStateHandle,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let tenant_name = tenant.name.clone();
        let host_name = host.name.clone();
        let runtime_name = runtime.name.clone();

        Hoverable::new(mouse_state, |_| {
            let mut container = Container::new(
                Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Flex::row()
                            .with_spacing(8.)
                            .with_child(
                                ConstrainedBox::new(
                                    Icon::Dataflow
                                        .to_warpui_icon(theme.sub_text_color(theme.background()))
                                        .finish(),
                                )
                                .with_width(14.)
                                .with_height(14.)
                                .finish(),
                            )
                            .with_child(
                                Flex::column()
                                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                                    .with_spacing(3.)
                                    .with_child(
                                        Text::new(
                                            runtime.name.clone(),
                                            appearance.ui_font_family(),
                                            12.,
                                        )
                                        .with_color(theme.active_ui_text_color().into_solid())
                                        .with_style(Properties::default().weight(Weight::Semibold))
                                        .finish(),
                                    )
                                    .with_child(Self::meta_text(
                                        format!("{} · git {}", runtime.workdir, runtime.git_ref),
                                        app,
                                    ))
                                    .finish(),
                            )
                            .finish(),
                    )
                    .with_child(Self::status_chip(&runtime.status, app))
                    .finish(),
            )
            .with_padding(Padding::uniform(6.).with_left(34.))
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

            if is_selected {
                container = container.with_background(theme.surface_1());
            }

            container.finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(MonolithCockpitAction::SelectRuntime {
                tenant_name: tenant_name.clone(),
                host_name: host_name.clone(),
                runtime_name: runtime_name.clone(),
            });
        })
        .finish()
    }

    fn tenant_card(
        tenant: &TenantProfile,
        is_expanded: bool,
        tenant_mouse_state: MouseStateHandle,
        host_mouse_states: &[MouseStateHandle],
        host_index: &mut usize,
        runtime_mouse_states: &[MouseStateHandle],
        runtime_index: &mut usize,
        filter: TenantFilter,
        is_selected: bool,
        selected_host: Option<&SelectedHostScope>,
        selected_runtime: Option<&SelectedRuntimeScope>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let chevron_icon = if is_expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };

        let tenant_name = tenant.name.clone();
        let header = Hoverable::new(tenant_mouse_state, |_| {
            Container::new(
                Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Flex::row()
                            .with_spacing(6.)
                            .with_child(
                                Text::new(tenant.name.clone(), appearance.ui_font_family(), 13.)
                                    .with_color(theme.active_ui_text_color().into_solid())
                                    .with_style(Properties::default().weight(Weight::Semibold))
                                    .finish(),
                            )
                            .with_child(Self::status_chip(
                                Self::tenant_environment_label(tenant),
                                app,
                            ))
                            .with_child(Self::status_chip(Self::tenant_status_label(tenant), app))
                            .finish(),
                    )
                    .with_child(
                        ConstrainedBox::new(
                            chevron_icon
                                .to_warpui_icon(theme.nonactive_ui_text_color())
                                .finish(),
                        )
                        .with_width(14.)
                        .with_height(14.)
                        .finish(),
                    )
                    .finish(),
            )
            .with_padding(Padding::uniform(1.))
            .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(MonolithCockpitAction::ToggleTenant {
                tenant_name: tenant_name.clone(),
            });
        })
        .finish();

        let mut content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(6.)
            .with_child(header);

        if is_expanded {
            content.add_child(Self::muted_text(
                format!(
                    "{} · {} / {} vms visible · {} / {} runtimes visible",
                    tenant.environment,
                    Self::visible_host_count(tenant, filter),
                    tenant.hosts.len(),
                    Self::visible_runtime_count(tenant, filter),
                    Self::runtime_count(tenant)
                ),
                12.,
                app,
            ));

            for host in tenant
                .hosts
                .iter()
                .filter(|host| Self::host_matches_filter(host, filter))
            {
                content.add_child(Self::host_navigator_row(
                    tenant,
                    host,
                    filter,
                    selected_host.is_some_and(|scope| {
                        scope.tenant_name == tenant.name && scope.host_name == host.name
                    }),
                    Self::next_mouse_state(host_mouse_states, host_index),
                    app,
                ));

                for runtime in host
                    .runtimes
                    .iter()
                    .filter(|runtime| Self::runtime_matches_filter(runtime, filter))
                {
                    content.add_child(Self::runtime_navigator_row(
                        tenant,
                        host,
                        runtime,
                        selected_runtime.is_some_and(|scope| {
                            scope.tenant_name == tenant.name
                                && scope.host_name == host.name
                                && scope.runtime_name == runtime.name
                        }),
                        Self::next_mouse_state(runtime_mouse_states, runtime_index),
                        app,
                    ));
                }
            }
        }

        let mut container = Container::new(content.finish())
            .with_padding(Padding::uniform(8.))
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

        if is_selected {
            container = container.with_background(theme.surface_1());
        }

        container.finish()
    }

    fn tenant_is_expanded(&self, tenant: &TenantProfile) -> bool {
        self.expanded_tenants.contains(&tenant.name)
    }
}

impl Entity for MonolithCockpitView {
    type Event = ();
}

impl TypedActionView for MonolithCockpitView {
    type Action = MonolithCockpitAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            MonolithCockpitAction::OpenCommand { command } => ctx.dispatch_global_action(
                OPEN_SUBSHELL_ACTION,
                SubshellCommandArg {
                    command: command.clone(),
                    shell_type: ShellType::from_name("bash"),
                },
            ),
            MonolithCockpitAction::OpenSshWorkbench { command } => {
                ctx.dispatch_global_action(
                    OPEN_SUBSHELL_ACTION,
                    SubshellCommandArg {
                        command: command.clone(),
                        shell_type: ShellType::from_name("bash"),
                    },
                );
                ctx.dispatch_typed_action(&WorkspaceAction::RunCommand(command.clone()));
            }
            MonolithCockpitAction::RunTerminalCommand { command } => {
                ctx.dispatch_typed_action(&WorkspaceAction::RunCommand(command.clone()));
            }
            MonolithCockpitAction::StartTenantChat {
                tenant_name,
                prompt,
            } => {
                self.selected_tenant = Some(tenant_name.clone());
                ctx.dispatch_typed_action(&WorkspaceAction::InsertInInput {
                    content: prompt.clone(),
                    replace_buffer: true,
                    ensure_agent_mode: true,
                });
                ctx.notify();
            }
            MonolithCockpitAction::CopyTenantContext {
                tenant_name,
                context,
            } => {
                self.selected_tenant = Some(tenant_name.clone());
                self.selected_host = None;
                self.selected_runtime = None;
                ctx.clipboard()
                    .write(ClipboardContent::plain_text(context.clone()));
                ctx.notify();
            }
            MonolithCockpitAction::CopyRuntimeContext { scope, context } => {
                self.selected_tenant = Some(scope.tenant_name.clone());
                self.selected_host = Some(SelectedHostScope {
                    tenant_name: scope.tenant_name.clone(),
                    host_name: scope.host_name.clone(),
                });
                self.selected_runtime = Some(scope.clone());
                ctx.clipboard()
                    .write(ClipboardContent::plain_text(context.clone()));
                ctx.notify();
            }
            MonolithCockpitAction::SelectRuntime {
                tenant_name,
                host_name,
                runtime_name,
            } => {
                self.selected_tenant = Some(tenant_name.clone());
                self.selected_host = Some(SelectedHostScope {
                    tenant_name: tenant_name.clone(),
                    host_name: host_name.clone(),
                });
                self.selected_runtime = Some(SelectedRuntimeScope {
                    tenant_name: tenant_name.clone(),
                    host_name: host_name.clone(),
                    runtime_name: runtime_name.clone(),
                });
                self.expanded_tenants.insert(tenant_name.clone());
                ctx.notify();
            }
            MonolithCockpitAction::SelectHost {
                tenant_name,
                host_name,
            } => {
                self.selected_tenant = Some(tenant_name.clone());
                self.selected_host = Some(SelectedHostScope {
                    tenant_name: tenant_name.clone(),
                    host_name: host_name.clone(),
                });
                self.selected_runtime = None;
                self.expanded_tenants.insert(tenant_name.clone());
                ctx.notify();
            }
            MonolithCockpitAction::ClearSelectedTenant => {
                self.selected_tenant = None;
                self.selected_host = None;
                self.selected_runtime = None;
                ctx.notify();
            }
            MonolithCockpitAction::ClearSelectedHost => {
                self.selected_host = None;
                self.selected_runtime = None;
                ctx.notify();
            }
            MonolithCockpitAction::ClearSelectedRuntime => {
                self.selected_runtime = None;
                ctx.notify();
            }
            MonolithCockpitAction::ShowTenantFilter { filter } => {
                self.tenant_filter = *filter;
                let (profile, _) = Self::load_profile(ctx);
                self.expanded_tenants = match filter {
                    TenantFilter::All => HashSet::new(),
                    TenantFilter::Active | TenantFilter::Offboarded => profile
                        .tenants
                        .iter()
                        .filter(|tenant| {
                            Self::tenant_matches_filter(tenant, *filter) && !tenant.hosts.is_empty()
                        })
                        .map(|tenant| tenant.name.clone())
                        .collect(),
                    TenantFilter::WithVms
                    | TenantFilter::RunningAgents
                    | TenantFilter::DownAgents => profile
                        .tenants
                        .iter()
                        .filter(|tenant| Self::tenant_matches_filter(tenant, *filter))
                        .map(|tenant| tenant.name.clone())
                        .collect(),
                };
                ctx.notify();
            }
            MonolithCockpitAction::ExpandAllTenants { tenant_names } => {
                self.expanded_tenants = tenant_names.iter().cloned().collect();
                ctx.notify();
            }
            MonolithCockpitAction::CollapseAllTenants => {
                self.expanded_tenants.clear();
                ctx.notify();
            }
            MonolithCockpitAction::ToggleTenant { tenant_name } => {
                self.selected_tenant = Some(tenant_name.clone());
                self.selected_host = None;
                self.selected_runtime = None;
                if !self.expanded_tenants.insert(tenant_name.clone()) {
                    self.expanded_tenants.remove(tenant_name);
                }
                ctx.notify();
            }
            MonolithCockpitAction::SwitchEnvironment { environment } => {
                self.selected_tenant = None;
                self.selected_host = None;
                self.selected_runtime = None;
                self.expanded_tenants.clear();
                MonolithSettings::handle(ctx).update(ctx, |settings, ctx| {
                    let (api_url, profile_path) = if environment == "prod" {
                        (PROD_API_URL, PROD_PROFILE_PATH)
                    } else {
                        (STAGING_API_URL, STAGING_PROFILE_PATH)
                    };
                    let _ = settings
                        .cockpit_environment
                        .set_value(environment.clone(), ctx);
                    let _ = settings.api_url.set_value(api_url.to_string(), ctx);
                    let _ = settings
                        .cockpit_profile_path
                        .set_value(profile_path.to_string(), ctx);
                });
            }
        }
    }
}

impl View for MonolithCockpitView {
    fn ui_name() -> &'static str {
        "MonolithCockpitView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let (profile, profile_status) = Self::load_profile(app);
        let settings = MonolithSettings::as_ref(app);
        let active_environment = settings.cockpit_environment.value().clone();
        let api_url = settings.api_url.value().clone();
        let filtered_tenants = profile
            .tenants
            .iter()
            .filter(|tenant| Self::tenant_matches_filter(tenant, self.tenant_filter))
            .collect::<Vec<_>>();

        let mut tenants = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.);
        let mut button_index = 0;
        let mut host_index = 0;
        let mut runtime_index = 0;
        for (tenant_index, tenant) in filtered_tenants.iter().enumerate() {
            let tenant_mouse_state = self
                .tenant_mouse_states
                .get(tenant_index)
                .cloned()
                .unwrap_or_default();
            tenants.add_child(Self::tenant_card(
                tenant,
                self.tenant_is_expanded(tenant),
                tenant_mouse_state,
                &self.host_mouse_states,
                &mut host_index,
                &self.runtime_mouse_states,
                &mut runtime_index,
                self.tenant_filter,
                self.selected_tenant
                    .as_ref()
                    .is_some_and(|selected| selected == &tenant.name),
                self.selected_host.as_ref(),
                self.selected_runtime.as_ref(),
                app,
            ));
        }

        let tenant_names = filtered_tenants
            .iter()
            .map(|tenant| tenant.name.clone())
            .collect::<Vec<_>>();
        let mut filter_button_index = 0;
        let mut fleet_control_button_index = 0;

        let mut navigator = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(14.)
            .with_child(
                Text::new("Fleet Navigator", appearance.ui_font_family(), 18.)
                    .with_color(theme.active_ui_text_color().into_solid())
                    .with_style(Properties::default().weight(Weight::Bold))
                    .finish(),
            )
            .with_child(Self::section_label("FLEET TREE", app))
            .with_child(
                Flex::row()
                    .with_spacing(6.)
                    .with_child(Self::tenant_filter_button(
                        &format!("all {}", Self::filter_count(&profile, TenantFilter::All)),
                        TenantFilter::All,
                        self.tenant_filter == TenantFilter::All,
                        Self::next_mouse_state(&self.filter_mouse_states, &mut filter_button_index),
                        app,
                    ))
                    .with_child(Self::tenant_filter_button(
                        &format!(
                            "active {}",
                            Self::filter_count(&profile, TenantFilter::Active)
                        ),
                        TenantFilter::Active,
                        self.tenant_filter == TenantFilter::Active,
                        Self::next_mouse_state(&self.filter_mouse_states, &mut filter_button_index),
                        app,
                    ))
                    .with_child(Self::tenant_filter_button(
                        &format!(
                            "offboarded {}",
                            Self::filter_count(&profile, TenantFilter::Offboarded)
                        ),
                        TenantFilter::Offboarded,
                        self.tenant_filter == TenantFilter::Offboarded,
                        Self::next_mouse_state(&self.filter_mouse_states, &mut filter_button_index),
                        app,
                    ))
                    .finish(),
            )
            .with_child(
                Flex::row()
                    .with_spacing(6.)
                    .with_child(Self::tenant_filter_button(
                        &format!(
                            "with vms {}",
                            Self::filter_count(&profile, TenantFilter::WithVms)
                        ),
                        TenantFilter::WithVms,
                        self.tenant_filter == TenantFilter::WithVms,
                        Self::next_mouse_state(&self.filter_mouse_states, &mut filter_button_index),
                        app,
                    ))
                    .with_child(Self::tenant_filter_button(
                        &format!(
                            "running {}",
                            Self::filter_count(&profile, TenantFilter::RunningAgents)
                        ),
                        TenantFilter::RunningAgents,
                        self.tenant_filter == TenantFilter::RunningAgents,
                        Self::next_mouse_state(&self.filter_mouse_states, &mut filter_button_index),
                        app,
                    ))
                    .with_child(Self::tenant_filter_button(
                        &format!(
                            "down {}",
                            Self::filter_count(&profile, TenantFilter::DownAgents)
                        ),
                        TenantFilter::DownAgents,
                        self.tenant_filter == TenantFilter::DownAgents,
                        Self::next_mouse_state(&self.filter_mouse_states, &mut filter_button_index),
                        app,
                    ))
                    .finish(),
            )
            .with_child(
                Flex::row()
                    .with_spacing(6.)
                    .with_child(Self::typed_button(
                        "expand",
                        MonolithCockpitAction::ExpandAllTenants { tenant_names },
                        Self::next_mouse_state(
                            &self.fleet_control_mouse_states,
                            &mut fleet_control_button_index,
                        ),
                        app,
                    ))
                    .with_child(Self::typed_button(
                        "collapse",
                        MonolithCockpitAction::CollapseAllTenants,
                        Self::next_mouse_state(
                            &self.fleet_control_mouse_states,
                            &mut fleet_control_button_index,
                        ),
                        app,
                    ))
                    .finish(),
            );

        if profile.tenants.is_empty() {
            navigator.add_child(
                Text::new(
                    "No live Monolith cockpit profile is configured.".to_string(),
                    appearance.ui_font_family(),
                    12.,
                )
                .with_color(theme.disabled_ui_text_color().into_solid())
                .finish(),
            );
        } else if filtered_tenants.is_empty() {
            navigator.add_child(
                Text::new(
                    format!(
                        "No tenants match filter: {}",
                        Self::tenant_filter_label(self.tenant_filter)
                    ),
                    appearance.ui_font_family(),
                    12.,
                )
                .with_color(theme.disabled_ui_text_color().into_solid())
                .finish(),
            );
        }

        navigator.add_child(tenants.finish());

        let mut workbench = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(14.)
            .with_child(
                Text::new("Monolith Cockpit", appearance.ui_font_family(), 18.)
                    .with_color(theme.active_ui_text_color().into_solid())
                    .with_style(Properties::default().weight(Weight::Bold))
                    .finish(),
            )
            .with_child(self.render_environment_switcher(app))
            .with_child(self.render_cloud_toolbar(app))
            .with_child(self.render_cockpit_summary(&profile, app))
            .with_child(self.render_scope_strip(app));

        if let Some(status) = profile_status {
            workbench.add_child(
                Text::new(status, appearance.ui_font_family(), 11.)
                    .with_color(theme.disabled_ui_text_color().into_solid())
                    .finish(),
            );
        }

        if let Some(selected_tenant_workbench) = self.render_selected_tenant_workbench(
            &profile,
            &active_environment,
            &api_url,
            self.tenant_filter,
            app,
        ) {
            workbench.add_child(selected_tenant_workbench);
        } else {
            workbench.add_child(Self::stage_empty_state(
                "TENANT",
                "Select a tenant from the navigator to open the tenant workbench.",
                app,
            ));
        }

        if let Some(selected_host_workbench) = self.render_selected_host_workbench(
            &profile,
            self.tenant_filter,
            &self.button_mouse_states,
            &mut button_index,
            app,
        ) {
            workbench.add_child(selected_host_workbench);
        } else {
            workbench.add_child(Self::stage_empty_state(
                "VM",
                "Select a VM under the active tenant to inspect host-level details.",
                app,
            ));
        }

        if let Some(scope) = self.selected_runtime.as_ref() {
            if let Some(selected_runtime_workbench) = self.render_selected_runtime_workbench(
                &profile,
                scope,
                &active_environment,
                &api_url,
                &self.button_mouse_states,
                &mut button_index,
                app,
            ) {
                workbench.add_child(selected_runtime_workbench);
            }
        } else {
            workbench.add_child(Self::stage_empty_state(
                "RUNTIME",
                "Select a runtime to open diagnostics, shell, and guarded actions.",
                app,
            ));
        }

        let content = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.)
            .with_child(
                ConstrainedBox::new(
                    Container::new(navigator.finish())
                        .with_padding(Padding::uniform(10.))
                        .with_background(theme.surface_1())
                        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                        .finish(),
                )
                .with_width(380.)
                .finish(),
            )
            .with_child(
                Shrinkable::new(
                    1.0,
                    Container::new(workbench.finish())
                        .with_padding(Padding::uniform(10.))
                        .with_background(theme.surface_1())
                        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                        .finish(),
                )
                .finish(),
            )
            .finish();

        let scrollable = ClippedScrollable::vertical(
            self.scroll_state.clone(),
            Container::new(content)
                .with_padding(Padding::uniform(12.))
                .finish(),
            ScrollbarWidth::Auto,
            theme.nonactive_ui_detail().into(),
            theme.active_ui_detail().into(),
            Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish();

        Shrinkable::new(
            1.0,
            Container::new(scrollable)
                .with_padding(Padding::uniform(0.))
                .finish(),
        )
        .finish()
    }
}
