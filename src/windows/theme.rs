use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{COLORREF, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, FillRect, HBRUSH, HDC, SetBkColor, SetTextColor,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlSurface {
    Window,
    Panel,
    Field,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Primary,
    Secondary,
    Ghost,
    Surface,
    Destructive,
    Disabled,
    Focus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub window: COLORREF,
    pub panel: COLORREF,
    pub field: COLORREF,
    pub text: COLORREF,
    pub muted_text: COLORREF,
    pub primary: COLORREF,
    pub secondary: COLORREF,
    pub destructive: COLORREF,
    pub disabled: COLORREF,
    pub focus: COLORREF,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DensityMetrics {
    pub margin: i32,
    pub field_height: i32,
    pub button_height: i32,
    pub label_height: i32,
}

pub const PALETTE: Palette = Palette {
    window: rgb(6, 6, 7),
    panel: rgb(16, 16, 20),
    field: rgb(10, 10, 13),
    text: rgb(244, 244, 241),
    muted_text: rgb(184, 184, 176),
    primary: rgb(240, 179, 75),
    secondary: rgb(36, 39, 48),
    destructive: rgb(232, 95, 111),
    disabled: rgb(92, 99, 112),
    focus: rgb(241, 196, 15),
};

pub const COMPACT_DENSITY: DensityMetrics = DensityMetrics {
    margin: 16,
    field_height: 28,
    button_height: 32,
    label_height: 18,
};

pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::Primary => "primary",
        Role::Secondary => "secondary",
        Role::Ghost => "ghost",
        Role::Surface => "surface",
        Role::Destructive => "destructive",
        Role::Disabled => "disabled",
        Role::Focus => "focus",
    }
}

pub fn role_color(role: Role) -> COLORREF {
    match role {
        Role::Primary => PALETTE.primary,
        Role::Secondary => PALETTE.secondary,
        Role::Ghost | Role::Surface => PALETTE.panel,
        Role::Destructive => PALETTE.destructive,
        Role::Disabled => PALETTE.disabled,
        Role::Focus => PALETTE.focus,
    }
}

pub fn apply_control_colors(hdc: HDC, surface: ControlSurface, enabled: bool) -> HBRUSH {
    let background = match surface {
        ControlSurface::Window => PALETTE.window,
        ControlSurface::Panel => PALETTE.panel,
        ControlSurface::Field => PALETTE.field,
    };
    let text = if enabled {
        PALETTE.text
    } else {
        role_color(Role::Disabled)
    };
    unsafe {
        SetTextColor(hdc, text);
        SetBkColor(hdc, background);
    }
    brush_for(surface)
}

pub fn paint_window_background(hwnd: windows_sys::Win32::Foundation::HWND, hdc: HDC) -> bool {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        if GetClientRect(hwnd, &mut rect) == 0 {
            return false;
        }
        FillRect(hdc, &rect, brush_for(ControlSurface::Window));
    }
    true
}

pub fn brush_for(surface: ControlSurface) -> HBRUSH {
    match surface {
        ControlSurface::Window => cached_brush(&WINDOW_BRUSH, PALETTE.window),
        ControlSurface::Panel => cached_brush(&PANEL_BRUSH, PALETTE.panel),
        ControlSurface::Field => cached_brush(&FIELD_BRUSH, PALETTE.field),
    }
}

pub fn accessibility_summary() -> String {
    format!(
        "Windows native theme roles: {}, {}, {}, {}, {}, {}, {}; compact density {}px buttons / {}px fields.",
        role_name(Role::Primary),
        role_name(Role::Secondary),
        role_name(Role::Ghost),
        role_name(Role::Surface),
        role_name(Role::Destructive),
        role_name(Role::Disabled),
        role_name(Role::Focus),
        COMPACT_DENSITY.button_height,
        COMPACT_DENSITY.field_height
    )
}

static WINDOW_BRUSH: OnceLock<usize> = OnceLock::new();
static PANEL_BRUSH: OnceLock<usize> = OnceLock::new();
static FIELD_BRUSH: OnceLock<usize> = OnceLock::new();

fn cached_brush(slot: &OnceLock<usize>, color: COLORREF) -> HBRUSH {
    *slot.get_or_init(|| unsafe { CreateSolidBrush(color) as usize }) as HBRUSH
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    (red as COLORREF) | ((green as COLORREF) << 8) | ((blue as COLORREF) << 16)
}

#[cfg(test)]
mod tests {
    use super::{COMPACT_DENSITY, ControlSurface, PALETTE, Role, role_color, role_name};

    #[test]
    fn exposes_design_role_names_and_density_metrics() {
        assert_eq!(role_name(Role::Primary), "primary");
        assert_eq!(role_name(Role::Destructive), "destructive");
        assert_eq!(COMPACT_DENSITY.button_height, 32);
        assert_eq!(COMPACT_DENSITY.field_height, 28);
    }

    #[test]
    fn maps_design_roles_to_palette_colors() {
        assert_eq!(role_color(Role::Primary), PALETTE.primary);
        assert_eq!(role_color(Role::Secondary), PALETTE.secondary);
        assert_eq!(role_color(Role::Destructive), PALETTE.destructive);
        assert_ne!(PALETTE.window, PALETTE.text);
        assert_eq!(ControlSurface::Field, ControlSurface::Field);
    }
}
