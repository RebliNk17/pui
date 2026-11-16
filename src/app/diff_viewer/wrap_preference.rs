//! One presentation-only preference per app window; saved by the existing app store.
use super::*;

fn key() -> egui::Id {
    egui::Id::new("diff-wrap-preference")
}

pub(in crate::app) fn init(ctx: &egui::Context, wrap: bool) {
    ctx.data_mut(|data| data.insert_temp(key(), wrap));
}

pub(in crate::app) fn get(ctx: &egui::Context) -> Option<bool> {
    ctx.data(|data| data.get_temp::<bool>(key()))
}
