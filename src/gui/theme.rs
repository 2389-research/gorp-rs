// ABOUTME: Design system for gorp desktop app - "Obsidian Terminal" aesthetic
// ABOUTME: Color palette, spacing scale, and styled component helpers

use iced::widget::{button, container, text, text_input};
use iced::{Border, Color, Shadow, Theme, Vector};

// ============================================================================
// COLOR PALETTE - Obsidian Terminal
// Deep layered darks with warm amber accents and cool slate interactions
// ============================================================================

pub mod colors {
    use iced::Color;

    // Background layers (darkest to lightest)
    pub const BG_BASE: Color = Color::from_rgb(0.067, 0.067, 0.082); // #111115 - deepest
    pub const BG_SURFACE: Color = Color::from_rgb(0.098, 0.098, 0.118); // #19191e - cards/panels
    pub const BG_ELEVATED: Color = Color::from_rgb(0.133, 0.133, 0.157); // #222228 - hover/elevated
    pub const BG_OVERLAY: Color = Color::from_rgb(0.176, 0.176, 0.208); // #2d2d35 - inputs/wells

    // Accent colors
    pub const ACCENT_PRIMARY: Color = Color::from_rgb(0.537, 0.706, 0.980); // #89b4fa - blue
    pub const ACCENT_WARM: Color = Color::from_rgb(0.976, 0.733, 0.306); // #f9bb4e - amber/gold
    pub const ACCENT_SUCCESS: Color = Color::from_rgb(0.651, 0.890, 0.631); // #a6e3a1 - green
    pub const ACCENT_DANGER: Color = Color::from_rgb(0.953, 0.545, 0.659); // #f38ba8 - red/pink

    // Text hierarchy
    pub const TEXT_PRIMARY: Color = Color::from_rgb(0.898, 0.914, 0.957); // #e5e9f4 - main text
    pub const TEXT_SECONDARY: Color = Color::from_rgb(0.627, 0.659, 0.745); // #a0a8be - muted
    pub const TEXT_TERTIARY: Color = Color::from_rgb(0.439, 0.467, 0.549); // #70778c - very muted
    pub const TEXT_INVERSE: Color = Color::from_rgb(0.067, 0.067, 0.082); // #111115 - on light bg

    // Borders
    pub const BORDER_SUBTLE: Color = Color::from_rgb(0.196, 0.200, 0.243); // #32333e
    pub const BORDER_DEFAULT: Color = Color::from_rgb(0.275, 0.282, 0.341); // #464857
    pub const BORDER_FOCUS: Color = Color::from_rgb(0.537, 0.706, 0.980); // same as ACCENT_PRIMARY

    // Status colors
    pub const STATUS_ONLINE: Color = Color::from_rgb(0.651, 0.890, 0.631); // green
    pub const STATUS_AWAY: Color = Color::from_rgb(0.976, 0.733, 0.306); // amber
    pub const STATUS_OFFLINE: Color = Color::from_rgb(0.439, 0.467, 0.549); // gray

    // Shadows (for depth)
    pub const SHADOW: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.4);

    // Transparent overlays
    pub const OVERLAY_BACKDROP: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.65);
}

// ============================================================================
// SPACING SCALE - 4px base unit
// ============================================================================

pub mod spacing {
    pub const XXXS: f32 = 2.0;
    pub const XXS: f32 = 4.0;
    pub const XS: f32 = 8.0;
    pub const SM: f32 = 12.0;
    pub const MD: f32 = 16.0;
    pub const LG: f32 = 24.0;
    pub const XL: f32 = 32.0;
    pub const XXL: f32 = 48.0;
    pub const XXXL: f32 = 64.0;
}

// ============================================================================
// BORDER RADIUS
// ============================================================================

pub mod radius {
    pub const NONE: f32 = 0.0;
    pub const SM: f32 = 4.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const FULL: f32 = 9999.0; // pill shape
}

// ============================================================================
// TYPOGRAPHY SIZES
// ============================================================================

pub mod text_size {
    pub const CAPTION: f32 = 11.0;
    pub const SMALL: f32 = 12.0;
    pub const BODY: f32 = 14.0;
    pub const LARGE: f32 = 16.0;
    pub const TITLE: f32 = 20.0;
    pub const HEADING: f32 = 24.0;
    pub const DISPLAY: f32 = 32.0;
}

// ============================================================================
// COMPONENT DIMENSIONS
// ============================================================================

pub const SIDEBAR_WIDTH: f32 = 260.0;
pub const HEADER_HEIGHT: f32 = 56.0;
pub const INPUT_HEIGHT: f32 = 44.0;
pub const BUTTON_HEIGHT: f32 = 36.0;
pub const ROOM_CARD_HEIGHT: f32 = 52.0;
pub const MESSAGE_MAX_WIDTH: f32 = 600.0;

// ============================================================================
// CONTAINER STYLES
// ============================================================================

/// Base surface container (cards, panels)
pub fn surface_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_SURFACE.into()),
        border: Border {
            color: colors::BORDER_SUBTLE,
            width: 1.0,
            radius: radius::MD.into(),
        },
        ..Default::default()
    }
}

/// Elevated container with shadow
pub fn elevated_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_ELEVATED.into()),
        border: Border {
            color: colors::BORDER_DEFAULT,
            width: 1.0,
            radius: radius::LG.into(),
        },
        shadow: Shadow {
            color: colors::SHADOW,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..Default::default()
    }
}

/// Sidebar container
pub fn sidebar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_SURFACE.into()),
        border: Border {
            color: colors::BORDER_SUBTLE,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

/// Main content area
pub fn content_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_BASE.into()),
        ..Default::default()
    }
}

/// Header/toolbar style
pub fn header_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_SURFACE.into()),
        border: Border {
            color: colors::BORDER_SUBTLE,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

/// Input well (sunken appearance)
pub fn input_well_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_OVERLAY.into()),
        border: Border {
            color: colors::BORDER_SUBTLE,
            width: 1.0,
            radius: radius::MD.into(),
        },
        ..Default::default()
    }
}

/// Modal backdrop
pub fn backdrop_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::OVERLAY_BACKDROP.into()),
        ..Default::default()
    }
}

/// Modal content container
pub fn modal_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_SURFACE.into()),
        border: Border {
            color: colors::ACCENT_PRIMARY,
            width: 1.0,
            radius: radius::LG.into(),
        },
        shadow: Shadow {
            color: colors::SHADOW,
            offset: Vector::new(0.0, 8.0),
            blur_radius: 32.0,
        },
        ..Default::default()
    }
}

/// Room card - default state
pub fn room_card_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::TRANSPARENT.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius::MD.into(),
        },
        ..Default::default()
    }
}

/// Room card - active/selected state
pub fn room_card_active_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_ELEVATED.into()),
        border: Border {
            color: colors::ACCENT_PRIMARY,
            width: 1.0,
            radius: radius::MD.into(),
        },
        ..Default::default()
    }
}

/// Message bubble - own message
pub fn message_own_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.537, 0.706, 0.980, 0.15).into()), // accent with alpha
        border: Border {
            color: Color::from_rgba(0.537, 0.706, 0.980, 0.3),
            width: 1.0,
            radius: radius::LG.into(),
        },
        ..Default::default()
    }
}

/// Message bubble - other's message
pub fn message_other_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_ELEVATED.into()),
        border: Border {
            color: colors::BORDER_SUBTLE,
            width: 1.0,
            radius: radius::LG.into(),
        },
        ..Default::default()
    }
}

/// Stat card (dashboard)
pub fn stat_card_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::BG_SURFACE.into()),
        border: Border {
            color: colors::BORDER_SUBTLE,
            width: 1.0,
            radius: radius::LG.into(),
        },
        ..Default::default()
    }
}

/// Badge/chip container
pub fn badge_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors::ACCENT_WARM.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius::FULL.into(),
        },
        ..Default::default()
    }
}

/// Status dot container (online/offline)
pub fn status_dot_style(status: &str) -> impl Fn(&Theme) -> container::Style {
    let color = match status {
        "online" | "connected" => colors::STATUS_ONLINE,
        "away" | "connecting" => colors::STATUS_AWAY,
        _ => colors::STATUS_OFFLINE,
    };
    move |_theme: &Theme| container::Style {
        background: Some(color.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius::FULL.into(),
        },
        ..Default::default()
    }
}

// ============================================================================
// BUTTON STYLES
// ============================================================================

/// Primary button style
pub fn button_primary(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(colors::ACCENT_PRIMARY.into()),
        text_color: colors::TEXT_INVERSE,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius::MD.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.537, 0.706, 0.980, 0.3),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
    };

    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            background: Some(Color::from_rgb(0.596, 0.757, 0.996).into()), // lighter
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Color::from_rgb(0.478, 0.655, 0.965).into()), // darker
            ..base
        },
        button::Status::Disabled => button::Style {
            background: Some(colors::BG_OVERLAY.into()),
            text_color: colors::TEXT_TERTIARY,
            shadow: Shadow::default(),
            ..base
        },
    }
}

/// Secondary/ghost button style
pub fn button_secondary(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Color::TRANSPARENT.into()),
        text_color: colors::TEXT_SECONDARY,
        border: Border {
            color: colors::BORDER_DEFAULT,
            width: 1.0,
            radius: radius::MD.into(),
        },
        shadow: Shadow::default(),
    };

    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            background: Some(colors::BG_ELEVATED.into()),
            text_color: colors::TEXT_PRIMARY,
            border: Border {
                color: colors::ACCENT_PRIMARY,
                width: 1.0,
                radius: radius::MD.into(),
            },
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(colors::BG_OVERLAY.into()),
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: colors::TEXT_TERTIARY,
            border: Border {
                color: colors::BORDER_SUBTLE,
                width: 1.0,
                radius: radius::MD.into(),
            },
            ..base
        },
    }
}

/// Ghost button (minimal, no border)
pub fn button_ghost(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Color::TRANSPARENT.into()),
        text_color: colors::TEXT_SECONDARY,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius::MD.into(),
        },
        shadow: Shadow::default(),
    };

    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            background: Some(colors::BG_ELEVATED.into()),
            text_color: colors::TEXT_PRIMARY,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(colors::BG_OVERLAY.into()),
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: colors::TEXT_TERTIARY,
            ..base
        },
    }
}

/// Nav button - default state
pub fn button_nav(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Color::TRANSPARENT.into()),
        text_color: colors::TEXT_SECONDARY,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius::MD.into(),
        },
        shadow: Shadow::default(),
    };

    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            background: Some(colors::BG_ELEVATED.into()),
            text_color: colors::TEXT_PRIMARY,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(colors::BG_OVERLAY.into()),
            ..base
        },
        button::Status::Disabled => base,
    }
}

/// Nav button - active/selected state
pub fn button_nav_active(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(colors::BG_ELEVATED.into()),
        text_color: colors::ACCENT_PRIMARY,
        border: Border {
            color: colors::ACCENT_PRIMARY,
            width: 0.0,
            radius: radius::MD.into(),
        },
        shadow: Shadow::default(),
    };

    match status {
        button::Status::Active | button::Status::Hovered | button::Status::Pressed => base,
        button::Status::Disabled => button::Style {
            text_color: colors::TEXT_TERTIARY,
            ..base
        },
    }
}

/// Room button - default state
pub fn button_room(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Color::TRANSPARENT.into()),
        text_color: colors::TEXT_SECONDARY,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius::MD.into(),
        },
        shadow: Shadow::default(),
    };

    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            background: Some(colors::BG_ELEVATED.into()),
            text_color: colors::TEXT_PRIMARY,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(colors::BG_OVERLAY.into()),
            ..base
        },
        button::Status::Disabled => base,
    }
}

/// Room button - active/selected state
pub fn button_room_active(_theme: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(colors::BG_ELEVATED.into()),
        text_color: colors::TEXT_PRIMARY,
        border: Border {
            color: colors::ACCENT_PRIMARY,
            width: 1.0,
            radius: radius::MD.into(),
        },
        shadow: Shadow::default(),
    };

    match status {
        button::Status::Active | button::Status::Hovered | button::Status::Pressed => base,
        button::Status::Disabled => base,
    }
}

// ============================================================================
// TEXT INPUT STYLES
// ============================================================================

/// Primary text input style
pub fn text_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let base = text_input::Style {
        background: colors::BG_OVERLAY.into(),
        border: Border {
            color: colors::BORDER_SUBTLE,
            width: 1.0,
            radius: radius::MD.into(),
        },
        icon: colors::TEXT_TERTIARY,
        placeholder: colors::TEXT_TERTIARY,
        value: colors::TEXT_PRIMARY,
        selection: colors::ACCENT_PRIMARY,
    };

    match status {
        text_input::Status::Active => base,
        text_input::Status::Hovered => text_input::Style {
            border: Border {
                color: colors::BORDER_DEFAULT,
                width: 1.0,
                radius: radius::MD.into(),
            },
            ..base
        },
        text_input::Status::Focused => text_input::Style {
            border: Border {
                color: colors::ACCENT_PRIMARY,
                width: 2.0,
                radius: radius::MD.into(),
            },
            ..base
        },
        text_input::Status::Disabled => text_input::Style {
            background: colors::BG_SURFACE.into(),
            value: colors::TEXT_TERTIARY,
            ..base
        },
    }
}

// ============================================================================
// TEXT HELPERS
// ============================================================================

/// Create styled text with primary color
pub fn text_primary(content: impl ToString) -> text::Text<'static> {
    text(content.to_string()).color(colors::TEXT_PRIMARY)
}

/// Create styled text with secondary color
pub fn text_secondary(content: impl ToString) -> text::Text<'static> {
    text(content.to_string()).color(colors::TEXT_SECONDARY)
}

/// Create styled text with tertiary (muted) color
pub fn text_muted(content: impl ToString) -> text::Text<'static> {
    text(content.to_string()).color(colors::TEXT_TERTIARY)
}

/// Create styled text with accent color
pub fn text_accent(content: impl ToString) -> text::Text<'static> {
    text(content.to_string()).color(colors::ACCENT_PRIMARY)
}

/// Create styled text with warm accent (amber)
pub fn text_warm(content: impl ToString) -> text::Text<'static> {
    text(content.to_string()).color(colors::ACCENT_WARM)
}

/// Create styled text with success color
pub fn text_success(content: impl ToString) -> text::Text<'static> {
    text(content.to_string()).color(colors::ACCENT_SUCCESS)
}

/// Create styled text with danger color
pub fn text_danger(content: impl ToString) -> text::Text<'static> {
    text(content.to_string()).color(colors::ACCENT_DANGER)
}
