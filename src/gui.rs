use crate::config::Config;
use crate::discovery::{discover_installed_apps, load_cached_apps, AppInfo};
use crate::launch::{launch_profile, LaunchOptions, ProfileLaunchReport};
use crate::profile;
use iced::widget::{
    button, column, container, image, pick_list, row, scrollable, text, text_input,
};
use iced::{
    application, border, keyboard, window, Background, Color, Element, Length, Padding, Shadow,
    Subscription, Task, Theme, Vector,
};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

// ---------------------------------------------------------------------------
// Design tokens — a calm, dark, Raycast-inspired palette. One accent, soft
// elevation, almost no hard borders: hierarchy comes from background tints.
// ---------------------------------------------------------------------------
const APP_BG: Color = Color::from_rgb(0.071, 0.071, 0.078); // #121214
const PANEL_BG: Color = Color::from_rgb(0.106, 0.106, 0.118); // #1b1b1e
const ELEVATED_BG: Color = Color::from_rgb(0.137, 0.137, 0.149); // #232326
const FIELD_BG: Color = Color::from_rgb(0.157, 0.157, 0.169); // #28282b
const ICON_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.055);
const ROW_HOVER_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.05);
const ROW_SELECTED_BG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.085);
const HAIRLINE: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.07);
const ACCENT: Color = Color::from_rgb(0.357, 0.553, 1.0); // #5b8dff
const ACCENT_SOFT: Color = Color::from_rgba(0.357, 0.553, 1.0, 0.16);
const ACCENT_SOFT_HOVER: Color = Color::from_rgba(0.357, 0.553, 1.0, 0.24);
const DANGER: Color = Color::from_rgb(1.0, 0.42, 0.42);
const DANGER_SOFT: Color = Color::from_rgba(1.0, 0.42, 0.42, 0.16);
const DANGER_SOFT_HOVER: Color = Color::from_rgba(1.0, 0.42, 0.42, 0.24);
const TEXT: Color = Color::from_rgb(0.925, 0.925, 0.937); // #ececef
const TEXT_DIM: Color = Color::from_rgb(0.62, 0.63, 0.66);
const MUTED: Color = Color::from_rgb(0.62, 0.63, 0.66);
const FAINT: Color = Color::from_rgb(0.42, 0.43, 0.47);
const CLOVIS_ICON: &[u8] = include_bytes!("../.assets/clovis-icon.png");

pub fn run_gui(config_path: PathBuf) -> iced::Result {
    application("Clovis", ClovisGui::update, ClovisGui::view)
        .window(window::Settings {
            icon: window::icon::from_file_data(CLOVIS_ICON, None).ok(),
            ..window::Settings::default()
        })
        .theme(|_| Theme::Dark)
        .subscription(ClovisGui::subscription)
        .run_with(|| ClovisGui::new(config_path))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Launch,
    Configure,
}

#[derive(Debug, Clone)]
enum Message {
    Loaded(Result<LoadedState, String>),
    AppsLoaded(Result<Vec<AppInfo>, String>),
    RefreshApps,
    SelectProfile(String),
    SelectConfigureProfile(String),
    SelectRelativeProfile(i32),
    SelectProfileIndex(usize),
    ActivateSelection,
    LaunchQueryAppend(String),
    LaunchQueryBackspace,
    ClearLaunchQuery,
    SwitchTab(Tab),
    LaunchSelected { close_after: bool },
    LaunchFinished(Result<ProfileLaunchReport, String>, bool),
    NewProfileNameChanged(String),
    CreateProfile,
    RenameValueChanged(String),
    RenameProfile,
    DeleteProfile,
    AppSearchChanged(String),
    ProfileAppSearchChanged(String),
    RemoveApp(String),
    ToggleApp(String, bool),
    Saved(Result<Config, String>),
}

#[derive(Debug, Clone)]
struct LoadedState {
    config: Config,
    profiles: Vec<String>,
}

struct ClovisGui {
    config_path: PathBuf,
    config: Config,
    profiles: Vec<String>,
    installed_apps: Vec<AppInfo>,
    selected_profile: Option<String>,
    configure_profile: Option<String>,
    tab: Tab,
    status: String,
    discovery_status: String,
    launch_status: String,
    launch_query: String,
    app_search: String,
    profile_app_search: String,
    selected_installed_app_index: usize,
    new_profile_name: String,
    rename_value: String,
    busy: bool,
}

impl ClovisGui {
    fn new(config_path: PathBuf) -> (Self, Task<Message>) {
        let app = Self {
            config_path: config_path.clone(),
            config: Config::new(),
            profiles: Vec::new(),
            installed_apps: Vec::new(),
            selected_profile: None,
            configure_profile: None,
            tab: Tab::Launch,
            status: "Loading profiles".to_string(),
            discovery_status: "App cache loads when needed".to_string(),
            launch_status: String::new(),
            launch_query: String::new(),
            app_search: String::new(),
            profile_app_search: String::new(),
            selected_installed_app_index: 0,
            new_profile_name: String::new(),
            rename_value: String::new(),
            busy: true,
        };

        (
            app,
            Task::perform(load_initial_state(config_path), Message::Loaded),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::on_key_press(handle_key_press)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(result) => {
                self.busy = false;
                match result {
                    Ok(state) => {
                        self.config = state.config;
                        self.profiles = state.profiles;
                        self.selected_profile = self.profiles.first().cloned();
                        self.configure_profile = self.selected_profile.clone();
                        self.rename_value = self.configure_profile.clone().unwrap_or_default();
                        self.status = format!("Loaded {} profile(s)", self.profiles.len());
                    }
                    Err(err) => {
                        self.status = err;
                    }
                }
                Task::none()
            }
            Message::AppsLoaded(result) => {
                match result {
                    Ok(apps) => {
                        self.discovery_status = format!("{} installed app(s) ready", apps.len());
                        self.installed_apps = apps;
                        self.selected_installed_app_index = 0;
                    }
                    Err(err) => {
                        self.discovery_status = err;
                    }
                }
                Task::none()
            }
            Message::RefreshApps => {
                self.discovery_status = "Refreshing installed apps".to_string();
                Task::perform(load_apps(false), Message::AppsLoaded)
            }
            Message::SelectProfile(profile) => {
                self.selected_profile = Some(profile);
                Task::none()
            }
            Message::SelectConfigureProfile(profile) => {
                self.rename_value = profile.clone();
                self.configure_profile = Some(profile);
                Task::none()
            }
            Message::SelectRelativeProfile(delta) => {
                if self.tab == Tab::Configure {
                    self.move_installed_selection(delta);
                    return Task::none();
                }
                if self.tab == Tab::Launch && !self.profiles.is_empty() {
                    let visible_profiles = self.visible_launch_profiles();
                    let current = self
                        .selected_profile
                        .as_ref()
                        .and_then(|name| {
                            visible_profiles.iter().position(|profile| profile == name)
                        })
                        .unwrap_or(0);
                    let last = visible_profiles.len().saturating_sub(1) as i32;
                    let next = (current as i32 + delta).clamp(0, last) as usize;
                    self.selected_profile = visible_profiles.get(next).cloned();
                }
                Task::none()
            }
            Message::SelectProfileIndex(index) => {
                if self.tab == Tab::Launch {
                    self.selected_profile = self.visible_launch_profiles().get(index).cloned();
                }
                Task::none()
            }
            Message::LaunchQueryAppend(value) => {
                // The Launch tab has no real text field, so the keyboard
                // subscription drives the search box here. In Configure the
                // real `text_input` widgets own typed input, so we ignore it.
                if self.tab == Tab::Launch {
                    self.launch_query.push_str(&value);
                    self.select_first_visible_profile();
                }
                Task::none()
            }
            Message::LaunchQueryBackspace => {
                if self.tab == Tab::Launch {
                    self.launch_query.pop();
                    self.select_first_visible_profile();
                }
                Task::none()
            }
            Message::ClearLaunchQuery => {
                if self.tab == Tab::Launch {
                    self.launch_query.clear();
                    self.select_first_visible_profile();
                    Task::none()
                } else {
                    self.app_search.clear();
                    self.selected_installed_app_index = 0;
                    text_input::focus(configure_search_id())
                }
            }
            Message::ActivateSelection => match self.tab {
                Tab::Launch => {
                    return self.launch_selected(true);
                }
                Tab::Configure => {
                    let entry = self.app_search.trim().to_string();
                    if !entry.is_empty() && profile::is_explicit_app(&entry) {
                        return self.add_app_from_search(entry);
                    }
                    return self.toggle_focused_installed_app();
                }
            },
            Message::SwitchTab(tab) => {
                self.tab = tab;
                if tab == Tab::Configure {
                    let focus = text_input::focus(configure_search_id());
                    if self.installed_apps.is_empty() {
                        self.discovery_status = "Loading installed app cache".to_string();
                        Task::batch([focus, Task::perform(load_apps(true), Message::AppsLoaded)])
                    } else {
                        focus
                    }
                } else {
                    Task::none()
                }
            }
            Message::LaunchSelected { close_after } => self.launch_selected(close_after),
            Message::LaunchFinished(result, close_after) => {
                match result {
                    Ok(report) => {
                        let failures = report.results.iter().filter(|item| !item.success).count();
                        self.launch_status = format!(
                            "{} dispatched in {:.2} ms, {} failure(s)",
                            report.profile, report.total_dispatch_ms, failures
                        );
                        if close_after && failures == 0 {
                            return iced::exit();
                        }
                    }
                    Err(err) => {
                        self.launch_status = err;
                    }
                }
                Task::none()
            }
            Message::NewProfileNameChanged(value) => {
                self.new_profile_name = value;
                Task::none()
            }
            Message::CreateProfile => {
                let mut config = self.config.clone();
                let name = self.new_profile_name.clone();
                match profile::create_profile(&mut config, &name) {
                    Ok(()) => self.save_config(config),
                    Err(err) => {
                        self.status = err;
                        Task::none()
                    }
                }
            }
            Message::RenameValueChanged(value) => {
                self.rename_value = value;
                Task::none()
            }
            Message::RenameProfile => {
                let Some(old_name) = self.configure_profile.clone() else {
                    self.status = "Select a profile to rename".to_string();
                    return Task::none();
                };
                let mut config = self.config.clone();
                match profile::rename_profile(&mut config, &old_name, &self.rename_value) {
                    Ok(()) => self.save_config(config),
                    Err(err) => {
                        self.status = err;
                        Task::none()
                    }
                }
            }
            Message::DeleteProfile => {
                let Some(name) = self.configure_profile.clone() else {
                    self.status = "Select a profile to delete".to_string();
                    return Task::none();
                };
                let mut config = self.config.clone();
                match profile::delete_profile(&mut config, &name) {
                    Ok(()) => self.save_config(config),
                    Err(err) => {
                        self.status = err;
                        Task::none()
                    }
                }
            }
            Message::AppSearchChanged(value) => {
                self.app_search = value;
                self.selected_installed_app_index = 0;
                Task::none()
            }
            Message::ProfileAppSearchChanged(value) => {
                self.profile_app_search = value;
                Task::none()
            }
            Message::RemoveApp(app) => {
                let Some(profile_name) = self.configure_profile.clone() else {
                    self.status = "Select a profile before removing apps".to_string();
                    return Task::none();
                };
                let mut config = self.config.clone();
                match profile::remove_profile_app(&mut config, &profile_name, &app) {
                    Ok(()) => self.save_config(config),
                    Err(err) => {
                        self.status = err;
                        Task::none()
                    }
                }
            }
            Message::ToggleApp(app, checked) => self.toggle_profile_app(app, checked),
            Message::Saved(result) => {
                match result {
                    Ok(config) => {
                        self.config = config;
                        self.profiles = profile::profile_names(&self.config);
                        if self.selected_profile.is_none()
                            || !self
                                .selected_profile
                                .as_ref()
                                .is_some_and(|name| self.profiles.contains(name))
                        {
                            self.selected_profile = self.profiles.first().cloned();
                        }
                        if self.configure_profile.is_none()
                            || !self
                                .configure_profile
                                .as_ref()
                                .is_some_and(|name| self.profiles.contains(name))
                        {
                            self.configure_profile = self.selected_profile.clone();
                        }
                        self.rename_value = self.configure_profile.clone().unwrap_or_default();
                        self.new_profile_name.clear();
                        self.status = "Configuration saved".to_string();
                    }
                    Err(err) => {
                        self.status = err;
                    }
                }
                Task::none()
            }
        }
    }

    fn save_config(&mut self, config: Config) -> Task<Message> {
        let path = self.config_path.clone();
        self.status = "Saving configuration".to_string();
        Task::perform(
            async move {
                profile::save_profiles(&path, &config)
                    .map(|_| config)
                    .map_err(|e| e.to_string())
            },
            Message::Saved,
        )
    }

    fn launch_selected(&mut self, close_after: bool) -> Task<Message> {
        if self.tab != Tab::Launch {
            return Task::none();
        }
        let Some(profile) = self.selected_profile.clone() else {
            self.launch_status = "Select a profile before launching".to_string();
            return Task::none();
        };
        let config = self.config.clone();
        let cached_apps = self.installed_apps.clone();
        self.launch_status = format!("Launching {profile}");
        Task::perform(
            async move {
                let apps = if cached_apps.is_empty() {
                    load_apps(true).await.unwrap_or_default()
                } else {
                    cached_apps
                };
                launch_profile(&config, &profile, LaunchOptions { force: true }, &apps)
            },
            move |result| Message::LaunchFinished(result, close_after),
        )
    }

    fn move_installed_selection(&mut self, delta: i32) {
        let visible_len = self.visible_installed_apps().len();
        if visible_len == 0 {
            self.selected_installed_app_index = 0;
            return;
        }
        let last = visible_len.saturating_sub(1) as i32;
        self.selected_installed_app_index =
            (self.selected_installed_app_index as i32 + delta).clamp(0, last) as usize;
    }

    fn add_app_from_search(&mut self, entry: String) -> Task<Message> {
        let Some(profile_name) = self.configure_profile.clone() else {
            self.status = "Select or create a profile first".to_string();
            return Task::none();
        };
        let mut config = self.config.clone();
        match profile::add_profile_app(&mut config, &profile_name, &entry) {
            Ok(()) => {
                self.app_search.clear();
                self.selected_installed_app_index = 0;
                let save = self.save_config(config);
                Task::batch([save, text_input::focus(configure_search_id())])
            }
            Err(err) => {
                self.status = err;
                text_input::focus(configure_search_id())
            }
        }
    }

    fn toggle_focused_installed_app(&mut self) -> Task<Message> {
        let Some(app) = self
            .visible_installed_apps()
            .get(self.selected_installed_app_index)
            .cloned()
        else {
            return Task::none();
        };
        let checked = self
            .selected_app_names()
            .contains(&profile::normalize_app(&app.name).to_lowercase());
        let app_name = app.name.clone();
        self.toggle_profile_app(app_name, !checked)
    }

    fn toggle_profile_app(&mut self, app: String, checked: bool) -> Task<Message> {
        let Some(profile_name) = self.configure_profile.clone() else {
            self.status = "Select a profile before editing apps".to_string();
            return Task::none();
        };
        let mut config = self.config.clone();
        let result = if checked {
            profile::add_profile_app(&mut config, &profile_name, &app)
        } else {
            profile::remove_profile_app(&mut config, &profile_name, &app)
        };
        match result {
            Ok(()) => self.save_config(config),
            Err(err) if checked && err.contains("already in profile") => Task::none(),
            Err(err) => {
                self.status = err;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let tabs = container(
            row![
                tab_button(
                    "Launch",
                    self.tab == Tab::Launch,
                    Message::SwitchTab(Tab::Launch)
                ),
                tab_button(
                    "Configure",
                    self.tab == Tab::Configure,
                    Message::SwitchTab(Tab::Configure)
                ),
            ]
            .spacing(2),
        )
        .padding(3)
        .style(segmented_control);

        let header = row![
            app_logo(),
            tabs,
            text(&self.status).size(12).color(FAINT).width(Length::Fill),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);

        let body = match self.tab {
            Tab::Launch => self.view_launch(),
            Tab::Configure => self.view_configure(),
        };

        container(column![header, body].spacing(12))
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(app_background)
            .into()
    }

    fn view_launch(&self) -> Element<'_, Message> {
        let selected_profile = self.selected_profile.as_deref();
        let visible_profiles = self.visible_launch_profiles();
        let mut palette = column![section_label("Profiles")].spacing(2);

        for (index, profile_name) in visible_profiles.iter().enumerate() {
            palette = palette.push(profile_launch_button(
                index,
                profile_name.clone(),
                self.profile_app_count(profile_name),
                selected_profile == Some(profile_name.as_str()),
            ));
        }

        if visible_profiles.is_empty() {
            palette = palette.push(empty_hint("No profiles match your search"));
        }

        let selected_apps = self.selected_profile_apps();
        let selected_app_count = selected_apps.len();
        if let Some(name) = selected_profile {
            palette = palette.push(section_label_owned(format!(
                "{name}  ·  {selected_app_count} app{}",
                if selected_app_count == 1 { "" } else { "s" }
            )));
            if selected_app_count == 0 {
                palette = palette.push(empty_hint("This profile has no apps yet"));
            }
            for app_name in selected_apps {
                let matching_app = self.installed_app_for(&app_name);
                palette = palette.push(launch_app_row(app_name, matching_app));
            }
        }

        palette = palette
            .push(section_label("Actions"))
            .push(command_action_row(
                "Launch this profile",
                "Enter",
                Message::LaunchSelected { close_after: true },
            ));

        column![
            command_strip(&self.launch_query),
            container(
                column![
                    scrollable(container(palette).padding([6, 6]))
                        .height(Length::Fill)
                        .style(scrollbar),
                    hairline(),
                    palette_footer(
                        if self.launch_status.is_empty() {
                            self.discovery_status.as_str()
                        } else {
                            self.launch_status.as_str()
                        },
                        &[
                            ("Up/Down", "Navigate"),
                            ("Enter", "Launch & close"),
                            ("Alt+1-9", "Quick pick"),
                        ],
                    ),
                ]
                .spacing(0),
            )
            .padding(6)
            .height(Length::Fill)
            .style(panel),
        ]
        .spacing(12)
        .height(Length::Fill)
        .into()
    }

    fn view_configure(&self) -> Element<'_, Message> {
        let profile_picker = container(
            pick_list(
                self.profiles.clone(),
                self.configure_profile.clone(),
                Message::SelectConfigureProfile,
            )
            .placeholder("Select a profile")
            .padding([8, 12])
            .text_size(14)
            .style(picker_style)
            .width(Length::Fixed(200.0)),
        );

        let profile_controls = container(
            row![
                profile_picker,
                text_input("Rename current profile", &self.rename_value)
                    .on_input(Message::RenameValueChanged)
                    .on_submit(Message::RenameProfile)
                    .padding([8, 12])
                    .size(14)
                    .style(field_style)
                    .width(Length::Fill),
                ghost_button("Rename", Message::RenameProfile),
                danger_soft_button("Delete", Message::DeleteProfile),
                vertical_divider(),
                text_input("New profile name…", &self.new_profile_name)
                    .on_input(Message::NewProfileNameChanged)
                    .on_submit(Message::CreateProfile)
                    .padding([8, 12])
                    .size(14)
                    .style(field_style)
                    .width(Length::Fixed(190.0)),
                accent_button("Create", Message::CreateProfile),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .padding(8)
        .style(panel);

        let selected_apps = self
            .configure_profile
            .as_ref()
            .and_then(|name| profile::get_profile(&self.config, name))
            .map(|profile| profile.apps)
            .unwrap_or_default();
        let selected_names = self.selected_app_names();
        let in_profile_total = selected_apps.len();

        let profile_query = self.profile_app_search.to_lowercase();
        let mut profile_list = column![].spacing(2);
        let mut shown = 0usize;
        for app in selected_apps
            .into_iter()
            .filter(|app| profile_query.is_empty() || app.to_lowercase().contains(&profile_query))
        {
            shown += 1;
            let matching_app = self.installed_app_for(&app);
            profile_list = profile_list.push(selected_app_row(app, matching_app));
        }
        if shown == 0 {
            profile_list = profile_list.push(empty_hint(if in_profile_total == 0 {
                "No apps yet — pick some from the right"
            } else {
                "Nothing matches that filter"
            }));
        }

        let profile_panel = container(
            column![
                panel_header(
                    "In profile",
                    format!("{in_profile_total}"),
                    Some(
                        text_input("Filter…", &self.profile_app_search)
                            .on_input(Message::ProfileAppSearchChanged)
                            .padding([6, 10])
                            .size(13)
                            .style(field_style)
                            .width(Length::Fixed(150.0))
                            .into()
                    ),
                ),
                hairline(),
                scrollable(container(profile_list).padding([6, 6]))
                    .height(Length::Fill)
                    .style(scrollbar),
            ]
            .spacing(0),
        )
        .padding(6)
        .style(panel)
        .width(Length::FillPortion(2));

        let visible_apps = self.visible_installed_apps();
        let search_query = self.app_search.trim().to_string();
        let manual_entry = (!search_query.is_empty() && profile::is_explicit_app(&search_query))
            .then(|| search_query.clone());

        let mut installed_list = column![].spacing(2);
        if let Some(entry) = manual_entry.as_deref() {
            let already_added =
                selected_names.contains(&profile::normalize_app(entry).to_lowercase());
            installed_list = installed_list.push(add_manual_row(entry, already_added));
        }
        for (index, app) in visible_apps.iter().enumerate() {
            let checked =
                selected_names.contains(&profile::normalize_app(&app.name).to_lowercase());
            installed_list = installed_list.push(app_toggle_row(
                app,
                checked,
                index == self.selected_installed_app_index,
            ));
        }
        if visible_apps.is_empty() && manual_entry.is_none() {
            installed_list = installed_list.push(empty_hint(if self.installed_apps.is_empty() {
                "No app cache yet — hit Refresh"
            } else {
                "Nothing matches — paste a full path or https:// URL to add it"
            }));
        }

        let installed_panel = container(
            column![
                panel_header(
                    "All apps",
                    format!("{} of {}", visible_apps.len(), self.installed_apps.len()),
                    Some(ghost_button("Refresh", Message::RefreshApps)),
                ),
                container(
                    text_input(
                        "Search apps  —  or paste a full path / URL + Enter",
                        &self.app_search
                    )
                    .id(configure_search_id())
                    .on_input(Message::AppSearchChanged)
                    .on_submit(Message::ActivateSelection)
                    .padding([8, 12])
                    .size(14)
                    .style(field_style)
                    .width(Length::Fill),
                )
                .padding([0, 6]),
                hairline(),
                scrollable(container(installed_list).padding([6, 6]))
                    .height(Length::Fill)
                    .style(scrollbar),
            ]
            .spacing(8),
        )
        .padding(6)
        .style(panel)
        .width(Length::FillPortion(3));

        column![
            profile_controls,
            row![profile_panel, installed_panel]
                .spacing(12)
                .height(Length::Fill),
            palette_footer(
                &self.status,
                &[("Enter", "Add / remove"), ("Up/Down", "Navigate")],
            ),
        ]
        .spacing(12)
        .into()
    }

    fn selected_profile_apps(&self) -> Vec<String> {
        self.selected_profile
            .as_ref()
            .and_then(|profile_name| profile::get_profile(&self.config, profile_name))
            .map(|profile| profile.apps)
            .unwrap_or_default()
    }

    fn profile_app_count(&self, profile_name: &str) -> usize {
        profile::get_profile(&self.config, profile_name)
            .map(|profile| profile.apps.len())
            .unwrap_or(0)
    }

    fn selected_app_names(&self) -> HashSet<String> {
        self.configure_profile
            .as_ref()
            .and_then(|name| profile::get_profile(&self.config, name))
            .map(|profile| {
                profile
                    .apps
                    .iter()
                    .map(|app| profile::normalize_app(app).to_lowercase())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn visible_installed_apps(&self) -> Vec<&AppInfo> {
        let query = self.app_search.to_lowercase();
        self.installed_apps
            .iter()
            .filter(|app| {
                query.is_empty()
                    || app.name.to_lowercase().contains(&query)
                    || app
                        .launch_target
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&query)
            })
            .take(180)
            .collect()
    }

    fn visible_launch_profiles(&self) -> Vec<String> {
        let query = self.launch_query.trim().to_lowercase();
        if query.is_empty() {
            return self.profiles.clone();
        }

        self.profiles
            .iter()
            .filter(|profile_name| {
                profile_name.to_lowercase().contains(&query)
                    || profile::get_profile(&self.config, profile_name)
                        .map(|profile| {
                            profile
                                .apps
                                .iter()
                                .any(|app| app.to_lowercase().contains(&query))
                        })
                        .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    fn select_first_visible_profile(&mut self) {
        let visible = self.visible_launch_profiles();
        if visible.is_empty() {
            self.selected_profile = None;
            return;
        }
        if !self
            .selected_profile
            .as_ref()
            .is_some_and(|profile| visible.contains(profile))
        {
            self.selected_profile = visible.first().cloned();
        }
    }

    fn installed_app_for(&self, app_name: &str) -> Option<&AppInfo> {
        self.installed_apps
            .iter()
            .find(|installed| installed.name.eq_ignore_ascii_case(app_name))
            .or_else(|| {
                let normalized = profile::normalize_app(app_name).to_lowercase();
                self.installed_apps.iter().find(|installed| {
                    profile::normalize_app(&installed.name).to_lowercase() == normalized
                })
            })
    }
}

fn handle_key_press(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Message> {
    match key.as_ref() {
        keyboard::Key::Named(keyboard::key::Named::Enter) if modifiers.control() => {
            Some(Message::LaunchSelected { close_after: true })
        }
        keyboard::Key::Named(keyboard::key::Named::Enter) => Some(Message::ActivateSelection),
        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
            Some(Message::SelectRelativeProfile(1))
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
            Some(Message::SelectRelativeProfile(-1))
        }
        keyboard::Key::Named(keyboard::key::Named::Backspace) => {
            Some(Message::LaunchQueryBackspace)
        }
        keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::ClearLaunchQuery),
        keyboard::Key::Character("1" | "&") if modifiers.control() => {
            Some(Message::SwitchTab(Tab::Launch))
        }
        keyboard::Key::Character("2" | "é") if modifiers.control() => {
            Some(Message::SwitchTab(Tab::Configure))
        }
        keyboard::Key::Character(value) if modifiers.alt() => value
            .parse::<usize>()
            .ok()
            .and_then(|index| index.checked_sub(1))
            .map(Message::SelectProfileIndex),
        keyboard::Key::Character(value) if !modifiers.control() && !modifiers.alt() => {
            Some(Message::LaunchQueryAppend(value.to_string()))
        }
        _ => None,
    }
}

fn configure_search_id() -> text_input::Id {
    text_input::Id::new("clovis-configure-search")
}

fn command_strip(query: &str) -> Element<'_, Message> {
    let display = if query.is_empty() {
        "Search profiles…"
    } else {
        query
    };
    let color = if query.is_empty() { FAINT } else { TEXT };
    let trailing: Element<'_, Message> = if query.is_empty() {
        text("").into()
    } else {
        row![text("clear").size(11).color(FAINT), key_cap("Esc")]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
    };

    container(
        row![
            container(text(">").size(17).color(ACCENT))
                .width(Length::Fixed(22.0))
                .center_x(Length::Fixed(22.0)),
            text(display).size(18).color(color).width(Length::Fill),
            trailing,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(command_bar)
    .into()
}

fn panel_header<'a>(
    title: &'a str,
    count: String,
    trailing: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut outer = row![
        text(title).size(13).color(TEXT),
        container(text(count).size(11).color(TEXT_DIM))
            .padding([1, 7])
            .style(count_badge),
        container(text("")).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill);
    if let Some(t) = trailing {
        outer = outer.push(t);
    }
    container(outer).padding([6, 8]).width(Length::Fill).into()
}

fn section_label(label: &'static str) -> Element<'static, Message> {
    section_label_owned(label.to_string())
}

fn section_label_owned(label: String) -> Element<'static, Message> {
    container(text(label.to_uppercase()).size(10).color(FAINT))
        .padding(Padding {
            top: 14.0,
            right: 10.0,
            bottom: 5.0,
            left: 10.0,
        })
        .width(Length::Fill)
        .into()
}

fn kind_label(label: &'static str) -> Element<'static, Message> {
    text(label).size(11).color(FAINT).into()
}

fn empty_hint(msg: &'static str) -> Element<'static, Message> {
    container(text(msg).size(13).color(FAINT))
        .padding([16, 12])
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
}

fn hairline() -> Element<'static, Message> {
    container(text(""))
        .height(Length::Fixed(1.0))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(HAIRLINE)),
            ..container::Style::default()
        })
        .into()
}

fn vertical_divider() -> Element<'static, Message> {
    container(text(""))
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(22.0))
        .style(|_| container::Style {
            background: Some(Background::Color(HAIRLINE)),
            ..container::Style::default()
        })
        .into()
}

fn key_cap(label: &str) -> Element<'static, Message> {
    container(text(label.to_string()).size(10).color(TEXT_DIM))
        .padding([2, 6])
        .style(keycap_style)
        .into()
}

fn palette_footer<'a>(
    status: &'a str,
    keys: &[(&'static str, &'static str)],
) -> Element<'a, Message> {
    let status_text: Element<'a, Message> = if status.is_empty() {
        text("").width(Length::Fill).into()
    } else {
        text(status)
            .size(11)
            .color(FAINT)
            .width(Length::Fill)
            .into()
    };

    let mut bar = row![status_text]
        .spacing(14)
        .align_y(iced::Alignment::Center);
    for (cap, label) in keys {
        bar = bar.push(
            row![key_cap(cap), text(*label).size(11).color(FAINT)]
                .spacing(5)
                .align_y(iced::Alignment::Center),
        );
    }

    container(bar).padding([8, 12]).width(Length::Fill).into()
}

fn tab_button(label: &str, active: bool, message: Message) -> Element<'_, Message> {
    button(text(label).size(13))
        .padding([6, 14])
        .style(tab_style(active))
        .on_press(message)
        .into()
}

fn app_logo() -> Element<'static, Message> {
    let icon_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".assets")
        .join("clovis-icon.png");

    container(
        image(icon_path)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0)),
    )
    .width(Length::Fixed(34.0))
    .height(Length::Fixed(34.0))
    .center_x(Length::Fixed(34.0))
    .center_y(Length::Fixed(34.0))
    .style(app_logo_shell)
    .into()
}

fn list_row<'a>(
    leading: Element<'a, Message>,
    title: String,
    subtitle: String,
    trailing: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        leading,
        column![
            text(title).size(14).color(TEXT),
            text(subtitle).size(11).color(FAINT),
        ]
        .spacing(2)
        .width(Length::Fill),
        trailing,
    ]
    .spacing(11)
    .align_y(iced::Alignment::Center)
    .into()
}

fn profile_launch_button(
    index: usize,
    profile_name: String,
    app_count: usize,
    active: bool,
) -> Element<'static, Message> {
    let select_profile = profile_name.clone();
    let badge: Element<'static, Message> = if index < 9 {
        row![
            key_cap(&format!("Alt+{}", index + 1)),
            kind_label("Profile"),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        kind_label("Profile")
    };
    let content = list_row(
        glyph_icon(&profile_name, active),
        profile_name,
        format!("{app_count} app{}", if app_count == 1 { "" } else { "s" }),
        badge,
    );

    button(container(content).padding([8, 10]).width(Length::Fill))
        .style(row_button(active))
        .on_press(Message::SelectProfile(select_profile))
        .width(Length::Fill)
        .into()
}

fn app_toggle_row(app: &AppInfo, checked: bool, active: bool) -> Element<'_, Message> {
    let app_name = app.name.clone();
    let trailing: Element<'_, Message> = match (active, checked) {
        (true, true) => row![text("remove").size(11).color(FAINT), key_cap("Enter")]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into(),
        (true, false) => row![text("add").size(11).color(ACCENT), key_cap("Enter")]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into(),
        (false, true) => container(text("Added").size(11).color(ACCENT))
            .padding([2, 8])
            .style(added_badge)
            .into(),
        (false, false) => kind_label("Application"),
    };

    button(
        container(list_row(
            app_icon(&app.name, app.icon_path.clone()),
            app.name.clone(),
            app_location(app),
            trailing,
        ))
        .padding([7, 10])
        .width(Length::Fill),
    )
    .style(row_button(active))
    .on_press(Message::ToggleApp(app_name, !checked))
    .width(Length::Fill)
    .into()
}

fn selected_app_row(name: String, app: Option<&AppInfo>) -> Element<'static, Message> {
    let remove_name = name.clone();
    let icon_path = app.and_then(|app| app.icon_path.clone());
    let location = app
        .map(app_location)
        .unwrap_or_else(|| "saved in this profile".to_string());
    container(list_row(
        app_icon(&name, icon_path),
        name,
        location,
        button(text("Remove").size(12))
            .padding([5, 11])
            .style(danger_soft_button_style)
            .on_press(Message::RemoveApp(remove_name))
            .into(),
    ))
    .padding([7, 10])
    .style(static_row)
    .into()
}

fn launch_app_row(name: String, app: Option<&AppInfo>) -> Element<'static, Message> {
    let icon_path = app.and_then(|app| app.icon_path.clone());
    let location = app
        .map(app_location)
        .unwrap_or_else(|| "saved in this profile".to_string());
    container(list_row(
        app_icon(&name, icon_path),
        name,
        location,
        kind_label("Application"),
    ))
    .padding([7, 10])
    .style(static_row)
    .into()
}

fn command_action_row(
    title: &'static str,
    shortcut: &'static str,
    message: Message,
) -> Element<'static, Message> {
    let content = row![
        container(text(">").size(14).color(ACCENT))
            .width(Length::Fixed(34.0))
            .height(Length::Fixed(34.0))
            .center_x(Length::Fixed(34.0))
            .center_y(Length::Fixed(34.0))
            .style(action_icon),
        text(title).size(14).color(TEXT).width(Length::Fill),
        key_cap(shortcut),
    ]
    .spacing(11)
    .align_y(iced::Alignment::Center);

    button(container(content).padding([7, 10]).width(Length::Fill))
        .style(row_button(false))
        .on_press(message)
        .width(Length::Fill)
        .into()
}

fn add_manual_row(entry: &str, already_added: bool) -> Element<'static, Message> {
    let is_url = profile::is_url_like(entry);
    let display = if !is_url && entry.contains('\\') {
        compact_path(Path::new(entry))
    } else {
        entry.to_string()
    };
    let subtitle = if is_url {
        "Opens this link when the profile launches"
    } else {
        "Launches this file directly"
    }
    .to_string();
    let trailing: Element<'static, Message> = if already_added {
        container(text("Already added").size(11).color(ACCENT))
            .padding([2, 8])
            .style(added_badge)
            .into()
    } else {
        row![text("add").size(11).color(ACCENT), key_cap("Enter")]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
    };
    let icon = container(text(if is_url { "@" } else { "+" }).size(14).color(ACCENT))
        .width(Length::Fixed(34.0))
        .height(Length::Fixed(34.0))
        .center_x(Length::Fixed(34.0))
        .center_y(Length::Fixed(34.0))
        .style(action_icon);

    button(
        container(list_row(icon.into(), display, subtitle, trailing))
            .padding([7, 10])
            .width(Length::Fill),
    )
    .style(row_button(!already_added))
    .on_press(Message::ActivateSelection)
    .width(Length::Fill)
    .into()
}

fn glyph_icon(name: &str, active: bool) -> Element<'static, Message> {
    let (fg, style): (Color, fn(&Theme) -> container::Style) = if active {
        (ACCENT, accent_icon)
    } else {
        (TEXT_DIM, icon_shell)
    };
    container(text(app_initials(name)).size(13).color(fg))
        .width(Length::Fixed(34.0))
        .height(Length::Fixed(34.0))
        .center_x(Length::Fixed(34.0))
        .center_y(Length::Fixed(34.0))
        .style(style)
        .into()
}

fn app_icon(name: &str, icon_path: Option<PathBuf>) -> Element<'static, Message> {
    if let Some(path) = icon_path.filter(|path| path.exists()) {
        return container(
            image(path)
                .width(Length::Fixed(22.0))
                .height(Length::Fixed(22.0)),
        )
        .width(Length::Fixed(34.0))
        .height(Length::Fixed(34.0))
        .center_x(Length::Fixed(34.0))
        .center_y(Length::Fixed(34.0))
        .style(icon_shell)
        .into();
    }

    container(text(app_initials(name)).size(12).color(TEXT_DIM))
        .width(Length::Fixed(34.0))
        .height(Length::Fixed(34.0))
        .center_x(Length::Fixed(34.0))
        .center_y(Length::Fixed(34.0))
        .style(icon_shell)
        .into()
}

fn ghost_button(label: &str, message: Message) -> Element<'_, Message> {
    button(text(label.to_string()).size(13))
        .padding([8, 13])
        .style(ghost_button_style)
        .on_press(message)
        .into()
}

fn accent_button(label: &str, message: Message) -> Element<'_, Message> {
    button(text(label.to_string()).size(13))
        .padding([8, 14])
        .style(accent_button_style)
        .on_press(message)
        .into()
}

fn danger_soft_button(label: &str, message: Message) -> Element<'_, Message> {
    button(text(label.to_string()).size(13))
        .padding([8, 13])
        .style(danger_soft_button_style)
        .on_press(message)
        .into()
}

fn app_initials(name: &str) -> String {
    let mut initials = String::new();
    for part in name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .take(2)
    {
        if let Some(ch) = part.chars().next() {
            initials.push(ch.to_ascii_uppercase());
        }
    }
    if initials.is_empty() {
        "?".to_string()
    } else {
        initials
    }
}

fn app_location(app: &AppInfo) -> String {
    format!(
        "{} - {}",
        app.source.replace('_', " "),
        compact_path(&app.launch_target)
    )
}

fn compact_path(path: &Path) -> String {
    let parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return path.display().to_string();
    }

    let keep = parts.len().min(4);
    let tail = parts[parts.len() - keep..].join("\\");
    if parts.len() > keep {
        format!("...\\{tail}")
    } else {
        tail
    }
}

fn soft_shadow(blur: f32, y: f32, alpha: f32) -> Shadow {
    Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, alpha),
        offset: Vector::new(0.0, y),
        blur_radius: blur,
    }
}

fn app_background(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(APP_BG)),
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL_BG)),
        border: border::rounded(12).color(HAIRLINE).width(1),
        shadow: soft_shadow(24.0, 6.0, 0.22),
        ..container::Style::default()
    }
}

fn command_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ELEVATED_BG)),
        border: border::rounded(12).color(HAIRLINE).width(1),
        shadow: soft_shadow(24.0, 6.0, 0.25),
        ..container::Style::default()
    }
}

fn segmented_control(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL_BG)),
        border: border::rounded(9).color(HAIRLINE).width(1),
        ..container::Style::default()
    }
}

fn count_badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ICON_BG)),
        border: border::rounded(5),
        ..container::Style::default()
    }
}

fn added_badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ACCENT_SOFT)),
        border: border::rounded(5),
        ..container::Style::default()
    }
}

fn keycap_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ELEVATED_BG)),
        border: border::rounded(4).color(HAIRLINE).width(1),
        ..container::Style::default()
    }
}

fn icon_shell(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ICON_BG)),
        border: border::rounded(8),
        ..container::Style::default()
    }
}

fn accent_icon(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ACCENT_SOFT)),
        border: border::rounded(8),
        ..container::Style::default()
    }
}

fn action_icon(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ACCENT_SOFT)),
        border: border::rounded(8),
        ..container::Style::default()
    }
}

fn app_logo_shell(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.075))),
        border: border::rounded(8).color(HAIRLINE).width(1),
        ..container::Style::default()
    }
}

fn static_row(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: border::rounded(8),
        ..container::Style::default()
    }
}

fn field_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused => ACCENT,
        text_input::Status::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.14),
        _ => HAIRLINE,
    };
    text_input::Style {
        background: Background::Color(FIELD_BG),
        border: border::rounded(8).color(border_color).width(1),
        icon: TEXT_DIM,
        placeholder: FAINT,
        value: TEXT,
        selection: ACCENT_SOFT_HOVER,
    }
}

fn picker_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let border_color = match status {
        pick_list::Status::Opened => ACCENT,
        pick_list::Status::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.14),
        pick_list::Status::Active => HAIRLINE,
    };
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: FAINT,
        handle_color: TEXT_DIM,
        background: Background::Color(FIELD_BG),
        border: border::rounded(8).color(border_color).width(1),
    }
}

fn scrollbar(_theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let scroller_color = match status {
        scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. } => {
            Color::from_rgba(1.0, 1.0, 1.0, 0.22)
        }
        _ => Color::from_rgba(1.0, 1.0, 1.0, 0.12),
    };
    let rail = scrollable::Rail {
        background: None,
        border: border::rounded(8),
        scroller: scrollable::Scroller {
            color: scroller_color,
            border: border::rounded(8),
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
    }
}

fn tab_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match (active, status) {
            (true, _) => Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            (false, button::Status::Hovered) => Color::from_rgba(1.0, 1.0, 1.0, 0.04),
            (false, _) => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: if active { TEXT } else { MUTED },
            border: border::rounded(7),
            ..Default::default()
        }
    }
}

fn row_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match (active, status) {
            (true, button::Status::Hovered) => Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            (true, _) => ROW_SELECTED_BG,
            (false, button::Status::Hovered) => ROW_HOVER_BG,
            (false, _) => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: TEXT,
            border: if active {
                border::rounded(8).color(ACCENT_SOFT_HOVER).width(1)
            } else {
                border::rounded(8)
            },
            ..Default::default()
        }
    }
}

fn ghost_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.10),
        button::Status::Pressed => Color::from_rgba(1.0, 1.0, 1.0, 0.06),
        _ => Color::from_rgba(1.0, 1.0, 1.0, 0.05),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: TEXT,
        border: border::rounded(8).color(HAIRLINE).width(1),
        ..Default::default()
    }
}

fn accent_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => ACCENT_SOFT_HOVER,
        button::Status::Pressed => ACCENT_SOFT,
        _ => ACCENT_SOFT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: ACCENT,
        border: border::rounded(8).color(ACCENT_SOFT_HOVER).width(1),
        ..Default::default()
    }
}

fn danger_soft_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => DANGER_SOFT_HOVER,
        _ => DANGER_SOFT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: DANGER,
        border: border::rounded(8).color(DANGER_SOFT_HOVER).width(1),
        ..Default::default()
    }
}

async fn load_initial_state(config_path: PathBuf) -> Result<LoadedState, String> {
    let config = profile::load_or_default(&config_path);
    let profiles = profile::profile_names(&config);
    Ok(LoadedState { config, profiles })
}

async fn load_apps(use_cache: bool) -> Result<Vec<AppInfo>, String> {
    if use_cache {
        return load_cached_apps()
            .map(|cached| cached.apps)
            .map_err(|_| "No app cache yet; use Refresh in Configure".to_string());
    }

    let report = discover_installed_apps(use_cache);
    if !report.errors.is_empty() && report.apps.is_empty() {
        return Err(report.errors.join("; "));
    }
    Ok(report.apps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{add_profile_app, create_profile, save_profiles};

    #[test]
    fn gui_initial_state_loads_profiles_from_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let mut config = Config::new();
        create_profile(&mut config, "work").unwrap();
        add_profile_app(&mut config, "work", "code").unwrap();
        save_profiles(&path, &config).unwrap();

        let loaded = futures::executor::block_on(load_initial_state(path)).unwrap();

        assert_eq!(loaded.profiles, vec!["work".to_string()]);
    }
}
