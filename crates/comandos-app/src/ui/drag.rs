//! Capa de dibujo del gesto propio: origen levantado, bandejas y fantasma.
use crate::workspace_view::{DragGesture, DragPhase};
use gtk::prelude::*;
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};
#[derive(Clone)]
pub struct DragLayer(pub Rc<Layer>);
pub struct Layer {
    pub widget: gtk::DrawingArea,
    pub gesture: RefCell<DragGesture>,
    pub target: RefCell<Option<Value>>,
    pub trays: RefCell<Vec<(String, [f64; 4])>>,
    pub source_label: RefCell<Option<gtk::Widget>>,
    pub text: RefCell<String>,
    pub dwell: RefCell<Option<String>>,
    colors: RefCell<([f64; 3], [f64; 3])>,
}
impl DragLayer {
    pub fn new() -> Self {
        let layer = Rc::new(Layer {
            widget: gtk::DrawingArea::new(),
            gesture: RefCell::new(DragGesture::default()),
            target: RefCell::new(None),
            trays: RefCell::new(Vec::new()),
            source_label: RefCell::new(None),
            text: RefCell::new(String::new()),
            dwell: RefCell::new(None),
            colors: RefCell::new(([0.086, 0.106, 0.133], [0.545, 0.486, 0.965])),
        });
        layer.widget.set_no_show_all(true);
        layer.widget.set_hexpand(true);
        layer.widget.set_vexpand(true);
        let weak = Rc::downgrade(&layer);
        layer.widget.connect_draw(move |_, cr| {
            if let Some(layer) = weak.upgrade() {
                draw(&layer, cr);
            }
            glib::Propagation::Proceed
        });
        Self(layer)
    }
    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.0.widget
    }
    pub fn colors(&self, bar: &str, brand: &str) {
        let rgb = |hex: &str| {
            let hex = hex.strip_prefix('#')?;
            if hex.len() != 6 {
                return None;
            }
            Some([
                u8::from_str_radix(hex.get(..2)?, 16).ok()? as f64 / 255.,
                u8::from_str_radix(hex.get(2..4)?, 16).ok()? as f64 / 255.,
                u8::from_str_radix(hex.get(4..6)?, 16).ok()? as f64 / 255.,
            ])
        };
        if let (Some(bar), Some(brand)) = (rgb(bar), rgb(brand)) {
            *self.0.colors.borrow_mut() = (bar, brand);
        }
    }
    pub fn active(&self) -> bool {
        matches!(
            self.0.gesture.borrow().phase,
            DragPhase::Lifted | DragPhase::Docking
        )
    }
    pub fn clear(&self) {
        self.0.target.borrow_mut().take();
        self.0.trays.borrow_mut().clear();
        self.0.source_label.borrow_mut().take();
        self.0.dwell.borrow_mut().take();
        self.widget().hide();
    }
    pub fn rect(&self, widget: &gtk::Widget) -> Option<[f64; 4]> {
        let (x, y) = widget.translate_coordinates(self.widget(), 0, 0)?;
        let r = widget.allocation();
        Some([x as f64, y as f64, r.width() as f64, r.height() as f64])
    }
}
impl Default for DragLayer {
    fn default() -> Self {
        Self::new()
    }
}
fn rounded(cr: &cairo::Context, [x, y, w, h]: [f64; 4], r: f64) {
    let r = r.max(0.).min(w / 2.).min(h / 2.);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.);
    cr.arc(x + w - r, y + h - r, r, 0., std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3. * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}
fn box_rect(value: &Value) -> Option<[f64; 4]> {
    let r = value.as_array()?;
    Some([
        r.first()?.as_f64()?,
        r.get(1)?.as_f64()?,
        r.get(2)?.as_f64()?,
        r.get(3)?.as_f64()?,
    ])
}
fn draw(layer: &Layer, cr: &cairo::Context) {
    let gesture = layer.gesture.borrow();
    if !matches!(gesture.phase, DragPhase::Lifted | DragPhase::Docking) {
        return;
    }
    let (bar, brand) = *layer.colors.borrow();
    if let Some(label) = layer.source_label.borrow().as_ref()
        && let Some((x, y)) = label.translate_coordinates(&layer.widget, 0, 0)
    {
        let r = label.allocation();
        cr.set_source_rgba(bar[0], bar[1], bar[2], 0.86);
        cr.rectangle(
            x as f64 - 1.,
            y as f64 - 1.,
            r.width() as f64 + 2.,
            r.height() as f64 + 2.,
        );
        let _ = cr.fill();
        cr.set_source_rgba(0., 0., 0., 0.18);
        rounded(
            cr,
            [
                x as f64 + 4.,
                y as f64 + 4.,
                (r.width() as f64 - 8.).max(0.),
                (r.height() as f64 - 8.).max(0.),
            ],
            7.,
        );
        let _ = cr.fill();
    }
    let target = layer.target.borrow();
    let hot = target
        .as_ref()
        .and_then(|v| v.get("mark"))
        .and_then(Value::as_str);
    for (mark, [x, y, w, h]) in layer.trays.borrow().iter() {
        let color = match mark.as_str() {
            "frozen" => [0.38, 0.65, 0.98],
            "awaiting_reply" => [0.96, 0.72, 0.24],
            "resolved" => [0.29, 0.87, 0.5],
            _ => [0.97, 0.44, 0.44],
        };
        let active = hot == Some(mark.as_str());
        cr.set_source_rgba(0.06, 0.09, 0.13, 0.94);
        cr.rectangle(*x, *y, *w, *h);
        let _ = cr.fill();
        cr.set_source_rgba(color[0], color[1], color[2], if active { 1. } else { 0.55 });
        cr.set_line_width(if active { 3. } else { 2. });
        cr.set_dash(if active { &[] } else { &[6., 4.] }, 0.);
        cr.rectangle(*x, *y, *w, *h);
        let _ = cr.stroke();
        cr.set_dash(&[], 0.);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(13.);
        let text = match mark.as_str() {
            "frozen" => "Aparcar",
            "awaiting_reply" => "Esperando",
            "resolved" => "Hecho",
            _ => "Quitar",
        };
        if let Ok(ext) = cr.text_extents(text) {
            cr.set_source_rgba(0.9, 0.93, 0.96, if active { 1. } else { 0.75 });
            cr.move_to(x + (w - ext.width()) / 2., y + h / 2. + ext.height() / 2.);
            let _ = cr.show_text(text);
        }
    }
    if let Some(target) = target.as_ref()
        && target.get("kind").and_then(Value::as_str) != Some("mark")
        && let Some(rect) = target.get("rect").and_then(box_rect)
    {
        let slot = target.get("slot").and_then(Value::as_bool) == Some(true);
        cr.set_source_rgba(brand[0], brand[1], brand[2], if slot { 0.14 } else { 0.18 });
        if slot {
            rounded(cr, rect, 8.);
        } else {
            let [x, y, w, h] = rect;
            cr.rectangle(x, y, w, h);
        }
        let _ = cr.fill_preserve();
        cr.set_source_rgba(brand[0], brand[1], brand[2], if slot { 0.9 } else { 0.95 });
        cr.set_line_width(2.);
        if slot {
            cr.set_dash(&[5., 4.], 0.);
        }
        let _ = cr.stroke();
        cr.set_dash(&[], 0.);
    }
    let label = layer.source_label.borrow();
    let text = layer.text.borrow();
    let (w, h) = if let Some(label) = label.as_ref() {
        let r = label.allocation();
        (r.width().max(24) as f64, r.height().max(16) as f64)
    } else {
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(12.);
        (cr.text_extents(&text).map_or(80., |e| e.width() + 4.), 18.)
    };
    let _ = cr.save();
    cr.translate(gesture.pointer.0, gesture.pointer.1);
    cr.rotate(-0.045);
    cr.translate(-w / 2., -h / 2.);
    let (extra_width, extra_height) = if label.is_some() {
        (10., 6.)
    } else {
        (16., 8.)
    };
    cr.set_source_rgba(0., 0., 0., 0.35);
    rounded(cr, [2., 5., w + extra_width, h + extra_height], 9.);
    let _ = cr.fill();
    cr.set_source_rgba(bar[0], bar[1], bar[2], 0.97);
    rounded(cr, [0., 0., w + extra_width, h + extra_height], 9.);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(brand[0], brand[1], brand[2], 0.9);
    cr.set_line_width(1.5);
    let _ = cr.stroke();
    if let Some(label) = label.as_ref() {
        cr.translate(5., 3.);
        cr.push_group();
        label.draw(cr);
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(0.95);
    } else if let Ok(ext) = cr.text_extents(&text) {
        cr.set_source_rgba(0.92, 0.94, 0.98, 1.);
        cr.move_to(8. - ext.x_bearing(), 4. + h / 2. + ext.height() / 2. - 1.);
        let _ = cr.show_text(&text);
    }
    let _ = cr.restore();
}
