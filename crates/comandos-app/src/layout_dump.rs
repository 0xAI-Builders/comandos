use gtk::prelude::*;
use serde_json::{Value, json};

pub fn capture(window: &gtk::Window) -> Value {
    let (width, height) = window.size();
    json!({
        "widget": window.widget_name().to_string(),
        "title": window.title().map(|s| s.to_string()),
        "geometry": {"width": width, "height": height},
        "children": children(window.upcast_ref::<gtk::Widget>()),
    })
}

fn children(widget: &gtk::Widget) -> Vec<Value> {
    let Some(container) = widget.dynamic_cast_ref::<gtk::Container>() else {
        return Vec::new();
    };
    container
        .children()
        .into_iter()
        .map(|child| {
            let alloc = child.allocation();
            json!({
                "widget": child.widget_name().to_string(),
                "type": child.type_().name(),
                "geometry": {
                    "x": alloc.x(),
                    "y": alloc.y(),
                    "width": alloc.width(),
                    "height": alloc.height(),
                },
                "children": children(&child),
            })
        })
        .collect()
}
