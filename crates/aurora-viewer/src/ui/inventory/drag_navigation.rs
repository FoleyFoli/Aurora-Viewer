//! Inventory drag scrolling and the one-second folder hover from LLFolderView
//! (Firestorm indra/llui, LGPL 2.1). Latch hover visits so folders toggle once.
use super::*;

fn active(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.primary_down())
        && (egui::DragAndDrop::has_payload_of_type::<InvDrag>(ctx)
            || egui::DragAndDrop::has_payload_of_type::<FolderDrag>(ctx)
            || egui::DragAndDrop::has_payload_of_type::<SelectionDrag>(ctx)
            || egui::DragAndDrop::has_payload_of_type::<super::super::task_drop::TaskDrag>(ctx))
}

#[derive(Clone)]
struct Hover {
    folder: egui::Id,
    pass: u64,
    since: f64,
    toggled: bool,
}

/// Navigation does not move inventory, so it also works over the source
/// folder or a destination which refuses the actual drop.
pub(crate) fn folder(ui: &egui::Ui, r: &egui::Response, editor: Uuid, id: Uuid, open: &mut bool) {
    if !active(ui.ctx()) || !ui.rect_contains_pointer(r.rect) {
        return;
    }
    let key = egui::Id::unique(("inventory_drag_folder_visit", editor, id));
    let pass = ui.ctx().cumulative_pass_nr();
    let now = ui.input(|i| i.time);
    let toggle = ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(egui::Id::unique("inventory_drag_folder_hover"), || Hover {
            folder: key,
            pass,
            since: now,
            toggled: false,
        });
        if state.folder != key || state.pass.saturating_add(1) < pass {
            *state = Hover {
                folder: key,
                pass,
                since: now,
                toggled: false,
            };
        }
        state.pass = pass;
        let toggle = !state.toggled && now - state.since >= 1.0;
        state.toggled |= toggle;
        toggle
    });
    if toggle {
        *open = !*open;
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
}

#[derive(Clone)]
struct Owner {
    scroll: egui::Id,
    frame: u64,
}

pub(crate) fn scroll<R>(ui: &egui::Ui, out: &egui::scroll_area::ScrollAreaOutput<R>, editor: Uuid) {
    if cfg!(test) || demo::enabled() {
        let rect = out.inner_rect.intersect(ui.clip_rect());
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(("inventory_drag_viewport", editor)), (rect, out.id)));
    }
    scroll_view(ui, out, false, false);
}

/// Follow the last inventory pane entered even above/below the whole floater. A different
/// window under the pointer takes precedence. In Explorer's column list the same rule
/// applies to the left/right edges. Cursor distance determines speed on either side.
pub(crate) fn scroll_view<R>(ui: &egui::Ui, out: &egui::scroll_area::ScrollAreaOutput<R>, horizontal: bool, marquee: bool) {
    let ctx = ui.ctx();
    let owner_id = egui::Id::unique("inventory_drag_scroll_owner");
    let (pos, origin, held, dt) = ui.input(|i| (
        i.pointer.latest_pos(),
        i.pointer.press_origin(),
        i.pointer.primary_down() && !i.key_pressed(egui::Key::Escape),
        i.unstable_dt.clamp(0.0, 0.05),
    ));
    if !held || !(marquee || active(ctx) || super::super::organized::dragging(ctx)) {
        ctx.data_mut(|d| {
            if d.get_temp::<Owner>(owner_id).is_some_and(|o| o.scroll == out.id) {
                d.remove::<Owner>(owner_id);
            }
        });
        return;
    }
    let Some(pos) = pos else { return };
    let rect = out.inner_rect.intersect(ui.clip_rect());
    if !rect.is_positive() {
        return;
    }
    let layer = ctx.layer_id_at(pos);
    let over_window = layer == Some(ui.layer_id());
    let unobstructed = over_window || layer.is_none_or(|l| l.order == egui::Order::Background);
    let entered = over_window && rect.contains(pos);
    // A fast first motion can cross the window edge before the payload is installed.
    let started_here = origin.is_some_and(|p| rect.contains(p) && ctx.layer_id_at(p) == Some(ui.layer_id()));
    let frame = ctx.cumulative_frame_nr();
    let owns_frame = ctx.data_mut(|d| {
        let owner = d.get_temp::<Owner>(owner_id).filter(|o| o.frame.saturating_add(1) >= frame);
        let current = owner.as_ref().filter(|o| o.scroll == out.id);
        if current.is_none() && !entered && !(owner.is_none() && started_here) {
            return false;
        }
        let first_pass = current.is_none_or(|o| o.frame != frame);
        d.insert_temp(owner_id, Owner { scroll: out.id, frame });
        first_pass
    });
    if !owns_frame || !unobstructed {
        return;
    }
    let (axis, in_column) = if horizontal { (0, rect.y_range().contains(pos.y)) } else { (1, rect.x_range().contains(pos.x)) };
    if !in_column {
        return;
    }
    let speed = edge_speed(rect.min[axis], rect.max[axis], pos[axis]);
    let mut offset = out.state.clamped_offset();
    let max = (out.content_size[axis] - out.inner_rect.size()[axis]).max(0.0);
    let next = (offset[axis] + speed * dt).clamp(0.0, max);
    if next != offset[axis] {
        offset[axis] = next;
        crate::ui::widgets::scroll_offset::set(ctx, out.id, offset);
        ctx.request_repaint();
    }
}

fn edge_speed(start: f32, end: f32, pointer: f32) -> f32 {
    let zone = 26.0_f32.min((end - start) * 0.2);
    let past_start = (start + zone - pointer).max(0.0);
    let past_end = (pointer - (end - zone)).max(0.0);
    ((past_end - past_start) * 15.0).clamp(-2400.0, 2400.0)
}
