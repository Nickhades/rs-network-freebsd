//! Self-drawn vector iconography.
//!
//! netcore intentionally does not use an icon theme. Every glyph is painted by
//! Cairo from a 24x24 design grid, so the UI keeps a consistent visual language
//! and automatically follows the application's own foreground/accent palette.

use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;
use std::time::Duration;

use gtk::cairo::{Context, FillRule, LineCap, LineJoin};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Wifi,
    WifiOffline,
    WifiAcquiring,
    Ethernet,
    EthernetOffline,
    Signal(u8),
    Lock,
    Refresh,
    Add,
    Close,
    Minimize,
    Maximize,
    Restore,
    Sun,
    Moon,
    Chip,
    Eye,
    EyeOff,
}

#[derive(Clone)]
pub struct NetcoreIcon {
    area: gtk::DrawingArea,
    kind: Rc<Cell<IconKind>>,
}

impl NetcoreIcon {
    pub fn new(kind: IconKind, size: i32) -> Self {
        let area = gtk::DrawingArea::new();
        area.set_content_width(size);
        area.set_content_height(size);
        area.set_size_request(size, size);
        area.set_halign(gtk::Align::Center);
        area.set_valign(gtk::Align::Center);
        area.set_can_focus(false);
        area.set_can_target(false);
        area.add_css_class("netcore-icon");

        let kind_cell = Rc::new(Cell::new(kind));
        let draw_kind = Rc::clone(&kind_cell);
        area.set_draw_func(move |area, cr, width, height| {
            draw(area, cr, width as f64, height as f64, draw_kind.get());
        });

        Self {
            area,
            kind: kind_cell,
        }
    }

    pub fn widget(&self) -> gtk::DrawingArea {
        self.area.clone()
    }

    pub fn set_kind(&self, kind: IconKind) {
        if self.kind.get() != kind {
            self.kind.set(kind);
            self.area.queue_draw();
        }
    }
}

pub fn icon_button(kind: IconKind, size: i32, tooltip: &str) -> (gtk::Button, NetcoreIcon) {
    let icon = NetcoreIcon::new(kind, size);
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("netcore-icon-button");
    button.set_child(Some(&icon.widget()));
    button.set_tooltip_text(Some(tooltip));
    button.set_valign(gtk::Align::Center);
    (button, icon)
}

fn draw(area: &gtk::DrawingArea, cr: &Context, width: f64, height: f64, kind: IconKind) {
    let color = area.color();
    let scale = width.min(height) / 24.0;
    if scale <= 0.0 {
        return;
    }
    let offset_x = (width - 24.0 * scale) / 2.0;
    let offset_y = (height - 24.0 * scale) / 2.0;

    cr.save().expect("cairo save");
    cr.translate(offset_x, offset_y);
    cr.scale(scale, scale);
    cr.set_line_width(1.9);
    cr.set_line_cap(LineCap::Round);
    cr.set_line_join(LineJoin::Round);
    set_color(cr, &color, 1.0);

    match kind {
        IconKind::Wifi => draw_wifi(cr, 1.0, false),
        IconKind::WifiOffline => {
            draw_wifi(cr, 0.35, false);
            set_color(cr, &color, 1.0);
            cr.move_to(5.0, 20.0);
            cr.line_to(19.0, 4.0);
            cr.stroke().expect("cairo stroke");
        }
        IconKind::WifiAcquiring => {
            draw_wifi(cr, 0.65, true);
        }
        IconKind::Ethernet => draw_ethernet(cr, 1.0, false),
        IconKind::EthernetOffline => draw_ethernet(cr, 1.0, true),
        IconKind::Signal(quality) => draw_signal(cr, quality, &color),
        IconKind::Lock => draw_lock(cr),
        IconKind::Refresh => draw_refresh(cr),
        IconKind::Add => {
            cr.move_to(12.0, 5.0);
            cr.line_to(12.0, 19.0);
            cr.move_to(5.0, 12.0);
            cr.line_to(19.0, 12.0);
            cr.stroke().expect("cairo stroke");
        }
        IconKind::Close => {
            cr.move_to(6.0, 6.0);
            cr.line_to(18.0, 18.0);
            cr.move_to(18.0, 6.0);
            cr.line_to(6.0, 18.0);
            cr.stroke().expect("cairo stroke");
        }
        IconKind::Minimize => {
            cr.move_to(6.0, 17.0);
            cr.line_to(18.0, 17.0);
            cr.stroke().expect("cairo stroke");
        }
        IconKind::Maximize => {
            rounded_rect(cr, 6.5, 6.5, 11.0, 11.0, 1.5);
            cr.stroke().expect("cairo stroke");
        }
        IconKind::Restore => {
            rounded_rect(cr, 5.5, 8.5, 10.0, 10.0, 1.5);
            cr.stroke().expect("cairo stroke");
            cr.move_to(8.5, 8.5);
            cr.line_to(8.5, 5.5);
            cr.line_to(18.5, 5.5);
            cr.line_to(18.5, 15.5);
            cr.line_to(15.5, 15.5);
            cr.stroke().expect("cairo stroke");
        }
        IconKind::Sun => draw_sun(cr),
        IconKind::Moon => draw_moon(cr),
        IconKind::Chip => draw_chip(cr),
        IconKind::Eye => draw_eye(cr, false),
        IconKind::EyeOff => draw_eye(cr, true),
    }

    cr.restore().expect("cairo restore");
}

fn set_color(cr: &Context, color: &gtk::gdk::RGBA, alpha: f64) {
    cr.set_source_rgba(
        f64::from(color.red()),
        f64::from(color.green()),
        f64::from(color.blue()),
        f64::from(color.alpha()) * alpha,
    );
}

fn draw_wifi(cr: &Context, alpha: f64, dashed: bool) {
    let _ = alpha;
    if dashed {
        cr.set_dash(&[1.2, 2.2], 0.0);
    }
    cr.arc(12.0, 19.0, 1.15, 0.0, 2.0 * PI);
    cr.fill().expect("cairo fill");
    for radius in [5.0, 9.0, 13.0] {
        cr.arc(12.0, 19.5, radius, -2.36, -0.78);
        cr.stroke().expect("cairo stroke");
    }
    cr.set_dash(&[], 0.0);
    cr.set_line_width(1.9);
}

fn draw_ethernet(cr: &Context, alpha: f64, offline: bool) {
    let _ = alpha;
    rounded_rect(cr, 5.0, 8.0, 14.0, 10.0, 2.0);
    cr.stroke().expect("cairo stroke");
    cr.move_to(8.5, 8.0);
    cr.line_to(8.5, 5.5);
    cr.move_to(12.0, 8.0);
    cr.line_to(12.0, 5.5);
    cr.move_to(15.5, 8.0);
    cr.line_to(15.5, 5.5);
    cr.move_to(12.0, 18.0);
    cr.line_to(12.0, 21.0);
    cr.stroke().expect("cairo stroke");
    if offline {
        cr.move_to(4.5, 20.5);
        cr.line_to(19.5, 3.5);
        cr.stroke().expect("cairo stroke");
    }
}

fn draw_signal(cr: &Context, quality: u8, color: &gtk::gdk::RGBA) {
    let active = if quality == 0 {
        0
    } else {
        usize::from(quality).saturating_mul(4).div_ceil(100)
    };
    cr.set_line_width(3.1);
    for index in 0..4 {
        let x = 4.5 + index as f64 * 5.0;
        let height = 5.0 + index as f64 * 3.0;
        set_color(cr, color, if index < active { 1.0 } else { 0.14 });
        cr.move_to(x, 20.0);
        cr.line_to(x, 20.0 - height);
        cr.stroke().expect("cairo stroke");
    }
    set_color(cr, color, 1.0);
}

fn draw_lock(cr: &Context) {
    rounded_rect(cr, 7.0, 11.0, 10.0, 9.0, 1.8);
    cr.stroke().expect("cairo stroke");
    cr.arc(12.0, 11.0, 3.4, PI, 2.0 * PI);
    cr.stroke().expect("cairo stroke");
    cr.move_to(12.0, 14.0);
    cr.line_to(12.0, 16.0);
    cr.stroke().expect("cairo stroke");
}

fn draw_refresh(cr: &Context) {
    let angle = 2.18;
    cr.arc(12.0, 12.0, 6.4, -2.42, angle);
    cr.stroke().expect("cairo stroke");
    let x = 12.0 + 6.4 * angle.cos();
    let y = 12.0 + 6.4 * angle.sin();
    let tangent = (-angle.sin(), angle.cos());
    let normal = (angle.cos(), angle.sin());
    cr.move_to(x, y);
    cr.line_to(
        x - 3.2 * tangent.0 + 2.0 * normal.0,
        y - 3.2 * tangent.1 + 2.0 * normal.1,
    );
    cr.line_to(
        x - 3.2 * tangent.0 - 2.0 * normal.0,
        y - 3.2 * tangent.1 - 2.0 * normal.1,
    );
    cr.close_path();
    cr.fill().expect("cairo fill");
}

fn draw_sun(cr: &Context) {
    cr.arc(12.0, 12.0, 3.5, 0.0, 2.0 * PI);
    cr.stroke().expect("cairo stroke");
    for index in 0..8 {
        let angle = index as f64 * PI / 4.0;
        cr.move_to(12.0 + 6.0 * angle.cos(), 12.0 + 6.0 * angle.sin());
        cr.line_to(12.0 + 8.4 * angle.cos(), 12.0 + 8.4 * angle.sin());
    }
    cr.stroke().expect("cairo stroke");
}

fn draw_moon(cr: &Context) {
    cr.set_fill_rule(FillRule::EvenOdd);
    cr.arc(13.0, 12.0, 6.2, 0.0, 2.0 * PI);
    cr.new_sub_path();
    cr.arc(16.2, 10.0, 5.4, 0.0, 2.0 * PI);
    cr.fill().expect("cairo fill");
    cr.set_fill_rule(FillRule::Winding);
}

fn draw_chip(cr: &Context) {
    rounded_rect(cr, 7.0, 7.0, 10.0, 10.0, 1.8);
    cr.stroke().expect("cairo stroke");
    for value in [9.5, 12.0, 14.5] {
        cr.move_to(value, 7.0);
        cr.line_to(value, 4.0);
        cr.move_to(value, 17.0);
        cr.line_to(value, 20.0);
        cr.move_to(7.0, value);
        cr.line_to(4.0, value);
        cr.move_to(17.0, value);
        cr.line_to(20.0, value);
    }
    cr.stroke().expect("cairo stroke");
    cr.arc(12.0, 12.0, 1.8, 0.0, 2.0 * PI);
    cr.stroke().expect("cairo stroke");
}

fn draw_eye(cr: &Context, off: bool) {
    cr.move_to(3.5, 12.0);
    cr.curve_to(7.0, 6.5, 17.0, 6.5, 20.5, 12.0);
    cr.curve_to(17.0, 17.5, 7.0, 17.5, 3.5, 12.0);
    cr.close_path();
    cr.stroke().expect("cairo stroke");
    cr.arc(12.0, 12.0, 2.4, 0.0, 2.0 * PI);
    cr.stroke().expect("cairo stroke");
    if off {
        cr.move_to(4.0, 20.0);
        cr.line_to(20.0, 4.0);
        cr.stroke().expect("cairo stroke");
    }
}

fn rounded_rect(cr: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    cr.new_sub_path();
    cr.arc(x + width - radius, y + radius, radius, -PI / 2.0, 0.0);
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        PI / 2.0,
    );
    cr.arc(x + radius, y + height - radius, radius, PI / 2.0, PI);
    cr.arc(x + radius, y + radius, radius, PI, 3.0 * PI / 2.0);
    cr.close_path();
}
#[derive(Clone)]
pub struct NetcoreSpinner {
    area: gtk::DrawingArea,
    angle: Rc<Cell<f64>>,
    source: Rc<RefCell<Option<glib::SourceId>>>,
}

impl NetcoreSpinner {
    pub fn new(size: i32) -> Self {
        let area = gtk::DrawingArea::new();
        area.set_content_width(size);
        area.set_content_height(size);
        area.set_size_request(size, size);
        area.set_halign(gtk::Align::Center);
        area.set_valign(gtk::Align::Center);
        area.set_can_focus(false);
        area.set_can_target(false);
        area.add_css_class("netcore-icon");

        let angle = Rc::new(Cell::new(0.0_f64));
        let draw_angle = Rc::clone(&angle);
        area.set_draw_func(move |area, cr, width, height| {
            let color = area.color();
            let scale = (width.min(height) as f64) / 24.0;
            if scale <= 0.0 {
                return;
            }
            cr.save().expect("cairo save");
            cr.translate(
                (f64::from(width) - 24.0 * scale) / 2.0,
                (f64::from(height) - 24.0 * scale) / 2.0,
            );
            cr.scale(scale, scale);
            cr.set_line_width(2.2);
            cr.set_line_cap(LineCap::Round);
            set_color(cr, &color, 0.18);
            cr.arc(12.0, 12.0, 7.0, 0.0, 2.0 * PI);
            cr.stroke().expect("cairo stroke");
            set_color(cr, &color, 1.0);
            let start = draw_angle.get();
            cr.arc(12.0, 12.0, 7.0, start, start + 1.8 * PI);
            cr.stroke().expect("cairo stroke");
            cr.restore().expect("cairo restore");
        });

        Self {
            area,
            angle,
            source: Rc::new(RefCell::new(None)),
        }
    }

    pub fn widget(&self) -> gtk::DrawingArea {
        self.area.clone()
    }

    pub fn set_visible(&self, visible: bool) {
        self.area.set_visible(visible);
    }

    pub fn start(&self) {
        if self.source.borrow().is_some() {
            return;
        }
        let weak = self.area.downgrade();
        let angle = Rc::clone(&self.angle);
        let id = glib::timeout_add_local(Duration::from_millis(45), move || {
            let Some(area) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            angle.set(angle.get() + 0.22);
            area.queue_draw();
            glib::ControlFlow::Continue
        });
        *self.source.borrow_mut() = Some(id);
    }

    pub fn stop(&self) {
        if let Some(id) = self.source.borrow_mut().take() {
            id.remove();
        }
    }
}
