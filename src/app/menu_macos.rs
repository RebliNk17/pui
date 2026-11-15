use super::*;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, sel,
};
use objc2_app_kit::{NSApplication, NSEventModifierFlags as Flags, NSMenu, NSMenuItem};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
struct Ivars {
    tx: mpsc::Sender<Command>,
    ctx: egui::Context,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind=MainThreadOnly]
    #[ivars=Ivars]
    struct Target;
    unsafe impl NSObjectProtocol for Target {}
    impl Target {
        #[unsafe(method(puiCommand:))]
        fn command(&self,item:&NSMenuItem){
            if let Some(command)=Command::from_tag(item.tag()).filter(|c| c.supported()) {
                let _=self.ivars().tx.send(command);self.ivars().ctx.request_repaint();
            }
        }
    }
);
pub(super) struct Native {
    _target: Retained<Target>,
    menu: Retained<NSMenu>,
    items: Vec<(Command, Retained<NSMenuItem>)>,
    state: Option<MenuState>,
    pub rx: Receiver<Command>,
}
impl Drop for Native {
    fn drop(&mut self) {
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            if app.mainMenu().is_some_and(|menu| *menu == *self.menu) {
                app.setWindowsMenu(None);
                app.setMainMenu(None);
            }
        }
    }
}
fn submenu(mtm: MainThreadMarker, parent: &NSMenu, title: &str) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    menu.setAutoenablesItems(false);
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            None,
            &NSString::from_str(""),
        )
    };
    item.setSubmenu(Some(&menu));
    parent.addItem(&item);
    menu
}
fn accelerator(command: Command, composer: bool) -> (String, Flags) {
    let Some(shortcut) = command.contextual_shortcut(composer) else {
        return (String::new(), Flags::empty());
    };
    let key = match shortcut.logical_key {
        egui::Key::Comma => ",".into(),
        egui::Key::Num0 => "0".into(),
        egui::Key::Plus => "+".into(),
        egui::Key::Minus => "-".into(),
        key => key.name().to_lowercase(),
    };
    let mut flags = Flags::empty();
    if shortcut.modifiers.command {
        flags |= Flags::Command;
    }
    if shortcut.modifiers.shift {
        flags |= Flags::Shift;
    }
    if shortcut.modifiers.ctrl {
        flags |= Flags::Control;
    }
    if shortcut.modifiers.alt {
        flags |= Flags::Option;
    }
    (key, flags)
}
impl Native {
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let (tx, rx) = mpsc::channel();
        let this = Target::alloc(mtm).set_ivars(Ivars {
            tx,
            ctx: ctx.clone(),
        });
        // NSObject's initializer has no additional requirements.
        let target: Retained<Target> = unsafe { msg_send![super(this), init] };
        let app = NSApplication::sharedApplication(mtm);
        let bar = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Pui"));
        let mut native = Self {
            _target: target,
            menu: bar,
            items: Vec::new(),
            state: None,
            rx,
        };
        let pui = submenu(mtm, &native.menu, "Pui");
        native.populate(mtm, &pui, entries::APP);
        pui.addItem(&NSMenuItem::separatorItem(mtm));
        // Only working AppKit application actions; no empty Services menu or
        // cross-process "Bring All to Front" that cannot control Pui windows.
        for (title, key, flags, selector) in [
            ("Hide Pui", "h", Flags::Command, sel!(hide:)),
            (
                "Hide Others",
                "h",
                Flags::Command | Flags::Option,
                sel!(hideOtherApplications:),
            ),
            ("Show All", "", Flags::empty(), sel!(unhideAllApplications:)),
        ] {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(title),
                    Some(selector),
                    &NSString::from_str(key),
                )
            };
            item.setKeyEquivalentModifierMask(flags);
            pui.addItem(&item);
        }
        pui.addItem(&NSMenuItem::separatorItem(mtm));
        native.populate(mtm, &pui, &[Entry::Action("Quit Pui", Command::Quit)]);
        for (title, entries) in entries::GROUPS {
            let menu = submenu(mtm, &native.menu, title);
            native.populate(mtm, &menu, entries);
            if *title == "Window" {
                app.setWindowsMenu(Some(&menu));
            }
        }
        app.setServicesMenu(None);
        app.setMainMenu(Some(&native.menu));
        Some(native)
    }
    fn populate(&mut self, mtm: MainThreadMarker, menu: &NSMenu, entries: &[Entry]) {
        let mut separator = false;
        for entry in entries.iter().filter(|e| e.supported()) {
            if matches!(entry, Entry::Separator) {
                separator = menu.numberOfItems() > 0;
                continue;
            }
            if separator {
                menu.addItem(&NSMenuItem::separatorItem(mtm));
                separator = false;
            }
            match entry {
                Entry::Action(title, command) => {
                    let (key, flags) = accelerator(*command, false);
                    // Selector signature matches Target::command; retained Target
                    // outlives the menu and is detached before it is released.
                    let item = unsafe {
                        NSMenuItem::initWithTitle_action_keyEquivalent(
                            NSMenuItem::alloc(mtm),
                            &NSString::from_str(title),
                            Some(sel!(puiCommand:)),
                            &NSString::from_str(&key),
                        )
                    };
                    unsafe {
                        item.setTarget(Some(&self._target));
                    }
                    item.setTag(*command as isize);
                    item.setKeyEquivalentModifierMask(flags);
                    menu.addItem(&item);
                    self.items.push((*command, item));
                }
                Entry::Submenu(title, children) => {
                    let child = submenu(mtm, menu, title);
                    self.populate(mtm, &child, children);
                }
                Entry::Separator => unreachable!(),
            }
        }
    }
    pub fn refresh(&mut self, state: MenuState) {
        // Updated by existing UI events, never by an idle timer or menu worker.
        if self.state == Some(state) {
            return;
        }
        for (command, item) in &self.items {
            let (key, flags) = accelerator(*command, state.composer_focused);
            item.setKeyEquivalent(&NSString::from_str(&key));
            item.setKeyEquivalentModifierMask(flags);
            item.setState(if state.checked(*command) == Some(true) {
                1
            } else {
                0
            });
            item.setEnabled(state.enabled(*command));
        }
        self.state = Some(state);
    }
}
