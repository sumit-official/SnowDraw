use cairo::{Context, Operator};
use gdk::prelude::*;
use gtk::prelude::*;
use glib::Propagation;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const ERASER_RADIUS: f64 = 14.0;

#[derive(Clone)]
struct Stroke {
    points: Vec<(f64, f64)>,
    color: (f64, f64, f64, f64),
    width: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Mouse,
    Pen,
    Eraser,
}

struct AppState {
    drawing: bool,
    tool: Tool,
    color: (f64, f64, f64),
    strokes: Vec<Stroke>,
    redo: Vec<Stroke>,
    width: f64,
    current: Option<Stroke>,
    erasing: bool,
    hover: Option<(f64, f64)>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            drawing: false,
            tool: Tool::Mouse,
            color: (0.98, 0.24, 0.34),
            strokes: Vec::new(),
            redo: Vec::new(),
            width: 4.0,
            current: None,
            erasing: false,
            hover: None,
        }
    }
}

thread_local! {
    // Toolbar rectangle in root (screen) coordinates: x, y, w, h.
    static TOOLBAR_RECT: Cell<(i32, i32, i32, i32)> = Cell::new((0, 0, 0, 0));
    // Top-left corner of the monitor the overlay covers.
    static MONITOR_ORIGIN: Cell<(i32, i32)> = Cell::new((0, 0));
    // Whether the overlay is currently capturing the mouse.
    static INPUT_ON: Cell<bool> = Cell::new(false);
}

/// Toolbar rectangle in overlay-local coordinates.
fn toolbar_rect_local() -> (i32, i32, i32, i32) {
    let (x, y, w, h) = TOOLBAR_RECT.with(|r| r.get());
    let (ox, oy) = MONITOR_ORIGIN.with(|o| o.get());
    (x - ox, y - oy, w, h)
}

fn dist_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= f64::EPSILON {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// Removes every stroke the eraser touches. Returns true if anything changed.
fn erase_at(state: &mut AppState, x: f64, y: f64) -> bool {
    let before = state.strokes.len();
    state.strokes.retain(|stroke| {
        let reach = ERASER_RADIUS + stroke.width / 2.0;
        let pts = &stroke.points;
        if pts.len() == 1 {
            return dist_to_segment((x, y), pts[0], pts[0]) > reach;
        }
        !pts.windows(2)
            .any(|seg| dist_to_segment((x, y), seg[0], seg[1]) <= reach)
    });
    state.strokes.len() != before
}

fn draw_strokes(cr: &Context, state: &AppState, width: i32, height: i32) {
    cr.set_operator(Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(Operator::Over);

    // Never paint annotations over the toolbar itself.
    let _ = cr.save();
    let (tx, ty, tw, th) = toolbar_rect_local();
    cr.set_fill_rule(cairo::FillRule::EvenOdd);
    cr.rectangle(0.0, 0.0, width as f64, height as f64);
    cr.rectangle(tx as f64, ty as f64, tw as f64, th as f64);
    cr.clip();

    let mut all = state.strokes.clone();
    if let Some(current) = &state.current {
        all.push(current.clone());
    }

    for stroke in all {
        if stroke.points.is_empty() {
            continue;
        }
        cr.set_source_rgba(stroke.color.0, stroke.color.1, stroke.color.2, stroke.color.3);
        cr.set_line_width(stroke.width);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        let (x, y) = stroke.points[0];
        cr.move_to(x, y);
        for &(x, y) in stroke.points.iter().skip(1) {
            cr.line_to(x, y);
        }
        if stroke.points.len() == 1 {
            cr.line_to(x + 0.1, y + 0.1);
        }
        let _ = cr.stroke();
    }

    // Eraser ring so you can see what will be erased.
    if state.drawing && state.tool == Tool::Eraser {
        if let Some((x, y)) = state.hover {
            cr.set_line_width(3.0);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
            cr.arc(x, y, ERASER_RADIUS, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.set_line_width(1.5);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
            cr.arc(x, y, ERASER_RADIUS, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
        }
    }
    let _ = cr.restore();
}

fn apply_input_mode(window: &gtk::Window, drawing: bool, width: i32, height: i32) {
    INPUT_ON.with(|i| i.set(drawing));
    if let Some(gdk_window) = window.window() {
        if drawing {
            // Whole screen captures the mouse EXCEPT the toolbar, so the toolbar
            // stays clickable and can never be drawn on.
            let rect = cairo::RectangleInt::new(0, 0, width.max(1), height.max(1));
            let region = cairo::Region::create_rectangle(&rect);
            let (tx, ty, tw, th) = toolbar_rect_local();
            let hole = cairo::RectangleInt::new(tx, ty, tw.max(0), th.max(0));
            let _ = region.subtract_rectangle(&hole);
            gdk_window.input_shape_combine_region(&region, 0, 0);
        } else {
            // An empty input region makes the overlay click-through in Interact mode.
            let empty = cairo::Region::create();
            gdk_window.input_shape_combine_region(&empty, 0, 0);
        }
    }
}

fn update_cursor(window: &gtk::Window, tool: Tool) {
    if let Some(gdk_window) = window.window() {
        let name = match tool {
            Tool::Mouse => None,
            Tool::Pen => Some("crosshair"),
            Tool::Eraser => Some("cell"),
        };
        let cursor = name.and_then(|n| gdk::Cursor::from_name(&gdk_window.display(), n));
        gdk_window.set_cursor(cursor.as_ref());
    }
}

fn confirm(parent: &gtk::Window, title: &str, message: &str) -> bool {
    let dialog = gtk::MessageDialog::new(
        Some(parent),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Warning,
        gtk::ButtonsType::YesNo,
        message,
    );
    dialog.set_title(title);
    let answer = dialog.run() == gtk::ResponseType::Yes;
    dialog.close();
    answer
}

fn main() {
    if let Err(err) = gtk::init() {
        eprintln!("SnowDraw could not start GTK: {err}");
        return;
    }

    // Use the installed "snowdraw" icon for every window / taskbar entry.
    gtk::Window::set_default_icon_name("snowdraw");

    let screen = match gdk::Screen::default() {
        Some(screen) => screen,
        None => {
            eprintln!("SnowDraw could not find an X11 screen.");
            return;
        }
    };
    // GTK 3.22+ exposes monitor information through GdkDisplay, not GdkScreen.
    let display = screen.display();
    let monitor = match display.primary_monitor().or_else(|| display.monitor(0)) {
        Some(monitor) => monitor,
        None => {
            eprintln!("SnowDraw could not find a monitor.");
            return;
        }
    };
    let monitor_geometry = monitor.geometry();
    let monitor_x = monitor_geometry.x();
    let monitor_y = monitor_geometry.y();
    let width = monitor_geometry.width();
    let height = monitor_geometry.height();
    if !screen.is_composited() {
        eprintln!("SnowDraw needs XFCE compositing enabled so the transparent overlay stays transparent.");
        return;
    }
    let rgba_visual = match screen.rgba_visual() {
        Some(visual) => visual,
        None => {
            eprintln!("SnowDraw needs an RGBA visual. Enable XFCE desktop compositing and try again.");
            return;
        }
    };

    let state = Rc::new(RefCell::new(AppState::default()));

    // Full-screen transparent annotation surface. The GDK input shape is empty
    // in Interact mode and full-screen in Draw mode.
    let overlay = gtk::Window::new(gtk::WindowType::Toplevel);
    overlay.set_title("SnowDraw overlay");
    overlay.set_decorated(false);
    overlay.set_app_paintable(true);
    overlay.set_accept_focus(false);
    overlay.set_focus_on_map(false);
    overlay.set_skip_taskbar_hint(true);
    overlay.set_skip_pager_hint(true);
    overlay.set_keep_above(true);
    overlay.set_visual(Some(&rgba_visual));
    overlay.set_type_hint(gdk::WindowTypeHint::Dock);
    overlay.set_widget_name("snowdraw-overlay");
    overlay.set_default_size(width, height);
    overlay.move_(monitor_x, monitor_y);

    let drawing_area = gtk::DrawingArea::new();
    drawing_area.set_size_request(width, height);
    drawing_area.set_widget_name("snowdraw-drawing-area");
    drawing_area.add_events(
        gdk::EventMask::BUTTON_PRESS_MASK
            | gdk::EventMask::BUTTON_RELEASE_MASK
            | gdk::EventMask::POINTER_MOTION_MASK,
    );
    overlay.add(&drawing_area);

    {
        let state = state.clone();
        drawing_area.connect_draw(move |_, cr| {
            draw_strokes(cr, &state.borrow(), width, height);
            Propagation::Proceed
        });
    }
    {
        let state = state.clone();
        let area = drawing_area.clone();
        drawing_area.connect_button_press_event(move |_, event| {
            if event.button() == 1 {
                let (x, y) = event.position();
                let mut s = state.borrow_mut();
                if s.drawing {
                    let tool = s.tool;
                    match tool {
                        Tool::Mouse => {}
                        Tool::Pen => {
                            let c = s.color;
                            let w = s.width;
                            s.current = Some(Stroke {
                                points: vec![(x, y)],
                                color: (c.0, c.1, c.2, 0.96),
                                width: w,
                            });
                        }
                        Tool::Eraser => {
                            s.erasing = true;
                            s.hover = Some((x, y));
                            erase_at(&mut s, x, y);
                        }
                    }
                    area.queue_draw();
                }
            }
            Propagation::Proceed
        });
    }
    {
        let state = state.clone();
        let area = drawing_area.clone();
        drawing_area.connect_motion_notify_event(move |_, event| {
            let (x, y) = event.position();
            let mut s = state.borrow_mut();
            if s.drawing && s.tool == Tool::Eraser {
                s.hover = Some((x, y));
                if s.erasing {
                    erase_at(&mut s, x, y);
                }
                area.queue_draw();
            } else if let Some(stroke) = s.current.as_mut() {
                stroke.points.push((x, y));
                area.queue_draw();
            }
            Propagation::Proceed
        });
    }
    {
        let state = state.clone();
        let area = drawing_area.clone();
        drawing_area.connect_button_release_event(move |_, event| {
            if event.button() == 1 {
                let mut state = state.borrow_mut();
                state.erasing = false;
                if let Some(stroke) = state.current.take() {
                    state.strokes.push(stroke);
                    state.redo.clear();
                }
                area.queue_draw();
            }
            Propagation::Proceed
        });
    }

    overlay.show_all();
    apply_input_mode(&overlay, false, width, height);

    // Floating toolbar is an independent, draggable window and is shown last
    // so it remains above the overlay.
    let toolbar = gtk::Window::new(gtk::WindowType::Toplevel);
    toolbar.set_title("SnowDraw Toolbar");
    toolbar.set_decorated(false);
    toolbar.set_keep_above(true);
    toolbar.set_skip_taskbar_hint(true);
    toolbar.set_skip_pager_hint(true);
    toolbar.set_app_paintable(true);
    toolbar.set_visual(Some(&rgba_visual));
    toolbar.set_widget_name("snowdraw-toolbar-window");
    toolbar.move_(monitor_x + 24, monitor_y + 24);

    let toolbar_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar_box.set_margin_top(10);
    toolbar_box.set_margin_bottom(14);
    toolbar_box.set_margin_start(12);
    toolbar_box.set_margin_end(12);
    toolbar_box.style_context().add_class("snowdraw-toolbar");

    // Drag handle (an EventBox so it really receives mouse events).
    let grip_label = gtk::Label::new(Some("❄  SnowDraw  ⋮⋮"));
    grip_label.style_context().add_class("snowdraw-grip");
    let grip = gtk::EventBox::new();
    grip.add(&grip_label);
    grip.set_tooltip_text(Some("Drag this area to move the toolbar"));

    // Tool selector: Mouse / Pencil / Eraser (exactly one is always active).
    let mouse_button = gtk::ToggleButton::with_label("↖  Mouse");
    mouse_button.set_tooltip_text(Some("Mouse: use your mouse normally, click through to your apps"));
    mouse_button.set_active(true);
    let pen_button = gtk::ToggleButton::with_label("✎  Pencil");
    pen_button.set_tooltip_text(Some("Pencil: draw on the screen"));
    let eraser_button = gtk::ToggleButton::with_label("⌫  Eraser");
    eraser_button.set_tooltip_text(Some("Eraser: click or drag over a stroke to remove it"));
    for b in [&mouse_button, &pen_button, &eraser_button] {
        b.style_context().add_class("snowdraw-tool");
    }
    let tool_group = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    tool_group.style_context().add_class("snowdraw-group");
    tool_group.pack_start(&mouse_button, false, false, 0);
    tool_group.pack_start(&pen_button, false, false, 0);
    tool_group.pack_start(&eraser_button, false, false, 0);

    // Color swatches + custom color.
    let presets: [(f64, f64, f64); 6] = [
        (0.98, 0.24, 0.34), // red
        (1.00, 0.84, 0.10), // yellow
        (0.20, 0.80, 0.35), // green
        (0.20, 0.55, 1.00), // blue
        (1.00, 1.00, 1.00), // white
        (0.05, 0.05, 0.05), // black
    ];
    let color_group = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    color_group.set_valign(gtk::Align::Center);
    let mut swatches: Vec<(gtk::Button, (f64, f64, f64))> = Vec::new();
    for (i, preset) in presets.iter().enumerate() {
        let swatch = gtk::Button::new();
        swatch.style_context().add_class("snowdraw-swatch");
        swatch.style_context().add_class(&format!("sw{i}"));
        swatch.set_valign(gtk::Align::Center);
        swatch.set_tooltip_text(Some("Pen color"));
        color_group.pack_start(&swatch, false, false, 0);
        swatches.push((swatch, *preset));
    }
    let color_button = gtk::Button::new();
    color_button.style_context().add_class("snowdraw-swatch");
    color_button.style_context().add_class("snowdraw-custom");
    color_button.set_valign(gtk::Align::Center);
    color_button.set_tooltip_text(Some("Pick any color"));
    color_group.pack_start(&color_button, false, false, 0);
    let swatches = Rc::new(swatches);

    let undo_button = gtk::Button::with_label("↶  Undo");
    undo_button.set_tooltip_text(Some("Undo the last stroke"));
    let redo_button = gtk::Button::with_label("↷  Redo");
    redo_button.set_tooltip_text(Some("Bring back the stroke you just undid"));

    // Pencil size: preset dots (thin -> thick), like Epic Pen.
    let size_presets: [f64; 5] = [2.0, 4.0, 8.0, 14.0, 22.0];
    let size_names = ["Thin", "Medium", "Thick", "Extra thick", "Huge"];
    let size_group = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    size_group.style_context().add_class("snowdraw-group");
    let mut size_items: Vec<(gtk::Button, gtk::DrawingArea, f64)> = Vec::new();
    for (i, size) in size_presets.iter().enumerate() {
        let size = *size;
        let button = gtk::Button::new();
        button.style_context().add_class("snowdraw-size");
        button.set_tooltip_text(Some(&format!("{} pencil ({} px)", size_names[i], size as i32)));
        let dot = gtk::DrawingArea::new();
        dot.set_size_request(26, 26);
        {
            let state = state.clone();
            dot.connect_draw(move |area, cr| {
                let color = state.borrow().color;
                let w = area.allocated_width() as f64;
                let h = area.allocated_height() as f64;
                let radius = (size / 2.0).min(11.0);
                // White halo so dark colors stay visible on the dark toolbar.
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
                cr.arc(w / 2.0, h / 2.0, radius + 1.5, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
                cr.set_source_rgba(color.0, color.1, color.2, 1.0);
                cr.arc(w / 2.0, h / 2.0, radius, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
                Propagation::Proceed
            });
        }
        button.add(&dot);
        size_group.pack_start(&button, false, false, 0);
        size_items.push((button, dot, size));
    }
    let size_items = Rc::new(size_items);
    // Highlight the selected size.
    let refresh_sizes: Rc<dyn Fn(f64)> = {
        let size_items = size_items.clone();
        Rc::new(move |width: f64| {
            for (button, dot, size) in size_items.iter() {
                if (*size - width).abs() < 0.01 {
                    button.style_context().add_class("selected");
                } else {
                    button.style_context().remove_class("selected");
                }
                dot.queue_draw();
            }
        })
    };
    (*refresh_sizes)(state.borrow().width);
    let clear_button = gtk::Button::with_label("Erase all");
    let quit_button = gtk::Button::with_label("✕  Exit");
    quit_button.style_context().add_class("snowdraw-quit");
    quit_button.set_tooltip_text(Some("Exit SnowDraw and close all its windows"));
    for b in [&undo_button, &redo_button, &clear_button] {
        b.style_context().add_class("snowdraw-action");
    }

    let separator = || {
        let sep = gtk::Separator::new(gtk::Orientation::Vertical);
        sep.style_context().add_class("snowdraw-sep");
        sep
    };
    toolbar_box.pack_start(&grip, false, false, 4);
    toolbar_box.pack_start(&separator(), false, false, 2);
    toolbar_box.pack_start(&tool_group, false, false, 0);
    toolbar_box.pack_start(&separator(), false, false, 2);
    toolbar_box.pack_start(&color_group, false, false, 4);
    toolbar_box.pack_start(&separator(), false, false, 2);
    toolbar_box.pack_start(&size_group, false, false, 0);
    toolbar_box.pack_start(&separator(), false, false, 2);
    toolbar_box.pack_start(&undo_button, false, false, 0);
    toolbar_box.pack_start(&redo_button, false, false, 0);
    toolbar_box.pack_start(&clear_button, false, false, 0);
    toolbar_box.pack_start(&separator(), false, false, 2);
    toolbar_box.pack_start(&quit_button, false, false, 0);
    toolbar.add(&toolbar_box);

    // Remember where the toolbar is so the overlay can leave a hole for it.
    MONITOR_ORIGIN.with(|o| o.set((monitor_x, monitor_y)));
    TOOLBAR_RECT.with(|r| r.set((monitor_x + 24, monitor_y + 24, 900, 70)));
    {
        let overlay = overlay.clone();
        let area = drawing_area.clone();
        toolbar.connect_configure_event(move |_, event| {
            let (x, y) = event.position();
            let (w, h) = event.size();
            TOOLBAR_RECT.with(|r| r.set((x as i32, y as i32, w as i32, h as i32)));
            if INPUT_ON.with(|i| i.get()) {
                apply_input_mode(&overlay, true, width, height);
            }
            area.queue_draw();
            // configure-event returns a plain bool: false = let GTK keep handling it.
            false
        });
    }

    // One place that switches tool. Mouse = click-through; Pencil/Eraser = capture.
    let select_tool: Rc<dyn Fn(Tool)> = {
        let state = state.clone();
        let overlay = overlay.clone();
        let toolbar = toolbar.clone();
        let mouse = mouse_button.clone();
        let pen = pen_button.clone();
        let eraser = eraser_button.clone();
        // GTK's ToggleButton::set_active() emits "clicked", which would call us
        // again and recurse forever, so ignore re-entrant calls.
        let busy = Rc::new(Cell::new(false));
        Rc::new(move |tool: Tool| {
            if busy.get() {
                return;
            }
            busy.set(true);
            let on = tool != Tool::Mouse;
            {
                let mut s = state.borrow_mut();
                s.tool = tool;
                s.drawing = on;
                s.current = None;
                s.erasing = false;
                s.hover = None;
            }
            mouse.set_active(tool == Tool::Mouse);
            pen.set_active(tool == Tool::Pen);
            eraser.set_active(tool == Tool::Eraser);
            busy.set(false);
            apply_input_mode(&overlay, on, width, height);
            update_cursor(&overlay, tool);
            overlay.queue_draw();
            if on {
                overlay.present();
                toolbar.present();
            }
        })
    };
    for (button, tool) in [
        (&mouse_button, Tool::Mouse),
        (&pen_button, Tool::Pen),
        (&eraser_button, Tool::Eraser),
    ] {
        let select_tool = select_tool.clone();
        button.connect_clicked(move |_| (*select_tool)(tool));
    }

    // Highlight the swatch that matches the current color (or the custom button).
    let refresh_ring: Rc<dyn Fn((f64, f64, f64))> = {
        let swatches = swatches.clone();
        let color_button = color_button.clone();
        Rc::new(move |c: (f64, f64, f64)| {
            let mut matched = false;
            for (swatch, preset) in swatches.iter() {
                let same = (preset.0 - c.0).abs() < 0.01
                    && (preset.1 - c.1).abs() < 0.01
                    && (preset.2 - c.2).abs() < 0.01;
                matched |= same;
                if same {
                    swatch.style_context().add_class("selected");
                } else {
                    swatch.style_context().remove_class("selected");
                }
            }
            if matched {
                color_button.style_context().remove_class("selected");
            } else {
                color_button.style_context().add_class("selected");
            }
        })
    };
    (*refresh_ring)(state.borrow().color);

    // Picking a color switches to the pencil.
    let set_color: Rc<dyn Fn((f64, f64, f64))> = {
        let state = state.clone();
        let select_tool = select_tool.clone();
        let refresh_ring = refresh_ring.clone();
        let refresh_sizes = refresh_sizes.clone();
        Rc::new(move |c: (f64, f64, f64)| {
            let width = {
                let mut s = state.borrow_mut();
                s.color = c;
                s.width
            };
            (*refresh_sizes)(width);
            (*refresh_ring)(c);
            (*select_tool)(Tool::Pen);
        })
    };
    for (swatch, preset) in swatches.iter() {
        let set_color = set_color.clone();
        let preset = *preset;
        swatch.connect_clicked(move |_| (*set_color)(preset));
    }
    {
        let state = state.clone();
        let overlay = overlay.clone();
        let toolbar = toolbar.clone();
        let set_color = set_color.clone();
        color_button.connect_clicked(move |_| {
            // Let the dialog receive the mouse while it is open.
            let was_on = INPUT_ON.with(|i| i.get());
            apply_input_mode(&overlay, false, width, height);
            let dialog = gtk::ColorChooserDialog::new(Some("Pick a pen color"), Some(&toolbar));
            dialog.set_modal(true);
            dialog.set_use_alpha(false);
            let c = state.borrow().color;
            dialog.set_rgba(&gdk::RGBA::new(c.0, c.1, c.2, 1.0));
            let accepted = dialog.run() == gtk::ResponseType::Ok;
            let rgba = dialog.rgba();
            dialog.close();
            apply_input_mode(&overlay, was_on, width, height);
            if accepted {
                (*set_color)((rgba.red(), rgba.green(), rgba.blue()));
            }
            toolbar.present();
        });
    }

    {
        let toolbar = toolbar.clone();
        grip.add_events(gdk::EventMask::BUTTON_PRESS_MASK);
        grip.connect_button_press_event(move |_, event| {
            if event.button() == 1 {
                if let Some(gdk_window) = toolbar.window() {
                    let (root_x, root_y) = event.root();
                    gdk_window.begin_move_drag(
                        1,
                        root_x as i32,
                        root_y as i32,
                        event.time(),
                    );
                }
            }
            Propagation::Proceed
        });
    }
    {
        let state = state.clone();
        let area = drawing_area.clone();
        undo_button.connect_clicked(move |_| {
            let mut s = state.borrow_mut();
            if let Some(stroke) = s.strokes.pop() {
                s.redo.push(stroke);
            }
            area.queue_draw();
        });
    }
    {
        let state = state.clone();
        let area = drawing_area.clone();
        redo_button.connect_clicked(move |_| {
            let mut s = state.borrow_mut();
            if let Some(stroke) = s.redo.pop() {
                s.strokes.push(stroke);
            }
            area.queue_draw();
        });
    }
    // Pencil size presets.
    for (button, _, size) in size_items.iter() {
        let state = state.clone();
        let refresh_sizes = refresh_sizes.clone();
        let size = *size;
        button.connect_clicked(move |_| {
            state.borrow_mut().width = size;
            (*refresh_sizes)(size);
        });
    }
    {
        let state = state.clone();
        let area = drawing_area.clone();
        let toolbar = toolbar.clone();
        let overlay = overlay.clone();
        clear_button.connect_clicked(move |_| {
            let was_drawing = state.borrow().drawing;
            apply_input_mode(&overlay, false, width, height);
            if confirm(&toolbar, "Erase all annotations?", "Erase every annotation on the desktop?") {
                let mut state = state.borrow_mut();
                state.strokes.clear();
                state.redo.clear();
                state.current = None;
                area.queue_draw();
            }
            apply_input_mode(&overlay, was_drawing, width, height);
            toolbar.present();
        });
    }
    {
        let toolbar = toolbar.clone();
        let overlay = overlay.clone();
        quit_button.connect_clicked(move |_| {
            // Close every SnowDraw window before leaving the GTK main loop.
            // This also removes the full-screen overlay immediately.
            overlay.close();
            toolbar.close();
            gtk::main_quit();
        });
    }

    let mut css_text = String::from(
        "
        #snowdraw-overlay, #snowdraw-drawing-area {
            background-color: transparent;
        }
        #snowdraw-toolbar-window {
            background-color: transparent;
        }
        .snowdraw-toolbar {
            background: linear-gradient(180deg, rgba(38, 42, 56, 0.97), rgba(22, 24, 33, 0.97));
            color: #f4f5f7;
            border: 1px solid rgba(255, 255, 255, 0.14);
            border-radius: 18px;
            padding: 8px 12px;
            box-shadow: 0 6px 18px rgba(0, 0, 0, 0.55);
        }
        .snowdraw-grip {
            color: #aab3c5;
            font-weight: bold;
            letter-spacing: 1px;
        }
        .snowdraw-sep {
            background: rgba(255, 255, 255, 0.14);
            min-width: 1px;
            margin: 6px 2px;
        }
        .snowdraw-group {
            background: rgba(255, 255, 255, 0.07);
            border-radius: 13px;
            padding: 3px;
        }
        .snowdraw-toolbar button {
            color: #e6e9f0;
            background-image: none;
            background-color: transparent;
            border: 1px solid transparent;
            border-radius: 10px;
            box-shadow: none;
            text-shadow: none;
            padding: 6px 12px;
        }
        .snowdraw-toolbar button:hover {
            background-color: rgba(255, 255, 255, 0.12);
        }
        .snowdraw-toolbar button.snowdraw-tool:checked {
            background-image: linear-gradient(135deg, #5b8cff, #7a5cff);
            color: #ffffff;
            font-weight: bold;
        }
        .snowdraw-toolbar button.snowdraw-size {
            padding: 2px;
            min-width: 30px;
            min-height: 30px;
        }
        .snowdraw-toolbar button.snowdraw-size.selected {
            background-color: rgba(91, 140, 255, 0.35);
            border-color: #8fb0f0;
        }
        .snowdraw-toolbar button.snowdraw-action {
            background-color: rgba(255, 255, 255, 0.08);
        }
        .snowdraw-toolbar button.snowdraw-action:hover {
            background-color: rgba(255, 255, 255, 0.18);
        }
        .snowdraw-toolbar button.snowdraw-quit {
            background-color: rgba(255, 77, 94, 0.18);
            color: #ff8794;
        }
        .snowdraw-toolbar button.snowdraw-quit:hover {
            background-color: #ff4d5e;
            color: #ffffff;
        }
        .snowdraw-toolbar button.snowdraw-swatch {
            min-width: 20px;
            min-height: 20px;
            padding: 0;
            border-radius: 999px;
            border: 2px solid rgba(255, 255, 255, 0.35);
        }
        .snowdraw-toolbar button.snowdraw-swatch:hover {
            border-color: rgba(255, 255, 255, 0.85);
        }
        .snowdraw-toolbar button.snowdraw-swatch.selected {
            border-color: #ffffff;
            box-shadow: 0 0 0 2px #5b8cff;
        }
        .snowdraw-toolbar button.snowdraw-custom {
            background-image: linear-gradient(135deg, #ff4d6d, #ffd60a, #34d399, #3b82f6, #a855f7);
        }
    ",
    );
    for (i, p) in presets.iter().enumerate() {
        css_text.push_str(&format!(
            ".snowdraw-toolbar button.sw{i} {{ background-image: none; background-color: rgb({}, {}, {}); }}\n",
            (p.0 * 255.0).round() as u8,
            (p.1 * 255.0).round() as u8,
            (p.2 * 255.0).round() as u8
        ));
    }
    let css = gtk::CssProvider::new();
    if css.load_from_data(css_text.as_bytes()).is_ok() {
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    toolbar.show_all();
    toolbar.present();
    gtk::main();
}
