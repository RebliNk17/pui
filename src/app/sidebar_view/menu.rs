//! Bounded two-level view menu. State is transient presentation, never saved execution.
use super::*;

#[cfg(test)]
mod safety_tests;
#[cfg(test)]
mod tests;

const WIDTH: f32 = 238.0;
const ROW: f32 = 30.0;
const GAP: f32 = 6.0;
const EDGE: f32 = 8.0;

fn id() -> egui::Id {
    egui::Id::new("session-view-menu")
}
fn state_id() -> egui::Id {
    id().with("navigation")
}

pub(super) fn demo(ctx: &egui::Context) {
    egui::Popup::open_id(ctx, id());
    save(
        ctx,
        State {
            page: Some(Page::Show),
            child: true,
            focus: true,
            ..Default::default()
        },
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Group,
    Sort,
    Show,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Group => "Group by",
            Self::Sort => "Sort by",
            Self::Show => "Show",
        }
    }
    fn len(self) -> usize {
        match self {
            Self::Group | Self::Sort => 5,
            Self::Show => 7,
        }
    }
}
#[derive(Clone, Default)]
struct State {
    page: Option<Page>,
    child: bool,
    root_index: usize,
    child_index: usize,
    focus: bool,
    trigger: Option<egui::Id>,
}
fn load(ctx: &egui::Context) -> State {
    ctx.data_mut(|data| data.get_temp(state_id()).unwrap_or_default())
}
fn save(ctx: &egui::Context, state: State) {
    ctx.data_mut(|data| data.insert_temp(state_id(), state));
}
fn close(ctx: &egui::Context, state: &State) {
    egui::Popup::close_id(ctx, id());
    if let Some(trigger) = state.trigger {
        ctx.memory_mut(|memory| memory.request_focus(trigger));
    }
}

/// Called before global Escape/approval/composer shortcuts; also safe in standalone render tests.
pub(super) fn inputs(app: &mut Pui, ctx: &egui::Context) {
    if !egui::Popup::is_id_open(ctx, id()) {
        return;
    }
    if app.prefs.hide_sidebar
        || app.settings_open
        || ctx.memory(|memory| memory.top_modal_layer().is_some())
    {
        egui::Popup::close_id(ctx, id());
        return;
    }
    let mut state = load(ctx);
    let mut action = None;
    let mut dismiss = false;
    ctx.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            if state.page.is_some() {
                state.page = None;
                state.child = false;
                state.focus = true;
            } else {
                dismiss = true;
            }
        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
            state.page = None;
            state.child = false;
            state.focus = true;
        } else {
            let back = input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                || input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab);
            let next = input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
            if back || next {
                let len = if state.child {
                    state.page.unwrap().len()
                } else {
                    6
                };
                let index = if state.child {
                    &mut state.child_index
                } else {
                    &mut state.root_index
                };
                loop {
                    *index = (*index + if back { len - 1 } else { 1 }) % len;
                    if state.child || *index != 3 || app.prefs.sidebar_view.sections.len() < 64 {
                        break;
                    }
                }
                state.focus = true;
            }
            let right = input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight);
            let activate = input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            if !state.child && (right || activate) && state.root_index < 3 {
                state.page = Some([Page::Group, Page::Sort, Page::Show][state.root_index]);
                state.child = true;
                state.child_index = 0;
                state.focus = true;
            } else if activate {
                action = Some((
                    state.child.then_some(state.page).flatten(),
                    if state.child {
                        state.child_index
                    } else {
                        state.root_index
                    },
                ));
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                if state.child {
                    state.child_index = 0;
                } else {
                    state.root_index = 0;
                }
                state.focus = true;
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                if state.child {
                    state.child_index = state.page.unwrap().len() - 1;
                } else {
                    state.root_index = 5;
                }
                state.focus = true;
            }
        }
        // A view menu never grants a pending approval or sends a draft.
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
    });
    if dismiss {
        close(ctx, &state);
    }
    if let Some((page, index)) = action {
        apply(app, ctx, &mut state, page, index);
    }
    save(ctx, state);
}

fn apply(app: &mut Pui, ctx: &egui::Context, state: &mut State, page: Option<Page>, index: usize) {
    match page {
        Some(Page::Group) => {
            app.prefs.sidebar_view.group = [
                Group::Date,
                Group::Status,
                Group::Folder,
                Group::Environment,
                Group::None,
            ][index];
            app.projects = app.prefs.sidebar_view.group == Group::Folder;
        }
        Some(Page::Sort) if index < 3 => {
            // Preserve the logical direction while changing the historic Name encoding.
            let descending = app.prefs.session_order.reverse
                == (app.prefs.session_order.sort == sidebar_order::Sort::Name);
            app.prefs.session_order.sort = [
                sidebar_order::Sort::Activity,
                sidebar_order::Sort::Created,
                sidebar_order::Sort::Name,
            ][index];
            app.prefs.session_order.reverse =
                descending == (app.prefs.session_order.sort == sidebar_order::Sort::Name);
        }
        Some(Page::Sort) => {
            app.prefs.session_order.reverse =
                (index == 3) == (app.prefs.session_order.sort == sidebar_order::Sort::Name)
        }
        Some(Page::Show) => match index {
            0 => app.prefs.sidebar_view.metadata ^= true,
            1 => app.prefs.sidebar_view.scheduled ^= true,
            2 => app.prefs.sidebar_view.side_chats ^= true,
            3 => app.prefs.sidebar_view.empty_groups ^= true,
            _ => {
                app.show_archived = index == 5;
                app.show_hidden = index == 6;
            }
        },
        None => match index {
            3 if app.prefs.sidebar_view.sections.len() < 64 => {
                app.new_sidebar_section();
                close(ctx, state);
            }
            4 => {
                let sections = std::mem::take(&mut app.prefs.sidebar_view.sections);
                app.prefs.sidebar_view = Options {
                    sections,
                    ..Default::default()
                };
                app.prefs.session_order = Default::default();
                app.projects = true;
                close(ctx, state);
            }
            5 => {
                app.prefs.sidebar_view.view_all = true;
                app.closed_projects.clear();
                for section in &mut app.prefs.sidebar_view.sections {
                    section.collapsed = false;
                }
                close(ctx, state);
            }
            _ => {}
        },
    }
}

#[derive(Debug)]
struct Placement {
    pos: egui::Pos2,
    width: f32,
    compact: bool,
    max_height: f32,
}
fn placement(screen: egui::Rect, trigger: egui::Rect) -> Placement {
    let screen = screen.shrink(EDGE);
    let width = WIDTH.min(screen.width().max(1.0));
    let beside = trigger.right() + GAP + width <= screen.right();
    let x = if beside {
        trigger.right() + GAP
    } else {
        (trigger.right() - width).clamp(screen.left(), (screen.right() - width).max(screen.left()))
    };
    // Narrow mode is a single card below the trigger, never a covering/flipped submenu.
    let below = screen.bottom() - trigger.bottom() - GAP;
    let above = trigger.top() - GAP - screen.top();
    let (y, max_height) = if beside {
        let y = trigger.top().clamp(
            screen.top(),
            (screen.bottom() - ROW - 20.0).max(screen.top()),
        );
        (y, screen.bottom() - y)
    } else if below >= above || below >= 280.0 {
        (trigger.bottom() + GAP, below)
    } else {
        (screen.top(), above)
    };
    Placement {
        pos: egui::pos2(x, y),
        width,
        compact: !beside || x + width * 2.0 + GAP > screen.right(),
        max_height: max_height.max(ROW + 20.0),
    }
}

fn row(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    selected: bool,
    enabled: bool,
    branch: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        label,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(12.0),
            color: colors.text,
            ..Default::default()
        },
    );
    if !value.is_empty() {
        job.append(
            value,
            18.0,
            egui::TextFormat {
                font_id: FontId::proportional(11.0),
                color: colors.muted,
                ..Default::default()
            },
        );
    }
    job.wrap.max_width = (ui.available_width() - 42.0).max(1.0);
    job.wrap.max_rows = 1;
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), Sense::click());
            if response.hovered() || response.has_focus() {
                ui.painter().rect_filled(rect, 5, colors.hover);
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect,
                    5,
                    Stroke::new(1.0, colors.accent),
                    egui::StrokeKind::Inside,
                );
            }
            if selected {
                let center = rect.left_center() + egui::vec2(12.0, 0.0);
                ui.painter().add(egui::Shape::line(
                    vec![
                        center + egui::vec2(-4.0, 0.0),
                        center + egui::vec2(-1.0, 3.0),
                        center + egui::vec2(5.0, -3.0),
                    ],
                    Stroke::new(1.5, colors.accent),
                ));
            }
            let galley = ui.fonts(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                egui::pos2(
                    rect.left() + if branch { 8.0 } else { 26.0 },
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                colors.text,
            );
            response
        })
        .inner;
    if branch {
        icons::chevron(
            ui,
            egui::Rect::from_center_size(
                response.rect.right_center() - egui::vec2(10.0, 0.0),
                Vec2::splat(12.0),
            ),
            false,
            colors.dim,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            enabled,
            selected,
            format!("{label} {value}"),
        )
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn track(ui: &egui::Ui, state: &mut State, response: &egui::Response, child: bool, index: usize) {
    if state.focus
        && state.child == child
        && (if child {
            state.child_index
        } else {
            state.root_index
        }) == index
    {
        response.request_focus();
        response.scroll_to_me(None);
        state.focus = false;
    }
    if response.hovered()
        && ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::PointerMoved(_)))
        })
    {
        state.child = child;
        if child {
            state.child_index = index;
        } else {
            state.root_index = index;
        }
    }
}

fn content(app: &mut Pui, ui: &mut egui::Ui, state: &mut State, page: Page) {
    let group = app.prefs.sidebar_view.group;
    let sort = app.prefs.session_order.sort;
    let descending = app.prefs.session_order.reverse == (sort == sidebar_order::Sort::Name);
    for index in 0..page.len() {
        if (page == Page::Sort && index == 3) || (page == Page::Show && [1, 4].contains(&index)) {
            ui.separator();
        }
        let (label, selected) = match page {
            Page::Group => {
                let value = [
                    Group::Date,
                    Group::Status,
                    Group::Folder,
                    Group::Environment,
                    Group::None,
                ][index];
                (value.label(), group == value)
            }
            Page::Sort => (
                [
                    "Last updated",
                    "Date created",
                    "Name",
                    "Descending",
                    "Ascending",
                ][index],
                if index < 3 {
                    sort == [
                        sidebar_order::Sort::Activity,
                        sidebar_order::Sort::Created,
                        sidebar_order::Sort::Name,
                    ][index]
                } else {
                    descending == (index == 3)
                },
            ),
            Page::Show => (
                [
                    "Metadata",
                    "Scheduled",
                    "Side chats",
                    "empty groups",
                    "Active chats",
                    "Archived chats",
                    "Hidden chats",
                ][index],
                [
                    app.prefs.sidebar_view.metadata,
                    app.prefs.sidebar_view.scheduled,
                    app.prefs.sidebar_view.side_chats,
                    app.prefs.sidebar_view.empty_groups,
                    !app.show_archived && !app.show_hidden,
                    app.show_archived,
                    app.show_hidden,
                ][index],
            ),
        };
        let response = row(ui, label, "", selected, true, false);
        track(ui, state, &response, true, index);
        if response.clicked() {
            apply(app, ui.ctx(), state, Some(page), index);
        }
    }
}

pub(super) fn show(app: &mut Pui, ui: &mut egui::Ui) -> egui::Response {
    inputs(app, ui.ctx());
    let view = icons::button(ui, Icon::Sort, "Session view options");
    view.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Session view options")
    });
    let ctx = ui.ctx().clone();
    if view.clicked() {
        if egui::Popup::is_id_open(&ctx, id()) {
            close(&ctx, &load(&ctx));
        } else {
            egui::Popup::open_id(&ctx, id());
            save(
                &ctx,
                State {
                    trigger: Some(view.id),
                    focus: true,
                    ..Default::default()
                },
            );
            app.focus_composer = false;
        }
    }
    if !egui::Popup::is_id_open(&ctx, id()) {
        return view;
    }
    let mut state = load(&ctx);
    state.trigger = Some(view.id);
    let place = placement(ctx.screen_rect(), view.rect);
    if place.compact && state.page.is_some() && !state.child {
        state.child = true;
        state.focus = true;
    }
    ui.painter().rect_stroke(
        view.rect,
        6,
        Stroke::new(1.0, theme::palette(&ctx).accent),
        egui::StrokeKind::Inside,
    );
    let mut anchor = None;
    let previous_child = ctx.memory(|memory| memory.area_rect(id().with("child")));
    let moving_to_child = state.page.is_some()
        && previous_child
            .is_some_and(|rect| ctx.input(|input| input.pointer.is_moving_towards_rect(&rect)));
    let popup = egui::Popup::from_response(&view)
        .id(id())
        .open_memory(None)
        .at_position(place.pos)
        .align(egui::RectAlign::BOTTOM_START)
        .align_alternatives(&[])
        .layout(Layout::top_down_justified(Align::Min))
        .width(place.width)
        .frame(prompt_ui::popup(&ctx))
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            theme::menu_controls(ui);
            ui.set_width((place.width - 20.0).max(1.0));
            ui.set_max_height((place.max_height - 20.0).max(ROW));
            egui::ScrollArea::vertical()
                .animated(false)
                .id_salt("view-menu-scroll")
                .max_height((place.max_height - 20.0).max(ROW))
                .show(ui, |ui| {
                    if place.compact
                        && let Some(page) = state.page
                    {
                        if row(ui, "Back to view options", "", false, true, false).clicked() {
                            state.page = None;
                            state.child = false;
                            state.focus = true;
                        }
                        ui.label(theme::small(ui, page.title()));
                        content(app, ui, &mut state, page);
                    } else {
                        for index in 0..6 {
                            if [3, 4].contains(&index) {
                                ui.separator();
                            }
                            let label = [
                                "Group by",
                                "Sort by",
                                "Show",
                                "New section",
                                "Reset to default",
                                "View all",
                            ][index];
                            let value = match index {
                                0 => app.prefs.sidebar_view.group.label(),
                                1 => app.prefs.session_order.sort.label(),
                                _ => "",
                            };
                            let enabled = index != 3 || app.prefs.sidebar_view.sections.len() < 64;
                            let response = row(ui, label, value, false, enabled, index < 3);
                            track(ui, &mut state, &response, false, index);
                            let hover = !place.compact
                                && response.hovered()
                                && !moving_to_child
                                && ui.input(|input| {
                                    input
                                        .events
                                        .iter()
                                        .any(|event| matches!(event, egui::Event::PointerMoved(_)))
                                });
                            if index < 3 {
                                let page = [Page::Group, Page::Sort, Page::Show][index];
                                if response.clicked() || hover {
                                    if state.page != Some(page) {
                                        state.child_index = 0;
                                    }
                                    state.page = Some(page);
                                    if response.clicked() {
                                        state.child = true;
                                        state.focus = true;
                                    }
                                }
                                if state.page == Some(page) {
                                    anchor = Some(response.rect);
                                }
                            } else if response.clicked() {
                                apply(app, &ctx, &mut state, None, index);
                            }
                        }
                    }
                });
        });
    let mut child_rect = None;
    if !place.compact
        && let (Some(page), Some(anchor)) = (state.page, anchor)
    {
        let child_pos = egui::pos2(place.pos.x + place.width + GAP, anchor.top());
        let child = egui::Popup::from_response(&view)
            .id(id().with("child"))
            .at_position(child_pos)
            .align(egui::RectAlign::BOTTOM_START)
            .align_alternatives(&[])
            .layout(Layout::top_down_justified(Align::Min))
            .width(WIDTH)
            .frame(prompt_ui::popup(&ctx))
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .show(|ui| {
                theme::menu_controls(ui);
                ui.set_width(WIDTH - 20.0);
                ui.set_max_height((ctx.screen_rect().bottom() - child_pos.y - 28.0).max(ROW));
                egui::ScrollArea::vertical()
                    .animated(false)
                    .id_salt(page.title())
                    .max_height((ctx.screen_rect().bottom() - child_pos.y - 28.0).max(ROW))
                    .show(ui, |ui| content(app, ui, &mut state, page));
            });
        child_rect = child.map(|child| child.response.rect);
    }
    if let Some(popup) = popup {
        let outside = ctx.input(|input| {
            input.pointer.any_click()
                && input.pointer.interact_pos().is_some_and(|pos| {
                    !popup.response.rect.contains(pos)
                        && !view.rect.contains(pos)
                        && !child_rect.is_some_and(|rect| rect.contains(pos))
                })
        });
        if outside {
            close(&ctx, &state);
        }
    }
    save(&ctx, state);
    view
}
