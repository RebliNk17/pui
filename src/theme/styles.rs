//! Curated surface, text and accent families; semantic status colors stay consistent.
use super::{Color32, NEUTRAL_DARK, NEUTRAL_LIGHT, Palette, Theme};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DarkStyle {
    Midnight,
    Forest,
    Plum,
    #[default]
    Neutral,
}
impl DarkStyle {
    pub const ALL: [Self; 4] = [Self::Neutral, Self::Midnight, Self::Forest, Self::Plum];
    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Forest => "Forest",
            Self::Plum => "Plum",
            Self::Neutral => "Classic",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Midnight => "Deep navy · sky blue",
            Self::Forest => "Evergreen · mint",
            Self::Plum => "Aubergine · lavender",
            Self::Neutral => "Original charcoal · violet",
        }
    }
    pub fn palette(self) -> &'static Palette {
        match self {
            Self::Midnight => &MIDNIGHT,
            Self::Forest => &FOREST,
            Self::Plum => &PLUM,
            Self::Neutral => &NEUTRAL_DARK,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LightStyle {
    Ivory,
    Sage,
    Rose,
    #[default]
    Neutral,
}
impl LightStyle {
    pub const ALL: [Self; 4] = [Self::Neutral, Self::Ivory, Self::Sage, Self::Rose];
    pub fn label(self) -> &'static str {
        match self {
            Self::Ivory => "Ivory",
            Self::Sage => "Sage",
            Self::Rose => "Rose",
            Self::Neutral => "Classic",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Ivory => "Warm cream · terracotta",
            Self::Sage => "Soft sage · forest green",
            Self::Rose => "Blush white · berry",
            Self::Neutral => "Original soft white · violet",
        }
    }
    pub fn palette(self) -> &'static Palette {
        match self {
            Self::Ivory => &IVORY,
            Self::Sage => &SAGE,
            Self::Rose => &ROSE,
            Self::Neutral => &NEUTRAL_LIGHT,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Styles {
    pub dark: DarkStyle,
    pub light: LightStyle,
}
impl Styles {
    pub fn palette(self, mode: Theme) -> &'static Palette {
        match mode {
            Theme::Dark => self.dark.palette(),
            Theme::Light => self.light.palette(),
        }
    }
}

const MIDNIGHT: Palette = Palette {
    bg: Color32::from_rgb(17, 24, 39),
    sidebar: Color32::from_rgb(23, 33, 50),
    surface: Color32::from_rgb(29, 41, 61),
    hover: Color32::from_rgb(38, 53, 75),
    border: Color32::from_rgb(55, 72, 96),
    text: Color32::from_rgb(233, 240, 250),
    muted: Color32::from_rgb(171, 187, 208),
    dim: Color32::from_rgb(150, 168, 192),
    accent: Color32::from_rgb(126, 184, 255),
    selection_bg: Color32::from_rgb(47, 81, 125),
    blue: Color32::from_rgb(91, 167, 245),
    code_bg: Color32::from_rgb(13, 20, 33),
    user_bg: Color32::from_rgb(32, 46, 68),
    accent_bg: Color32::from_rgb(31, 54, 85),
    settings_bg: Color32::from_rgb(14, 21, 35),
    settings_nav: Color32::from_rgb(20, 29, 45),
    settings_card: Color32::from_rgb(26, 38, 57),
    switch_on: Color32::from_rgb(126, 184, 255),
    switch_off: Color32::from_rgb(65, 80, 103),
    ..NEUTRAL_DARK
};

const FOREST: Palette = Palette {
    bg: Color32::from_rgb(18, 29, 25),
    sidebar: Color32::from_rgb(24, 38, 32),
    surface: Color32::from_rgb(30, 46, 38),
    hover: Color32::from_rgb(42, 61, 49),
    border: Color32::from_rgb(58, 78, 64),
    text: Color32::from_rgb(231, 241, 232),
    muted: Color32::from_rgb(176, 196, 180),
    dim: Color32::from_rgb(154, 178, 159),
    accent: Color32::from_rgb(132, 214, 167),
    selection_bg: Color32::from_rgb(49, 95, 68),
    blue: Color32::from_rgb(91, 167, 245),
    code_bg: Color32::from_rgb(14, 24, 20),
    user_bg: Color32::from_rgb(34, 53, 42),
    accent_bg: Color32::from_rgb(31, 61, 43),
    settings_bg: Color32::from_rgb(15, 26, 22),
    settings_nav: Color32::from_rgb(21, 35, 29),
    settings_card: Color32::from_rgb(28, 44, 36),
    switch_on: Color32::from_rgb(132, 214, 167),
    switch_off: Color32::from_rgb(66, 88, 73),
    ..NEUTRAL_DARK
};

const PLUM: Palette = Palette {
    bg: Color32::from_rgb(31, 23, 35),
    sidebar: Color32::from_rgb(40, 29, 45),
    surface: Color32::from_rgb(49, 36, 55),
    hover: Color32::from_rgb(65, 47, 73),
    border: Color32::from_rgb(84, 63, 94),
    text: Color32::from_rgb(244, 232, 248),
    muted: Color32::from_rgb(199, 177, 207),
    dim: Color32::from_rgb(178, 154, 188),
    accent: Color32::from_rgb(216, 169, 239),
    selection_bg: Color32::from_rgb(105, 64, 125),
    blue: Color32::from_rgb(91, 167, 245),
    code_bg: Color32::from_rgb(25, 18, 30),
    user_bg: Color32::from_rgb(56, 40, 63),
    accent_bg: Color32::from_rgb(67, 40, 79),
    settings_bg: Color32::from_rgb(27, 19, 31),
    settings_nav: Color32::from_rgb(36, 26, 41),
    settings_card: Color32::from_rgb(47, 34, 53),
    switch_on: Color32::from_rgb(216, 169, 239),
    switch_off: Color32::from_rgb(91, 68, 101),
    ..NEUTRAL_DARK
};

const IVORY: Palette = Palette {
    bg: Color32::from_rgb(252, 248, 240),
    sidebar: Color32::from_rgb(244, 236, 223),
    surface: Color32::from_rgb(255, 253, 247),
    hover: Color32::from_rgb(233, 222, 204),
    border: Color32::from_rgb(214, 198, 175),
    text: Color32::from_rgb(52, 42, 33),
    muted: Color32::from_rgb(101, 82, 65),
    dim: Color32::from_rgb(110, 91, 73),
    accent: Color32::from_rgb(157, 67, 43),
    selection_bg: Color32::from_rgb(218, 171, 145),
    amber: Color32::from_rgb(140, 91, 13),
    code_bg: Color32::from_rgb(242, 234, 220),
    user_bg: Color32::from_rgb(238, 226, 207),
    accent_bg: Color32::from_rgb(249, 230, 215),
    settings_bg: Color32::from_rgb(252, 248, 240),
    settings_nav: Color32::from_rgb(244, 236, 223),
    settings_card: Color32::from_rgb(255, 253, 247),
    switch_on: Color32::from_rgb(157, 67, 43),
    switch_off: Color32::from_rgb(193, 175, 151),
    ..NEUTRAL_LIGHT
};

const SAGE: Palette = Palette {
    bg: Color32::from_rgb(245, 249, 242),
    sidebar: Color32::from_rgb(232, 240, 227),
    surface: Color32::from_rgb(252, 254, 249),
    hover: Color32::from_rgb(217, 230, 211),
    border: Color32::from_rgb(190, 207, 183),
    text: Color32::from_rgb(33, 48, 36),
    muted: Color32::from_rgb(72, 94, 72),
    dim: Color32::from_rgb(83, 105, 83),
    accent: Color32::from_rgb(37, 105, 68),
    selection_bg: Color32::from_rgb(153, 192, 158),
    code_bg: Color32::from_rgb(232, 240, 226),
    user_bg: Color32::from_rgb(225, 236, 218),
    accent_bg: Color32::from_rgb(220, 239, 220),
    settings_bg: Color32::from_rgb(245, 249, 242),
    settings_nav: Color32::from_rgb(232, 240, 227),
    settings_card: Color32::from_rgb(252, 254, 249),
    switch_on: Color32::from_rgb(37, 105, 68),
    switch_off: Color32::from_rgb(163, 188, 159),
    ..NEUTRAL_LIGHT
};

const ROSE: Palette = Palette {
    bg: Color32::from_rgb(253, 247, 249),
    sidebar: Color32::from_rgb(246, 234, 240),
    surface: Color32::from_rgb(255, 252, 254),
    hover: Color32::from_rgb(237, 219, 230),
    border: Color32::from_rgb(219, 192, 208),
    text: Color32::from_rgb(54, 35, 49),
    muted: Color32::from_rgb(104, 76, 96),
    dim: Color32::from_rgb(116, 88, 108),
    accent: Color32::from_rgb(153, 55, 105),
    selection_bg: Color32::from_rgb(219, 166, 196),
    amber: Color32::from_rgb(140, 91, 13),
    code_bg: Color32::from_rgb(246, 233, 239),
    user_bg: Color32::from_rgb(242, 226, 235),
    accent_bg: Color32::from_rgb(251, 225, 238),
    settings_bg: Color32::from_rgb(253, 247, 249),
    settings_nav: Color32::from_rgb(246, 234, 240),
    settings_card: Color32::from_rgb(255, 252, 254),
    switch_on: Color32::from_rgb(153, 55, 105),
    switch_off: Color32::from_rgb(201, 174, 190),
    ..NEUTRAL_LIGHT
};
